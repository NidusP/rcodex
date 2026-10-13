//! 数据面 HTTP：health、models、responses 入口和 in-flight 计数。
//!
//! 不直接打上游；准入后把请求交给 protocol/forward。只绑定调用方传入的回环 listener。
//! `run_sidecar` 挂上这个 router。

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::conversation_archive::RequestCapture;
use crate::managed_codex_config::{INSTANCE_GATEWAY_HEADER, LEGACY_INSTANCE_GATEWAY_HEADER};
use crate::routing::{
    is_ephemeral_family_alias, is_internal_codex_model_id, CompactMode, CompactionSupport,
    RouteSource,
};
use crate::secrets::instance_gateway_key_matches;

use super::compaction;
use super::diagnostics::RequestRecorder;
use super::errors::{api_error, not_found, reject_local};
use super::forward::forward_provider;
use super::helpers::{ephemeral_family_alias_route, gateway_trace};
use super::responses_compat::prepare_responses_relay;
use super::runtime_types::{BindingRuntime, ManagedGatewayAppState};
use super::{LOOPBACK_HOST, MAX_REQUEST_BYTES};

pub(super) fn managed_gateway_router(state: ManagedGatewayAppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/v1/responses", post(responses))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state)
}

pub(super) async fn health(State(state): State<ManagedGatewayAppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "bindHost": LOOPBACK_HOST,
        "activeBindings": state.registry.read().await.len()
    }))
}

pub(super) async fn admit(
    state: &ManagedGatewayAppState,
    headers: &HeaderMap,
) -> Result<Arc<BindingRuntime>, Response> {
    let current = headers
        .get(INSTANCE_GATEWAY_HEADER)
        .and_then(|value| value.to_str().ok());
    let legacy = headers
        .get(LEGACY_INSTANCE_GATEWAY_HEADER)
        .and_then(|value| value.to_str().ok());
    let presented = match (current, legacy) {
        (Some(current), Some(legacy)) if current != legacy => {
            return Err(api_error(StatusCode::UNAUTHORIZED, "invalid_instance_key"));
        }
        (Some(current), _) => current,
        (None, Some(legacy)) => legacy,
        (None, None) => return Err(api_error(StatusCode::UNAUTHORIZED, "invalid_instance_key")),
    };
    let registry = state.registry.read().await;
    registry
        .values()
        .find(|binding| instance_gateway_key_matches(presented, &binding.admission_key_hash))
        .cloned()
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "invalid_instance_key"))
}

pub(super) async fn models(
    State(state): State<ManagedGatewayAppState>,
    headers: HeaderMap,
) -> Response {
    let binding = match admit(&state, &headers).await {
        Ok(binding) => binding,
        Err(response) => return response,
    };
    let models = binding
        .route_set
        .routes
        .iter()
        .filter(|route| {
            !is_internal_codex_model_id(&route.public_model_id)
                && !is_internal_codex_model_id(&route.upstream_model_id)
                && !is_ephemeral_family_alias(route)
        })
        .map(|route| {
            json!({
                "id": route.public_model_id,
                "object": "model",
                "owned_by": "rcodex"
            })
        })
        .collect::<Vec<_>>();
    Json(json!({ "object": "list", "data": models })).into_response()
}

