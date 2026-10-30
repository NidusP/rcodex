use std::{
    collections::{HashSet, VecDeque},
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use reqwest::Client;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    diagnostics::{CheckStatus, ConnectivityStatus, ProbeSnapshot, ProviderModelOption},
    providers::TransportMode,
    routing::is_internal_codex_model_id,
    secrets::validate_api_key,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const TICKET_TTL_SECS: i64 = 10 * 60;
const TICKET_LIMIT: usize = 8;
const MAX_TEXT_PROBE_MODELS: usize = 8;

#[derive(Clone)]
pub struct ProbeCall {
    pub route: &'static str,
    pub model: Option<String>,
    pub success: bool,
    pub http_status: Option<u16>,
    pub error_code: Option<String>,
    pub latency_ms: i64,
    pub transport_mode: TransportMode,
}

#[derive(Clone)]
pub struct ProbeExecution {
    pub snapshot: ProbeSnapshot,
    pub calls: Vec<ProbeCall>,
}

#[derive(Clone)]
struct ProbeTicket {
    id: String,
    base_url: String,
    key_fingerprint: String,
    execution: ProbeExecution,
    created_at: i64,
}

pub enum TicketMatch {
    Missing,
    Mismatched,
    Matched(ProbeExecution),
}

#[derive(Default)]
pub struct ProbeTicketStore {
    tickets: Mutex<VecDeque<ProbeTicket>>,
}

impl ProbeTicketStore {
    pub fn insert(&self, base_url: &str, api_key: &str, execution: ProbeExecution) -> String {
        let id = Uuid::new_v4().to_string();
        let mut tickets = self.tickets.lock().expect("probe ticket lock");
        let now = unix_now();
        tickets.retain(|ticket| now.saturating_sub(ticket.created_at) < TICKET_TTL_SECS);
        while tickets.len() >= TICKET_LIMIT {
            tickets.pop_front();
        }
        tickets.push_back(ProbeTicket {
            id: id.clone(),
            base_url: base_url.to_string(),
            key_fingerprint: key_fingerprint(api_key),
            execution,
            created_at: now,
        });
        id
    }

    pub fn take_matching(&self, id: &str, base_url: &str, api_key: &str) -> TicketMatch {
        let mut tickets = self.tickets.lock().expect("probe ticket lock");
        let now = unix_now();
        tickets.retain(|ticket| now.saturating_sub(ticket.created_at) < TICKET_TTL_SECS);
        let Some(index) = tickets.iter().position(|ticket| ticket.id == id) else {
            return TicketMatch::Missing;
        };
        let ticket = tickets.remove(index).expect("ticket index exists");
        if ticket.base_url == base_url && ticket.key_fingerprint == key_fingerprint(api_key) {
            TicketMatch::Matched(ticket.execution)
        } else {
            TicketMatch::Mismatched
        }
    }
}

pub fn probe_client() -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| format!("无法创建渠道探针客户端：{error}"))
}

pub fn transport_for_wire(wire_api: &str) -> Option<TransportMode> {
    match wire_api {
        "responses" => Some(TransportMode::GatewayPassthrough),
        "chat_completions" => Some(TransportMode::GatewayChatCompletions),
        _ => None,
    }
}

pub fn validate_draft(base_url: String, api_key: String) -> Result<(String, String), String> {
    let base_url = crate::providers::validate_base_url(base_url)?;
    let api_key = validate_api_key(api_key)?;
    Ok((base_url, api_key.expose().to_string()))
}

pub async fn execute(client: &Client, base_url: &str, api_key: &str) -> ProbeExecution {
    let mut calls = Vec::new();
    let models = match load_models(client, base_url, api_key, &mut calls).await {
        Ok(models) => models,
        Err(execution) => return execution,
    };
    classify_text(client, base_url, api_key, models, calls).await
}

