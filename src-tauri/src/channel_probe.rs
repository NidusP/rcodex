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
    secrets::validate_api_key,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const TICKET_TTL_SECS: i64 = 10 * 60;
const TICKET_LIMIT: usize = 8;

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

pub fn desired_transport(current: TransportMode, wire_api: Option<&str>) -> Option<TransportMode> {
    let desired = transport_for_wire(wire_api?)?;
    if current == desired {
        return None;
    }
    Some(desired)
}

pub async fn execute(client: &Client, base_url: &str, api_key: &str) -> ProbeExecution {
    let started = Instant::now();
    let models_url = format!("{}/models", base_url.trim_end_matches('/'));
    match client.get(&models_url).bearer_auth(api_key).send().await {
        Ok(response) if response.status().is_success() => {
            let status = response.status().as_u16();
            match response.bytes().await {
                Ok(body) => match serde_json::from_slice::<Value>(&body) {
                    Ok(value) => {
                        let models = parse_models(&value);
                        let mut calls = vec![call(
                            "/v1/models",
                            None,
                            true,
                            Some(status),
                            None,
                            started.elapsed(),
                            TransportMode::GatewayPassthrough,
                        )];
                        if models.is_empty() {
                            return finish(
                                ConnectivityStatus::Degraded,
                                None,
                                None,
                                models,
                                CheckStatus::Ready,
                                CheckStatus::NotRun,
                                Some("models_empty".to_string()),
                                calls,
                            );
                        }
                        let model = models[0].upstream_model_id.clone();
                        classify_text(client, base_url, api_key, model, models, &mut calls).await
                    }
                    Err(_) => failed_models(
                        started.elapsed(),
                        Some(status),
                        ConnectivityStatus::Degraded,
                        "invalid_models_response",
                    ),
                },
                Err(_) => failed_models(
                    started.elapsed(),
                    Some(status),
                    ConnectivityStatus::Degraded,
                    "models_body_unavailable",
                ),
            }
        }
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.bytes().await.unwrap_or_default();
            let code = upstream_error_code(status, &body);
            failed_models(
                started.elapsed(),
                Some(status),
                connectivity_from_error(status, &body),
                code,
            )
        }
        Err(error) => failed_models(
            started.elapsed(),
            None,
            connectivity_from_transport(&error),
            transport_error_code(&error),
        ),
    }
}

pub fn validate_draft(base_url: String, api_key: String) -> Result<(String, String), String> {
    let base_url = crate::providers::validate_base_url(base_url)?;
    let api_key = validate_api_key(api_key)?;
    Ok((base_url, api_key.expose().to_string()))
}

async fn classify_text(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: String,
    models: Vec<ProviderModelOption>,
    calls: &mut Vec<ProbeCall>,
) -> ProbeExecution {
    match post_text(
        client,
        base_url,
        api_key,
        &model,
        "/v1/responses",
        responses_body(&model),
        TransportMode::GatewayPassthrough,
    )
    .await
    {
        TextAttempt::Ready(call) => {
            calls.push(call);
            finish(
                ConnectivityStatus::Ready,
                Some("responses".to_string()),
                Some(model),
                models,
                CheckStatus::Ready,
                CheckStatus::Ready,
                None,
                calls.clone(),
            )
        }
        TextAttempt::RouteMissing(call) => {
            calls.push(call);
            match post_text(
                client,
                base_url,
                api_key,
                &model,
                "/v1/chat/completions",
                chat_body(&model),
                TransportMode::GatewayChatCompletions,
            )
            .await
            {
                TextAttempt::Ready(call) => {
                    calls.push(call);
                    finish(
                        ConnectivityStatus::Ready,
                        Some("chat_completions".to_string()),
                        Some(model),
                        models,
                        CheckStatus::Ready,
                        CheckStatus::Ready,
                        None,
                        calls.clone(),
                    )
                }
                TextAttempt::RouteMissing(call) | TextAttempt::Stop(call) => {
                    let error_code = call.error_code.clone();
                    let status = call_status(&call);
                    calls.push(call);
                    finish(
                        status,
                        None,
                        Some(model),
                        models,
                        CheckStatus::Ready,
                        CheckStatus::Failed,
                        error_code,
                        calls.clone(),
                    )
                }
            }
        }
        TextAttempt::Stop(call) => {
            let error_code = call.error_code.clone();
            let status = call_status(&call);
            calls.push(call);
            finish(
                status,
                None,
                Some(model),
                models,
                CheckStatus::Ready,
                CheckStatus::Failed,
                error_code,
                calls.clone(),
            )
        }
    }
}