pub(super) async fn responses(
    State(state): State<ManagedGatewayAppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut downstream_capture = None;
    let mut in_flight_guard = None;
    let mut response = async {
        let binding = match admit(&state, &headers).await {
            Ok(binding) => binding,
            Err(response) => {
                gateway_trace(&[("event", "admit_failed"), ("code", "invalid_instance_key")]);
                return response;
            }
        };
        let archive = state.archive.read().await.clone();
        let recorder = Some(RequestRecorder {
            diagnostics: state.diagnostics.read().await.clone(),
            usage: state.usage.read().await.clone(),
            archive: archive.and_then(|archive| {
                if !archive.is_enabled() {
                    return None;
                }
                archive.begin(
                    binding.route_set.launch_preset_id.clone(),
                    &headers,
                    &body,
                    binding.archive_home.clone(),
                    headers
                        .get(AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned)
                        .into_iter()
                        .collect(),
                )
            }),
        });
        downstream_capture = recorder.as_ref().and_then(|r| r.archive.clone());
        let mut request = match serde_json::from_slice::<Value>(&body) {
            Ok(Value::Object(request)) => request,
            _ => {
                return reject_local(
                    &recorder,
                    &binding.binding_id,
                    "",
                    "invalid_request",
                    StatusCode::BAD_REQUEST,
                )
            }
        };
        let public_model = match request.get("model").and_then(Value::as_str) {
            Some(model) => model.to_string(),
            None => {
                return reject_local(
                    &recorder,
                    &binding.binding_id,
                    "",
                    "model_required",
                    StatusCode::BAD_REQUEST,
                )
            }
        };
        let route = binding
            .route_set
            .routes
            .iter()
            .find(|route| route.public_model_id == public_model)
            .cloned()
            .or_else(|| ephemeral_family_alias_route(&binding.route_set, &public_model));
        let Some(route) = route else {
            return reject_local(
                &recorder,
                &binding.binding_id,
                &public_model,
                "model_not_in_route_set",
                StatusCode::NOT_FOUND,
            );
        };
        let compact_mode =
            CompactMode::from_gateway_snapshot(&binding.route_set.gateway_service.config_snapshot);
        if let Err(code) = prepare_responses_relay(&route, compact_mode, &mut request) {
            return reject_local(
                &recorder,
                &binding.binding_id,
                &public_model,
                code,
                StatusCode::BAD_REQUEST,
            );
        }
        let summarize =
            compaction::should_summarize(compact_mode, &route, &Value::Object(request.clone()));
        request.insert(
            "model".to_string(),
            Value::String(route.upstream_model_id.clone()),
        );

        match &route.source {
            RouteSource::Official { .. } => {
                if headers.get(AUTHORIZATION).is_none() {
                    return reject_local(
                        &recorder,
                        &binding.binding_id,
                        &public_model,
                        "official_authorization_missing",
                        StatusCode::UNAUTHORIZED,
                    );
                }
                reject_local(
                    &recorder,
                    &binding.binding_id,
                    &public_model,
                    "official_route_unavailable",
                    StatusCode::SERVICE_UNAVAILABLE,
                )
            }
            RouteSource::ApiProvider {
                provider_id,
                provider_revision_id,
            } => {
                let Some(upstream) = binding.upstreams.get(provider_revision_id) else {
                    return reject_local(
                        &recorder,
                        &binding.binding_id,
                        &public_model,
                        "provider_revision_unavailable",
                        StatusCode::SERVICE_UNAVAILABLE,
                    );
                };
                if upstream.provider_id != *provider_id
                    || upstream.provider_revision_id != *provider_revision_id
                {
                    return reject_local(
                        &recorder,
                        &binding.binding_id,
                        &public_model,
                        "provider_revision_mismatch",
                        StatusCode::SERVICE_UNAVAILABLE,
                    );
                }
                if summarize {
                    in_flight_guard =
                        Some(InFlightRequestGuard::new(state.in_flight_requests.clone()));
                    return compaction::run_summarizer(
                        upstream,
                        &route,
                        request,
                        Some(binding.binding_id.clone()),
                        recorder,
                    )
                    .await;
                }
                let request =
                    if route.capabilities.compaction_support == CompactionSupport::NativeRemote {
                        request
                    } else {
                        match compaction::expand_compaction_history(&request) {
                            Ok(prepared) => prepared,
                            Err(code) => {
                                return reject_local(
                                    &recorder,
                                    &binding.binding_id,
                                    &public_model,
                                    code,
                                    StatusCode::BAD_REQUEST,
                                )
                            }
                        }
                    };
                in_flight_guard = Some(InFlightRequestGuard::new(state.in_flight_requests.clone()));
                forward_provider(
                    upstream,
                    &route,
                    request,
                    Some(binding.binding_id.clone()),
                    recorder,
                    None,
                )
                .await
            }
        }
    }
    .await;
    if let Some(guard) = in_flight_guard.take() {
        response = attach_in_flight_guard(response, guard);
    }
    // The response body may outlive this handler by minutes. Keeping the
    // capture in the body wrapper records what the local HTTP layer actually
    // yielded, including a caller closing a stream early.
    match downstream_capture {
        Some(capture) => capture_downstream_response(response, capture),
        None => response,
    }
}

pub(super) struct InFlightRequestGuard {
    counter: Arc<AtomicUsize>,
}

impl InFlightRequestGuard {
    pub(super) fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::AcqRel);
        Self { counter }
    }
}

impl Drop for InFlightRequestGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) fn attach_in_flight_guard(response: Response, guard: InFlightRequestGuard) -> Response {
    let (parts, body) = response.into_parts();
    let stream = futures_util::stream::unfold(
        (body.into_data_stream(), guard),
        |(mut body, guard)| async move {
            match body.next().await {
                Some(chunk) => Some((chunk, (body, guard))),
                None => None,
            }
        },
    );
    Response::from_parts(parts, Body::from_stream(stream))
}

pub(super) struct DownstreamDelivery {
    capture: RequestCapture,
    exhausted: bool,
}

impl Drop for DownstreamDelivery {
    fn drop(&mut self) {
        if !self.exhausted {
            self.capture
                .metadata(json!({"downstreamDelivery": "cancelled"}));
        }
    }
}

pub(super) fn capture_downstream_response(response: Response, capture: RequestCapture) -> Response {
    let (parts, body) = response.into_parts();
    let state = (
        body.into_data_stream(),
        DownstreamDelivery {
            capture,
            exhausted: false,
        },
    );
    let stream = futures_util::stream::unfold(state, |(mut body, mut delivery)| async move {
        match body.next().await {
            Some(Ok(chunk)) => {
                delivery.capture.stage("downstream-response", &chunk);
                Some((Ok::<Bytes, std::io::Error>(chunk), (body, delivery)))
            }
            Some(Err(error)) => {
                delivery
                    .capture
                    .metadata(json!({"downstreamDelivery": "error"}));
                Some((
                    Err(std::io::Error::other(error.to_string())),
                    (body, delivery),
                ))
            }
            None => {
                delivery.exhausted = true;
                delivery
                    .capture
                    .metadata(json!({"downstreamDelivery": "body_exhausted"}));
                None
            }
        }
    });
    Response::from_parts(parts, Body::from_stream(stream))
}