async fn load_models(
    client: &Client,
    base_url: &str,
    api_key: &str,
    calls: &mut Vec<ProbeCall>,
) -> Result<Vec<ProviderModelOption>, ProbeExecution> {
    let mut routes = vec!["/models"];
    if !prefix_ends_with_v1(base_url) {
        routes.push("/v1/models");
    }
    let mut last_empty = None;
    for route in routes {
        let started = Instant::now();
        let url = join_probe_url(base_url, route);
        match client.get(&url).bearer_auth(api_key).send().await {
            Ok(response) if response.status().is_success() => {
                let status = response.status().as_u16();
                match response.bytes().await {
                    Ok(body) => match serde_json::from_slice::<Value>(&body) {
                        Ok(value) => {
                            let models = parse_models(&value);
                            if models.is_empty() {
                                let call = call(
                                    route,
                                    None,
                                    false,
                                    Some(status),
                                    Some("models_empty".to_string()),
                                    started.elapsed(),
                                    TransportMode::GatewayPassthrough,
                                );
                                calls.push(call.clone());
                                last_empty = Some(call);
                            } else {
                                calls.push(call(
                                    route,
                                    None,
                                    true,
                                    Some(status),
                                    None,
                                    started.elapsed(),
                                    TransportMode::GatewayPassthrough,
                                ));
                                return Ok(models);
                            }
                        }
                        Err(_) => calls.push(call(
                            route,
                            None,
                            false,
                            Some(status),
                            Some("invalid_models_response".to_string()),
                            started.elapsed(),
                            TransportMode::GatewayPassthrough,
                        )),
                    },
                    Err(_) => calls.push(call(
                        route,
                        None,
                        false,
                        Some(status),
                        Some("models_body_unavailable".to_string()),
                        started.elapsed(),
                        TransportMode::GatewayPassthrough,
                    )),
                }
            }
            Ok(response) => {
                let status = response.status().as_u16();
                let body = response.bytes().await.unwrap_or_default();
                let code = upstream_error_code(status, &body);
                calls.push(call(
                    route,
                    None,
                    false,
                    Some(status),
                    Some(code.to_string()),
                    started.elapsed(),
                    TransportMode::GatewayPassthrough,
                ));
            }
            Err(error) => calls.push(call(
                route,
                None,
                false,
                None,
                Some(transport_error_code(&error).to_string()),
                started.elapsed(),
                TransportMode::GatewayPassthrough,
            )),
        }
    }
    if let Some(empty) = last_empty {
        return Err(finish(
            ConnectivityStatus::Degraded,
            None,
            None,
            None,
            Vec::new(),
            CheckStatus::Ready,
            CheckStatus::NotRun,
            empty.error_code,
            calls.clone(),
        ));
    }
    let status = if calls
        .iter()
        .any(|call| matches!(call.http_status, Some(401 | 403)))
    {
        ConnectivityStatus::Unauthorized
    } else {
        calls
            .last()
            .map(call_status)
            .unwrap_or(ConnectivityStatus::Degraded)
    };
    let error_code = calls.last().and_then(|call| call.error_code.clone());
    Err(finish(
        status,
        None,
        None,
        None,
        Vec::new(),
        CheckStatus::Failed,
        CheckStatus::NotRun,
        error_code.or_else(|| Some("models_empty".to_string())),
        calls.clone(),
    ))
}

struct TextTarget {
    route: &'static str,
    wire_api: &'static str,
    transport_mode: TransportMode,
}

fn text_targets(base_url: &str) -> Vec<TextTarget> {
    let mut targets = vec![TextTarget {
        route: "/responses",
        wire_api: "responses",
        transport_mode: TransportMode::GatewayPassthrough,
    }];
    if !prefix_ends_with_v1(base_url) {
        targets.push(TextTarget {
            route: "/v1/responses",
            wire_api: "responses",
            transport_mode: TransportMode::GatewayPassthrough,
        });
    }
    targets.push(TextTarget {
        route: "/chat/completions",
        wire_api: "chat_completions",
        transport_mode: TransportMode::GatewayChatCompletions,
    });
    if !prefix_ends_with_v1(base_url) {
        targets.push(TextTarget {
            route: "/v1/chat/completions",
            wire_api: "chat_completions",
            transport_mode: TransportMode::GatewayChatCompletions,
        });
    }
    targets
}

