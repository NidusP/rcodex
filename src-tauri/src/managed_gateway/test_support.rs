//! managed_gateway 测试夹具：假上游、RouteSet 和 Codex 调用包装。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex as StdMutex,
};

use axum::{http::HeaderValue, routing::post};
use tower::ServiceExt;

use super::*;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

use super::errors::*;
use super::forward::*;
use super::helpers::*;
use super::http::*;
use super::lifecycle::*;
use super::protocol::*;
use super::runtime::*;
use super::runtime_types;
use super::runtime_types::*;
use super::sse::SseDecoder;
use super::stream::*;
use crate::managed_codex_config::{INSTANCE_GATEWAY_HEADER, LEGACY_INSTANCE_GATEWAY_HEADER};
use crate::routing::{
    CompactMode, CompactionSupport, GatewayTimeoutSnapshot, ModelRoute, RouteSource, WireApi,
};
use crate::{
    database::Database,
    managed_codex_config::{
        build_version_matched_catalog, materialize_managed_codex_config, read_bundled_catalog,
        INSTANCE_GATEWAY_ENV,
    },
    providers::{CreateProviderInput, TransportMode},
    routing::{
        CapabilitySet, GatewayServiceRevision, RouteSetProviderRevision, RouteSetRevision,
        RouteSetState,
    },
    secrets::{instance_gateway_key_hash, ApiKey, ApiKeyVault, InstanceGatewayKey},
};
use std::collections::BTreeMap;
use std::sync::{atomic::AtomicUsize, Arc};
use tokio::sync::RwLock;
use url::Url;

#[derive(Default)]
pub(super) struct MemoryVault(StdMutex<HashMap<String, String>>);

impl ApiKeyVault for MemoryVault {
    fn load(&self, secret_ref: &str) -> Result<Option<ApiKey>, String> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(secret_ref)
            .map(|value| ApiKey::for_test(value)))
    }

    fn save(&self, secret_ref: &str, api_key: &ApiKey) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .insert(secret_ref.to_string(), api_key.expose().to_string());
        Ok(())
    }

    fn clear(&self, secret_ref: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(secret_ref);
        Ok(())
    }
}

pub(super) fn test_timeouts() -> GatewayTimeoutSnapshot {
    GatewayTimeoutSnapshot {
        connect_seconds: 5,
        first_byte_seconds: 5,
        stream_idle_seconds: 5,
        non_stream_total_seconds: 5,
    }
}

pub(super) fn compact_trigger_body(model: &str) -> String {
    json!({
        "model": model,
        "tools": [{"type": "function", "name": "shell"}],
        "input": [
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "continue"}]},
            {"type": "compaction_trigger"}
        ]
    })
    .to_string()
}

pub(super) async fn bind_channel_state(
    binding_id: &str,
    key: &InstanceGatewayKey,
    mut routes: RouteSetRevision,
    upstream: ProviderUpstreamRuntime,
    compact_mode: CompactMode,
    compaction_support: CompactionSupport,
) -> ManagedGatewayAppState {
    routes.gateway_service.config_snapshot =
        json!({"compactMode": compact_mode.as_snapshot_str()}).to_string();
    routes.routes[0].capabilities.compaction_support = compaction_support;
    let revision_id = match &routes.routes[0].source {
        RouteSource::ApiProvider {
            provider_revision_id,
            ..
        } => provider_revision_id.clone(),
        RouteSource::Official { .. } => "official".to_string(),
    };
    let state = ManagedGatewayAppState {
        registry: Arc::new(RwLock::new(BTreeMap::new())),
        diagnostics: Arc::new(RwLock::new(None)),
        usage: Arc::new(RwLock::new(None)),
        archive: Arc::new(RwLock::new(None)),
        in_flight_requests: Arc::new(AtomicUsize::new(0)),
    };
    state.registry.write().await.insert(
        binding_id.to_string(),
        Arc::new(BindingRuntime {
            binding_id: binding_id.to_string(),
            archive_home: None,
            admission_key_hash: instance_gateway_key_hash(key),
            route_set: routes,
            upstreams: [(revision_id, upstream)].into_iter().collect(),
        }),
    );
    state
}

pub(super) fn compaction_output_item(body: &Value) -> Option<&Value> {
    body.get("output")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("type") == Some(&json!("compaction")))
        })
}