enum TextAttempt {
    Ready(ProbeCall),
    RouteMissing(ProbeCall),
    Stop(ProbeCall),
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
    let url = format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        route.trim_start_matches("/v1")
    );
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
            let call = call(
                route,
                Some(model.to_string()),
                valid,
                Some(status),
                (!valid).then(|| "invalid_text_response".to_string()),
                started.elapsed(),
                transport_mode,
            );
            if valid {
                TextAttempt::Ready(call)
            } else {
                TextAttempt::Stop(call)
            }
        }
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.bytes().await.unwrap_or_default();
            let code = upstream_error_code(status, &body).to_string();
            let call = call(
                route,
                Some(model.to_string()),
                false,
                Some(status),
                Some(code),
                started.elapsed(),
                transport_mode,
            );
            if matches!(status, 401 | 403)
                || body_indicates_auth_failure(&body)
                || matches!(status, 429)
                || (500..600).contains(&status)
            {
                TextAttempt::Stop(call)
            } else if route_is_missing(status, &body) {
                TextAttempt::RouteMissing(call)
            } else {
                TextAttempt::Stop(call)
            }
        }
        Err(error) => TextAttempt::Stop(call(
            route,
            Some(model.to_string()),
            false,
            None,
            Some(transport_error_code(&error).to_string()),
            started.elapsed(),
            transport_mode,
        )),
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

fn failed_models(
    elapsed: Duration,
    http_status: Option<u16>,
    status: ConnectivityStatus,
    error_code: &str,
) -> ProbeExecution {
    finish(
        status,
        None,
        None,
        Vec::new(),
        CheckStatus::Failed,
        CheckStatus::NotRun,
        Some(error_code.to_string()),
        vec![call(
            "/v1/models",
            None,
            false,
            http_status,
            Some(error_code.to_string()),
            elapsed,
            TransportMode::GatewayPassthrough,
        )],
    )
}

fn finish(
    status: ConnectivityStatus,
    wire_api: Option<String>,
    model: Option<String>,
    models: Vec<ProviderModelOption>,
    models_check: CheckStatus,
    text_check: CheckStatus,
    error_code: Option<String>,
    calls: Vec<ProbeCall>,
) -> ProbeExecution {
    let wire_api = if matches!(status, ConnectivityStatus::Ready)
        && matches!(models_check, CheckStatus::Ready)
        && matches!(text_check, CheckStatus::Ready)
    {
        wire_api.filter(|value| matches!(value.as_str(), "responses" | "chat_completions"))
    } else {
        None
    };
    ProbeExecution {
        snapshot: ProbeSnapshot {
            status,
            wire_api,
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

fn connectivity_from_error(status: u16, body: &[u8]) -> ConnectivityStatus {
    if matches!(status, 401 | 403) || body_indicates_auth_failure(body) {
        ConnectivityStatus::Unauthorized
    } else {
        ConnectivityStatus::Degraded
    }
}

fn connectivity_from_transport(error: &reqwest::Error) -> ConnectivityStatus {
    if error.is_connect() || error.is_timeout() {
        ConnectivityStatus::Unreachable
    } else {
        ConnectivityStatus::Degraded
    }
}

fn upstream_error_code(status: u16, body: &[u8]) -> &'static str {
    if matches!(status, 401 | 403) || body_indicates_auth_failure(body) {
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
        assert!(matches!(
            execution.snapshot.status,
            ConnectivityStatus::Ready
        ));
        assert_eq!(execution.snapshot.models.len(), 1);
        assert_eq!(chat_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(execution.calls.len(), 2);
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
        assert_eq!(execution.calls.len(), 3);
    }

    #[tokio::test]
    async fn unauthorized_text_does_not_try_chat() {
        let chat_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits = chat_hits.clone();
        let router = Router::new()
            .route("/models", get(|| async { models() }))
            .route(
                "/responses",
                post(|| async { StatusCode::UNAUTHORIZED.into_response() }),
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
        assert!(execution.snapshot.wire_api.is_none());
        assert!(matches!(
            execution.snapshot.status,
            ConnectivityStatus::Unauthorized
        ));
        assert_eq!(chat_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn upstream_failure_does_not_try_chat() {
        let chat_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits = chat_hits.clone();
        let router = Router::new()
            .route("/models", get(|| async { models() }))
            .route(
                "/responses",
                post(|| async { StatusCode::BAD_GATEWAY.into_response() }),
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
        assert!(execution.snapshot.wire_api.is_none());
        assert_eq!(chat_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(execution.calls.len(), 2);
    }

    #[test]
    fn mismatched_ticket_is_not_reusable() {
        let store = ProbeTicketStore::default();
        let execution = finish(
            ConnectivityStatus::Ready,
            Some("responses".to_string()),
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