fn prefix_ends_with_v1(base_url: &str) -> bool {
    let Some(scheme) = base_url.find("://") else {
        return false;
    };
    let after = &base_url[scheme + 3..];
    let path = after.find('/').map(|index| &after[index..]).unwrap_or("");
    let path = path.trim_end_matches('/');
    path == "/v1" || path.ends_with("/v1")
}

fn join_probe_url(base_url: &str, route: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), route)
}

pub(crate) fn confirmed_base_for_route(base_url: &str, route: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if route.starts_with("/v1/") {
        format!("{base}/v1")
    } else {
        base.to_string()
    }
}

async fn classify_text(
    client: &Client,
    base_url: &str,
    api_key: &str,
    models: Vec<ProviderModelOption>,
    mut calls: Vec<ProbeCall>,
) -> ProbeExecution {
    let probe_models = text_probe_models(&models);
    for target in text_targets(base_url) {
        for model in &probe_models {
            match post_text(
                client,
                base_url,
                api_key,
                model,
                target.route,
                if target.wire_api == "responses" {
                    responses_body(model)
                } else {
                    chat_body(model)
                },
                target.transport_mode,
            )
            .await
            {
                TextAttempt::Ready(call) => {
                    let confirmed = confirmed_base_for_route(base_url, target.route);
                    calls.push(call);
                    return finish(
                        ConnectivityStatus::Ready,
                        Some(target.wire_api.to_string()),
                        Some(confirmed),
                        Some(model.clone()),
                        models,
                        CheckStatus::Ready,
                        CheckStatus::Ready,
                        None,
                        calls,
                    );
                }
                TextAttempt::Failed(call) => {
                    let stop_model_attempts = matches!(
                        call.error_code.as_deref(),
                        Some("upstream_route_unsupported") | Some("upstream_unauthorized")
                    );
                    calls.push(call);
                    if stop_model_attempts {
                        break;
                    }
                }
            }
        }
    }
    let error_code = calls.last().and_then(|call| call.error_code.clone());
    let status = calls
        .last()
        .map(call_status)
        .unwrap_or(ConnectivityStatus::Degraded);
    finish(
        status,
        None,
        None,
        probe_models.first().cloned(),
        models,
        CheckStatus::Ready,
        CheckStatus::Failed,
        error_code,
        calls,
    )
}

enum TextAttempt {
    Ready(ProbeCall),
    Failed(ProbeCall),
}

async fn post_text(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    route: &'static str,
    body: Value,
    transport_mode: TransportMode,
) -> TextAttempt {
    let started = Instant::now();
    let url = join_probe_url(base_url, route);
    let failed = |http_status, error_code, elapsed| {
        TextAttempt::Failed(call(
            route,
            Some(model.to_string()),
            false,
            http_status,
            error_code,
            elapsed,
            transport_mode,
        ))
    };
    match client
        .post(url)
        .bearer_auth(api_key)
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&body).expect("probe JSON is serializable"))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            let status = response.status().as_u16();
            let valid = response
                .bytes()
                .await
                .ok()
                .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
                .is_some();
            if valid {
                TextAttempt::Ready(call(
                    route,
                    Some(model.to_string()),
                    true,
                    Some(status),
                    None,
                    started.elapsed(),
                    transport_mode,
                ))
            } else {
                failed(
                    Some(status),
                    Some("invalid_text_response".to_string()),
                    started.elapsed(),
                )
            }
        }
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.bytes().await.unwrap_or_default();
            let code = upstream_error_code(status, &body).to_string();
            failed(Some(status), Some(code), started.elapsed())
        }
        Err(error) => failed(
            None,
            Some(transport_error_code(&error).to_string()),
            started.elapsed(),
        ),
    }
}

fn responses_body(model: &str) -> Value {
    serde_json::json!({
        "model": model,
        "input": "Reply with OK.",
        "max_output_tokens": 8,
        "stream": false
    })
}

fn chat_body(model: &str) -> Value {
    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Reply with OK."}],
        "max_tokens": 8,
        "stream": false
    })
}