pub(super) fn route_set(provider_id: String, revision_id: String) -> RouteSetRevision {
    RouteSetRevision {
        id: "route-set-a".to_string(),
        launch_preset_id: "preset-a".to_string(),
        launch_preset_version: 1,
        official_account_id: None,
        official_credential_revision_id: None,
        gateway_service: GatewayServiceRevision {
            id: "gateway-a".to_string(),
            protocol_version: 1,
            config_snapshot: "{}".to_string(),
            config_sha256: "sha256:gateway".to_string(),
            created_at: 1,
        },
        requires_openai_auth: false,
        catalog_path: "/managed/catalog.json".to_string(),
        catalog_sha256: "sha256:catalog".to_string(),
        state: RouteSetState::Ready,
        created_at: 1,
        providers: vec![RouteSetProviderRevision {
            provider_id: provider_id.clone(),
            provider_revision_id: revision_id.clone(),
            position: 0,
            timeouts: GatewayTimeoutSnapshot {
                connect_seconds: 5,
                first_byte_seconds: 5,
                stream_idle_seconds: 5,
                non_stream_total_seconds: 5,
            },
        }],
        routes: vec![ModelRoute {
            id: "route-a".to_string(),
            route_identity_id: "identity-a".to_string(),
            public_model_id: "deepseek-managed".to_string(),
            display_name: "DeepSeek Managed".to_string(),
            source: RouteSource::ApiProvider {
                provider_id,
                provider_revision_id: revision_id,
            },
            upstream_model_id: "deepseek-chat".to_string(),
            wire_api: WireApi::Responses,
            adapter_kind: "responses_passthrough".to_string(),
            adapter_version: "1".to_string(),
            capabilities: CapabilitySet {
                supports_tools: true,
                ..CapabilitySet::default()
            },
            position: 0,
        }],
    }
}

pub(super) fn jsonl_thread_id(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let value = serde_json::from_str::<Value>(line).ok()?;
        value
            .get("thread_id")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

pub(super) fn tools_len(request: &Value) -> usize {
    request
        .get("tools")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

pub(super) fn run_isolated_codex(
    executable: &Path,
    home: &Path,
    instance_key: &str,
    args: &[&str],
) -> std::process::Output {
    Command::new(executable)
        .args(args)
        .current_dir(home)
        .stdin(Stdio::null())
        .env("CODEX_HOME", home)
        .env("CODEX_SQLITE_HOME", home)
        .env(INSTANCE_GATEWAY_ENV, instance_key)
        .env("NO_PROXY", "127.0.0.1,localhost,::1")
        .env("no_proxy", "127.0.0.1,localhost,::1")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN")
        .output()
        .unwrap()
}

pub(super) fn responses_sse_completed(
    model: &str,
    text: &str,
    response_id: &str,
    item_id: &str,
    input_tokens: u64,
) -> Response {
    let output_item = json!({
        "id": item_id,
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text, "annotations": [], "logprobs": []}]
    });
    let events = [
        sse_event(
            "response.created",
            json!({
                "type": "response.created",
                "sequence_number": 0,
                "response": {
                    "id": response_id,
                    "object": "response",
                    "created_at": 1,
                    "status": "in_progress",
                    "error": null,
                    "incomplete_details": null,
                    "instructions": null,
                    "max_output_tokens": null,
                    "model": model,
                    "output": [],
                    "parallel_tool_calls": true,
                    "previous_response_id": null,
                    "reasoning": {"effort": null, "summary": null},
                    "store": false,
                    "temperature": 1.0,
                    "text": {"format": {"type": "text"}},
                    "tool_choice": "auto",
                    "tools": [],
                    "top_p": 1.0,
                    "truncation": "disabled",
                    "usage": null,
                    "metadata": {}
                }
            }),
        ),
        sse_event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "sequence_number": 1,
                "output_index": 0,
                "item": {"id": item_id, "type": "message", "status": "in_progress", "role": "assistant", "content": []}
            }),
        ),
        sse_event(
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "sequence_number": 2,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": [], "logprobs": []}
            }),
        ),
        sse_event(
            "response.output_text.delta",
            json!({
                "type": "response.output_text.delta",
                "sequence_number": 3,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "delta": text,
                "logprobs": []
            }),
        ),
        sse_event(
            "response.output_text.done",
            json!({
                "type": "response.output_text.done",
                "sequence_number": 4,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "text": text,
                "logprobs": []
            }),
        ),
        sse_event(
            "response.content_part.done",
            json!({
                "type": "response.content_part.done",
                "sequence_number": 5,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "part": {"type": "output_text", "text": text, "annotations": [], "logprobs": []}
            }),
        ),
        sse_event(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "sequence_number": 6,
                "output_index": 0,
                "item": output_item.clone()
            }),
        ),
        sse_event(
            "response.completed",
            json!({
                "type": "response.completed",
                "sequence_number": 7,
                "response": {
                    "id": response_id,
                    "object": "response",
                    "created_at": 1,
                    "status": "completed",
                    "error": null,
                    "incomplete_details": null,
                    "instructions": null,
                    "max_output_tokens": null,
                    "model": model,
                    "output": [output_item],
                    "parallel_tool_calls": true,
                    "previous_response_id": null,
                    "reasoning": {"effort": null, "summary": null},
                    "store": false,
                    "temperature": 1.0,
                    "text": {"format": {"type": "text"}},
                    "tool_choice": "auto",
                    "tools": [],
                    "top_p": 1.0,
                    "truncation": "disabled",
                    "usage": {
                        "input_tokens": input_tokens,
                        "input_tokens_details": {"cached_tokens": 0},
                        "output_tokens": 8,
                        "output_tokens_details": {"reasoning_tokens": 0},
                        "total_tokens": input_tokens + 8
                    },
                    "metadata": {}
                }
            }),
        ),
    ];
    let payload = events
        .into_iter()
        .flat_map(|event| event.to_vec())
        .collect::<Vec<_>>();
    let mut response = Response::new(Body::from(payload));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
    response
}

#[derive(Clone)]
pub(super) struct CompactProbeUpstream {
    pub(super) observed: Arc<StdMutex<Vec<(HeaderMap, Value)>>>,
    pub(super) next_id: Arc<StdMutex<u64>>,
}

pub(super) async fn mock_compact_probe_stream(
    State(state): State<CompactProbeUpstream>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    state.observed.lock().unwrap().push((headers, body.clone()));
    let id = {
        let mut next_id = state.next_id.lock().unwrap();
        *next_id += 1;
        *next_id
    };
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("compact-probe-upstream");
    responses_sse_completed(
        model,
        "COMPACT_PROBE_OK",
        &format!("resp_compact_{id}"),
        &format!("msg_compact_{id}"),
        8_000,
    )
}

#[derive(Clone)]
pub(super) struct IsolatedCompactCapture {
    pub(super) observed: Arc<StdMutex<Vec<(String, Value)>>>,
    pub(super) gateway_responses_url: String,
    pub(super) client: reqwest::Client,
}

pub(super) async fn capture_isolated_compact_request(
    State(state): State<IsolatedCompactCapture>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    state
        .observed
        .lock()
        .unwrap()
        .push(("/v1/responses".to_string(), body.clone()));
    let mut forward_headers = headers.clone();
    forward_headers.remove("host");
    forward_headers.remove("content-length");
    let forwarded = match state
        .client
        .post(&state.gateway_responses_url)
        .headers(forward_headers)
        .json(&body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return api_error(StatusCode::BAD_GATEWAY, "upstream_connect_failed"),
    };
    let status = forwarded.status();
    let content_type = forwarded.headers().get(CONTENT_TYPE).cloned();
    let bytes = forwarded.bytes().await.unwrap_or_default();
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    if let Some(content_type) = content_type {
        response.headers_mut().insert(CONTENT_TYPE, content_type);
    }
    response
}

pub(super) async fn capture_isolated_compact_v1_endpoint(
    State(state): State<IsolatedCompactCapture>,
    Json(body): Json<Value>,
) -> Response {
    state
        .observed
        .lock()
        .unwrap()
        .push(("/responses/compact".to_string(), body));
    api_error(StatusCode::NOT_FOUND, "not_found")
}

pub(super) async fn mock_upstream(
    State(observed): State<Arc<StdMutex<Option<(HeaderMap, Value)>>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    *observed.lock().unwrap() = Some((headers, body.clone()));
    Json(json!({
        "id": "response-a",
        "object": "response",
        "model": body["model"],
        "status": "completed",
        "output": [{
            "id": "message-a",
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "mock response"}]
        }],
        "usage": {"input_tokens": 12, "output_tokens": 7, "total_tokens": 19}
    }))
}

pub(super) async fn mock_plain_text_502() -> Response {
    (StatusCode::BAD_GATEWAY, "Unknown error").into_response()
}