fn text_probe_models(models: &[ProviderModelOption]) -> Vec<String> {
    let mut candidates = models
        .iter()
        .filter(|model| is_text_probe_model(&model.upstream_model_id))
        .collect::<Vec<_>>();
    candidates.sort_by_key(|model| probe_model_priority(&model.upstream_model_id));
    let candidates = candidates
        .into_iter()
        .map(|model| model.upstream_model_id.clone())
        .take(MAX_TEXT_PROBE_MODELS)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        models
            .iter()
            .map(|model| model.upstream_model_id.clone())
            .take(MAX_TEXT_PROBE_MODELS)
            .collect()
    } else {
        candidates
    }
}

fn is_text_probe_model(model_id: &str) -> bool {
    if is_internal_codex_model_id(model_id) {
        return false;
    }
    let normalized = model_id.trim().to_ascii_lowercase();
    !normalized.contains("imagine")
        && !normalized.contains("dall-e")
        && !normalized.contains("dalle")
        && !normalized.contains("gpt-image")
        && !normalized.starts_with("image-")
}

/// Prefer likely mainstream text models as probe candidates without treating
/// their names as evidence of protocol support. A successful request remains
/// the only confirmation; this ranking only avoids spending the first probe on
/// a relay's internal, preview, or non-conversational entry.
fn probe_model_priority(model_id: &str) -> u8 {
    let normalized = model_id.trim().to_ascii_lowercase();
    if normalized.contains("grok-4")
        || normalized.contains("deepseek-")
        || normalized.contains("glm-5")
        || normalized.contains("qwen3.")
        || normalized.contains("qwen3-")
        || normalized.contains("claude-sonnet")
        || normalized.contains("claude-opus")
        || normalized.contains("anthropic-")
    {
        return 0;
    }
    if normalized.starts_with("gpt-") {
        if normalized.contains("terra") {
            return 1;
        }
        if normalized.contains("astra") {
            return 2;
        }
        if normalized.contains("sol") || normalized.contains("luna") {
            return 3;
        }
        return 10;
    }
    if normalized.contains("chat")
        || normalized.contains("reasoner")
        || normalized.contains("sonnet")
        || normalized.contains("opus")
        || normalized.contains("-plus")
        || normalized.contains("-max")
    {
        return 5;
    }
    20
}

fn finish(
    status: ConnectivityStatus,
    wire_api: Option<String>,
    confirmed_base_url: Option<String>,
    model: Option<String>,
    models: Vec<ProviderModelOption>,
    models_check: CheckStatus,
    text_check: CheckStatus,
    error_code: Option<String>,
    calls: Vec<ProbeCall>,
) -> ProbeExecution {
    let confirmed = matches!(status, ConnectivityStatus::Ready)
        && matches!(models_check, CheckStatus::Ready)
        && matches!(text_check, CheckStatus::Ready);
    let wire_api = if confirmed {
        wire_api.filter(|value| matches!(value.as_str(), "responses" | "chat_completions"))
    } else {
        None
    };
    let confirmed_base_url = if wire_api.is_some() {
        confirmed_base_url
    } else {
        None
    };
    ProbeExecution {
        snapshot: ProbeSnapshot {
            status,
            wire_api,
            confirmed_base_url,
            model,
            models,
            models_check,
            text_check,
            error_code,
        },
        calls,
    }
}

fn call(
    route: &'static str,
    model: Option<String>,
    success: bool,
    http_status: Option<u16>,
    error_code: Option<String>,
    elapsed: Duration,
    transport_mode: TransportMode,
) -> ProbeCall {
    ProbeCall {
        route,
        model,
        success,
        http_status,
        error_code,
        latency_ms: i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
        transport_mode,
    }
}

fn call_status(call: &ProbeCall) -> ConnectivityStatus {
    match call.http_status {
        Some(401 | 403) => ConnectivityStatus::Unauthorized,
        None if call.error_code.as_deref() == Some("upstream_timeout")
            || call.error_code.as_deref() == Some("upstream_unreachable") =>
        {
            ConnectivityStatus::Unreachable
        }
        _ => ConnectivityStatus::Degraded,
    }
}