pub(super) async fn mock_grok_responses_stream(
    State(observed): State<Arc<StdMutex<Option<(HeaderMap, Value)>>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    *observed.lock().unwrap() = Some((headers, body.clone()));
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("grok-upstream");
    let response_id = "resp_grok_contract";
    let item_id = "msg_grok_contract";
    let text = "GROK_CONTRACT_OK";
    let events = [
        sse_event(
            "response.created",
            json!({
                "type": "response.created",
                "sequence_number": 0,
                "response": {
                    "id": response_id,
                    "object": "response",
                    "created_at": 1,
                    "status": "in_progress",
                    "error": null,
                    "incomplete_details": null,
                    "instructions": null,
                    "max_output_tokens": null,
                    "model": model,
                    "output": [],
                    "parallel_tool_calls": true,
                    "previous_response_id": null,
                    "reasoning": {"effort": null, "summary": null},
                    "store": false,
                    "temperature": 1.0,
                    "text": {"format": {"type": "text"}},
                    "tool_choice": "auto",
                    "tools": [],
                    "top_p": 1.0,
                    "truncation": "disabled",
                    "usage": null,
                    "metadata": {}
                }
            }),
        ),
        sse_event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "sequence_number": 1,
                "output_index": 0,
                "item": {"id": item_id, "type": "message", "status": "in_progress", "role": "assistant", "content": []}
            }),
        ),
        sse_event(
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "sequence_number": 2,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": [], "logprobs": []}
            }),
        ),
        sse_event(
            "response.output_text.delta",
            json!({
                "type": "response.output_text.delta",
                "sequence_number": 3,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "delta": text,
                "logprobs": []
            }),
        ),
        sse_event(
            "response.output_text.done",
            json!({
                "type": "response.output_text.done",
                "sequence_number": 4,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "text": text,
                "logprobs": []
            }),
        ),
        sse_event(
            "response.content_part.done",
            json!({
                "type": "response.content_part.done",
                "sequence_number": 5,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "part": {"type": "output_text", "text": text, "annotations": [], "logprobs": []}
            }),
        ),
        sse_event(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "sequence_number": 6,
                "output_index": 0,
                "item": {"id": item_id, "type": "message", "status": "completed", "role": "assistant", "content": [{"type": "output_text", "text": text, "annotations": [], "logprobs": []}]}
            }),
        ),
        sse_event(
            "response.completed",
            json!({
                "type": "response.completed",
                "sequence_number": 7,
                "response": {
                    "id": response_id,
                    "object": "response",
                    "created_at": 1,
                    "status": "completed",
                    "error": null,
                    "incomplete_details": null,
                    "instructions": null,
                    "max_output_tokens": null,
                    "model": model,
                    "output": [{"id": item_id, "type": "message", "status": "completed", "role": "assistant", "content": [{"type": "output_text", "text": text, "annotations": [], "logprobs": []}]}],
                    "parallel_tool_calls": true,
                    "previous_response_id": null,
                    "reasoning": {"effort": null, "summary": null},
                    "store": false,
                    "temperature": 1.0,
                    "text": {"format": {"type": "text"}},
                    "tool_choice": "auto",
                    "tools": [],
                    "top_p": 1.0,
                    "truncation": "disabled",
                    "usage": {"input_tokens": 1, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 1, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 2},
                    "metadata": {}
                }
            }),
        ),
    ];
    let payload = events
        .into_iter()
        .flat_map(|event| event.to_vec())
        .collect::<Vec<_>>();
    let mut response = Response::new(Body::from(payload));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
    response
}

pub(super) async fn mock_grok_chat_stream(
    State(observed): State<Arc<StdMutex<Option<(HeaderMap, Value)>>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    *observed.lock().unwrap() = Some((headers, body.clone()));
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("grok-upstream");
    let events = [
        format!(
            "data: {}\n\n",
            json!({
                "id": "chatcmpl_grok_contract",
                "object": "chat.completion.chunk",
                "created": 1,
                "model": model,
                "choices": [{"index": 0, "delta": {"role": "assistant", "content": "GROK_CONTRACT_OK"}, "finish_reason": null}]
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "id": "chatcmpl_grok_contract",
                "object": "chat.completion.chunk",
                "created": 1,
                "model": model,
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
            })
        ),
        "data: [DONE]\n\n".to_string(),
    ];
    let mut response = Response::new(Body::from(events.concat()));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
    response
}