fn route_is_missing(status: u16, body: &[u8]) -> bool {
    if matches!(status, 404 | 405) {
        return true;
    }
    if status != 400 {
        return false;
    }
    let text = String::from_utf8_lossy(body).to_ascii_lowercase();
    if text.contains("model") {
        return false;
    }
    [
        "unknown url",
        "unknown route",
        "no route",
        "invalid path",
        "endpoint not found",
        "path not found",
        "route not found",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn parse_models(value: &Value) -> Vec<ProviderModelOption> {
    let Some(entries) = value.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    entries
        .iter()
        .filter_map(|entry| {
            let id = entry
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| {
                    !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
                })?
                .to_owned();
            if !seen.insert(id.clone()) {
                return None;
            }
            let display_name = entry
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| {
                    !name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
                })
                .unwrap_or(&id)
                .to_owned();
            let owned_by = entry
                .get("owned_by")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|owner| {
                    !owner.is_empty() && owner.len() <= 128 && !owner.chars().any(char::is_control)
                })
                .map(str::to_owned);
            Some(ProviderModelOption {
                upstream_model_id: id,
                display_name,
                owned_by,
            })
        })
        .take(1024)
        .collect()
}

fn upstream_error_code(status: u16, body: &[u8]) -> &'static str {
    if route_is_missing(status, body) {
        "upstream_route_unsupported"
    } else if matches!(status, 401 | 403) || body_indicates_auth_failure(body) {
        "upstream_unauthorized"
    } else {
        match status {
            404 | 405 => "upstream_route_unsupported",
            _ => "upstream_http_error",
        }
    }
}

fn body_indicates_auth_failure(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    [
        "authentication failed",
        "invalid api key",
        "invalid_api_key",
        "unauthorized",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn transport_error_code(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "upstream_timeout"
    } else if error.is_connect() {
        "upstream_unreachable"
    } else {
        "upstream_transport_error"
    }
}

fn key_fingerprint(api_key: &str) -> String {
    let digest = Sha256::digest(api_key.as_bytes());
    format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use axum::{
        http::StatusCode,
        response::IntoResponse,
        routing::{get, post},
        Json, Router,
    };
    use tokio::net::TcpListener;

    use super::*;

    async fn serve(router: Router) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        base_url
    }

    fn models() -> Json<Value> {
        Json(serde_json::json!({
            "data": [
                {"id": "alpha", "name": "Alpha", "owned_by": "lab"}
            ]
        }))
    }

    #[test]
    fn model_error_is_not_a_missing_route() {
        let body = br#"{"error":{"message":"model not found"}}"#;
        assert!(!route_is_missing(400, body));
        assert!(route_is_missing(404, body));
        assert!(route_is_missing(
            400,
            br#"{"error":{"message":"unknown route"}}"#
        ));
    }

    #[tokio::test]
    async fn responses_success_does_not_call_chat() {
        let chat_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits = chat_hits.clone();
        let router = Router::new()
            .route("/models", get(move || async { models() }))
            .route(
                "/responses",
                post(|| async { Json(serde_json::json!({"ok": true})) }),
            )
            .route(
                "/chat/completions",
                post(move || {
                    hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    async { StatusCode::OK.into_response() }
                }),
            );
        let base_url = serve(router).await;
        let client = probe_client().unwrap();
        let execution = execute(&client, &base_url, "sk-test").await;
        assert_eq!(execution.snapshot.wire_api.as_deref(), Some("responses"));
        assert_eq!(
            execution.snapshot.confirmed_base_url.as_deref(),
            Some(base_url.as_str())
        );
        assert!(matches!(
            execution.snapshot.status,
            ConnectivityStatus::Ready
        ));
        assert_eq!(execution.snapshot.models.len(), 1);
        assert_eq!(chat_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(execution.calls.len(), 2);
        assert_eq!(execution.calls[1].route, "/responses");
    }

    #[tokio::test]
    async fn responses_probe_skips_internal_model_and_retries_another_candidate() {
        let chat_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits = chat_hits.clone();
        let router = Router::new()
            .route(
                "/models",
                get(|| async {
                    Json(serde_json::json!({
                        "data": [
                            {"id": "codex-auto-review", "owned_by": "openai"},
                            {"id": "gpt-5.2", "owned_by": "openai"},
                            {"id": "gpt-5.6-terra", "owned_by": "openai"}
                        ]
                    }))
                }),
            )
            .route(
                "/responses",
                post(|Json(body): Json<Value>| async move {
                    let model = body
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if matches!(model, "gpt-5.2" | "codex-auto-review") {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({
                                "error": {"message": "model unavailable"}
                            })),
                        )
                            .into_response()
                    } else {
                        Json(serde_json::json!({"ok": true})).into_response()
                    }
                }),
            )
            .route(
                "/chat/completions",
                post(move || {
                    hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    async { StatusCode::OK.into_response() }
                }),
            );
        let base_url = serve(router).await;
        let execution = execute(&probe_client().unwrap(), &base_url, "sk-test").await;

        assert_eq!(execution.snapshot.wire_api.as_deref(), Some("responses"));
        assert_eq!(execution.snapshot.model.as_deref(), Some("gpt-5.6-terra"));
        assert_eq!(execution.snapshot.models.len(), 3);
        assert_eq!(chat_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(execution.calls.len(), 2);
        assert_eq!(execution.calls[1].model.as_deref(), Some("gpt-5.6-terra"));
        assert_eq!(execution.calls[1].route, "/responses");
    }

    #[test]
    fn text_probe_models_prioritizes_mainstream_text_ids_without_hiding_unknowns() {
        let models = vec![
            ProviderModelOption {
                upstream_model_id: "codex-auto-review".to_string(),
                display_name: "Codex Auto Review".to_string(),
                owned_by: Some("openai".to_string()),
            },
            ProviderModelOption {
                upstream_model_id: "gpt-5.2".to_string(),
                display_name: "GPT-5.2".to_string(),
                owned_by: Some("openai".to_string()),
            },
            ProviderModelOption {
                upstream_model_id: "gpt-5.6-terra".to_string(),
                display_name: "GPT-5.6 Terra".to_string(),
                owned_by: Some("openai".to_string()),
            },
            ProviderModelOption {
                upstream_model_id: "relay-private-chat".to_string(),
                display_name: "Relay private chat".to_string(),
                owned_by: None,
            },
        ];
        assert_eq!(
            text_probe_models(&models),
            vec![
                "gpt-5.6-terra".to_string(),
                "relay-private-chat".to_string(),
                "gpt-5.2".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn missing_responses_route_tries_chat_once() {
        let router = Router::new()
            .route("/models", get(|| async { models() }))
            .route(
                "/responses",
                post(|| async { StatusCode::NOT_FOUND.into_response() }),
            )
            .route(
                "/chat/completions",
                post(|| async { Json(serde_json::json!({"ok": true})) }),
            );
        let base_url = serve(router).await;
        let execution = execute(&probe_client().unwrap(), &base_url, "sk-test").await;
        assert_eq!(
            execution.snapshot.wire_api.as_deref(),
            Some("chat_completions")
        );
        assert_eq!(
            execution.snapshot.confirmed_base_url.as_deref(),
            Some(base_url.as_str())
        );
        assert_eq!(execution.calls.len(), 4);
        assert_eq!(
            execution
                .calls
                .iter()
                .map(|call| call.route)
                .collect::<Vec<_>>(),
            vec![
                "/models",
                "/responses",
                "/v1/responses",
                "/chat/completions"
            ]
        );
    }

    #[tokio::test]
    async fn unauthorized_responses_continues_to_a_later_target() {
        let router = Router::new()
            .route("/models", get(|| async { models() }))
            .route(
                "/responses",
                post(|| async { StatusCode::UNAUTHORIZED.into_response() }),
            )
            .route(
                "/chat/completions",
                post(|| async { Json(serde_json::json!({"ok": true})) }),
            );
        let base_url = serve(router).await;
        let execution = execute(&probe_client().unwrap(), &base_url, "sk-test").await;
        assert_eq!(
            execution.snapshot.wire_api.as_deref(),
            Some("chat_completions")
        );
        assert_eq!(
            execution.calls.last().map(|call| call.route),
            Some("/chat/completions")
        );
    }

    #[tokio::test]
    async fn payment_required_tries_v1_responses_before_chat() {
        let chat_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits = chat_hits.clone();
        let router = Router::new()
            .route("/models", get(|| async { models() }))
            .route(
                "/responses",
                post(|| async { StatusCode::PAYMENT_REQUIRED.into_response() }),
            )
            .route(
                "/v1/responses",
                post(|| async { Json(serde_json::json!({"ok": true})) }),
            )
            .route(
                "/chat/completions",
                post(move || {
                    hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    async { StatusCode::OK.into_response() }
                }),
            );
        let base_url = serve(router).await;
        let execution = execute(&probe_client().unwrap(), &base_url, "sk-test").await;
        assert_eq!(execution.snapshot.wire_api.as_deref(), Some("responses"));
        assert_eq!(
            execution.snapshot.confirmed_base_url.as_deref(),
            Some(format!("{base_url}/v1").as_str())
        );
        assert_eq!(chat_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            execution.calls.last().map(|call| call.route),
            Some("/v1/responses")
        );
    }

    #[tokio::test]
    async fn only_v1_chat_confirms_chat_prefix() {
        let router = Router::new()
            .route("/models", get(|| async { models() }))
            .route(
                "/v1/chat/completions",
                post(|| async { Json(serde_json::json!({"ok": true})) }),
            );
        let base_url = serve(router).await;
        let execution = execute(&probe_client().unwrap(), &base_url, "sk-test").await;
        assert_eq!(
            execution.snapshot.wire_api.as_deref(),
            Some("chat_completions")
        );
        assert_eq!(
            execution.snapshot.confirmed_base_url.as_deref(),
            Some(format!("{base_url}/v1").as_str())
        );
        assert_eq!(execution.calls.len(), 5);
    }

    #[tokio::test]
    async fn all_text_targets_fail_leave_protocol_unconfirmed() {
        let router = Router::new().route("/models", get(|| async { models() }));
        let base_url = serve(router).await;
        let execution = execute(&probe_client().unwrap(), &base_url, "sk-test").await;
        assert!(execution.snapshot.wire_api.is_none());
        assert!(execution.snapshot.confirmed_base_url.is_none());
        assert_eq!(execution.calls.len(), 5);
    }

    #[tokio::test]
    async fn prefix_already_v1_does_not_append_v1_again() {
        let router = Router::new()
            .route("/v1/models", get(|| async { models() }))
            .route(
                "/v1/responses",
                post(|| async { Json(serde_json::json!({"ok": true})) }),
            );
        let root = serve(router).await;
        let base_url = format!("{root}/v1");
        let execution = execute(&probe_client().unwrap(), &base_url, "sk-test").await;
        assert_eq!(execution.snapshot.wire_api.as_deref(), Some("responses"));
        assert_eq!(
            execution.snapshot.confirmed_base_url.as_deref(),
            Some(base_url.as_str())
        );
        assert_eq!(
            execution
                .calls
                .iter()
                .map(|call| call.route)
                .collect::<Vec<_>>(),
            vec!["/models", "/responses"]
        );
    }

    #[test]
    fn mismatched_ticket_is_not_reusable() {
        let store = ProbeTicketStore::default();
        let execution = finish(
            ConnectivityStatus::Ready,
            Some("responses".to_string()),
            Some("https://example.com/v1".to_string()),
            None,
            Vec::new(),
            CheckStatus::Ready,
            CheckStatus::Ready,
            None,
            Vec::new(),
        );
        let id = store.insert("https://example.com/v1", "sk-one", execution);
        assert!(matches!(
            store.take_matching(&id, "https://example.com/v1", "sk-two"),
            TicketMatch::Mismatched
        ));
        assert!(matches!(
            store.take_matching(&id, "https://example.com/v1", "sk-one"),
            TicketMatch::Missing
        ));
    }
}
