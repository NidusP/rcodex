//! GatewaySummarizer 与 compaction trigger 的数据面测试。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex as StdMutex,
};

use axum::{http::HeaderValue, routing::post};
use tower::ServiceExt;

use super::test_support::*;
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
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use url::Url;

#[tokio::test]
async fn compaction_request_keeps_its_existing_fail_closed_path() {
    let key = InstanceGatewayKey::for_test("agent-message-compact-key");
    let mut upstream =
        fixture_upstream("http://127.0.0.1:9", WireApi::Responses, test_timeouts()).unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let state = bind_channel_state(
        "agent-message-compact-instance",
        &key,
        route_set("provider-a".to_string(), "revision-a".to_string()),
        upstream,
        CompactMode::LocalClient,
        CompactionSupport::Unsupported,
    )
    .await;

    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(
                    json!({
                        "model": "deepseek-managed",
                        "input": [
                            {
                                "type": "agent_message",
                                "content": [{
                                    "type": "encrypted_content",
                                    "encrypted_content": "3r-compaction-v1:opaque"
                                }]
                            },
                            {"type": "compaction_trigger"}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body = std::str::from_utf8(&body).unwrap();
    assert!(body.contains("unsupported_compaction"));
    assert!(!body.contains("unreadable_agent_message"));
}

#[test]
fn third_party_compaction_trigger_is_rejected_before_forwarding() {
    let route = route_set("provider-a".to_string(), "revision-a".to_string())
        .routes
        .remove(0);
    let request = json!({
        "model": "deepseek-managed",
        "input": [
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "continue"}]},
            {"type": "compaction_trigger"}
        ]
    });
    assert_eq!(
        preflight_capabilities(&route, CompactMode::LocalClient, &request),
        Err("unsupported_compaction")
    );
    let mut summarizer = route.clone();
    summarizer.capabilities.compaction_support = CompactionSupport::GatewaySummarizer;
    assert_eq!(
        preflight_capabilities(&summarizer, CompactMode::RemoteV2, &request),
        Ok(())
    );
    let mut fake_gpt = route.clone();
    fake_gpt.public_model_id = "gpt-4o".to_string();
    fake_gpt.upstream_model_id = "gpt-4o".to_string();
    fake_gpt.capabilities.compaction_support = CompactionSupport::Unsupported;
    assert_eq!(
        preflight_capabilities(&fake_gpt, CompactMode::RemoteV2, &request),
        Err("not_native_gpt_compaction")
    );
    let mut native = route.clone();
    native.capabilities.compaction_support = CompactionSupport::NativeRemote;
    assert_eq!(
        native.capabilities.compaction_support,
        CompactionSupport::NativeRemote
    );
    assert_eq!(
        preflight_capabilities(&native, CompactMode::RemoteV2, &request),
        Ok(())
    );
}

#[test]
fn gateway_rejects_snapshots_that_reenable_default_provider_fallback() {
    let mut routes = route_set("provider".to_string(), "revision".to_string());
    assert!(validate_isolated_binding(&routes).is_ok());
    routes.gateway_service.config_snapshot =
        json!({"compatibilityFallback": {"baseUrl": "https://original.example"}}).to_string();
    assert_eq!(
        validate_isolated_binding(&routes).unwrap_err(),
        "compatibility_mode_removed"
    );
    routes.gateway_service.config_snapshot = "not-json".to_string();
    assert_eq!(
        validate_isolated_binding(&routes).unwrap_err(),
        "gateway_service_snapshot_invalid"
    );
}

#[test]
fn official_compaction_trigger_is_not_blocked_by_third_party_guard() {
    let mut route = route_set("provider-a".to_string(), "revision-a".to_string())
        .routes
        .remove(0);
    route.source = RouteSource::Official {
        account_id: "official-account".to_string(),
        credential_revision_id: "official-credential".to_string(),
    };
    let request = json!({"input": [{"type": "compaction_trigger"}]});
    assert_eq!(
        preflight_capabilities(&route, CompactMode::RemoteV2, &request),
        Ok(())
    );
}

#[tokio::test]
async fn compaction_trigger_stops_before_third_party_upstream() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("compaction-guard-key");
    let upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        GatewayTimeoutSnapshot {
            connect_seconds: 5,
            first_byte_seconds: 5,
            stream_idle_seconds: 5,
            non_stream_total_seconds: 5,
        },
    )
    .unwrap();
    let state = ManagedGatewayAppState {
        registry: Arc::new(RwLock::new(BTreeMap::new())),
        diagnostics: Arc::new(RwLock::new(None)),
        usage: Arc::new(RwLock::new(None)),
        archive: Arc::new(RwLock::new(None)),
        in_flight_requests: Arc::new(AtomicUsize::new(0)),
    };
    state.registry.write().await.insert(
        "compaction-instance".to_string(),
        Arc::new(BindingRuntime {
            binding_id: "compaction-instance".to_string(),
            archive_home: None,
            admission_key_hash: instance_gateway_key_hash(&key),
            route_set: route_set("provider-a".to_string(), "revision-a".to_string()),
            upstreams: [("revision-a".to_string(), upstream)].into_iter().collect(),
        }),
    );

    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(
                    r#"{"model":"deepseek-managed","input":[{"type":"compaction_trigger"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body = std::str::from_utf8(&bytes).unwrap();
    assert!(body.contains("unsupported_compaction"));
    assert!(observed.lock().unwrap().is_none());
}

#[tokio::test]
async fn remote_v2_summarizer_strips_trigger_and_returns_one_envelope() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("summarizer-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    upstream.api_key = Some(Arc::new(ApiKey::for_test("channel-summarizer-key")));
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "grok-managed".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    let state = bind_channel_state(
        "summarizer-instance",
        &key,
        routes,
        upstream,
        CompactMode::RemoteV2,
        CompactionSupport::GatewaySummarizer,
    )
    .await;

    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .header(AUTHORIZATION, "Bearer official-must-not-leave")
                .body(Body::from(compact_trigger_body("grok-managed")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let item = compaction_output_item(&body).expect("one compaction item");
    let envelope = item["encrypted_content"].as_str().unwrap();
    assert!(envelope.starts_with(compaction::ENVELOPE_PREFIX));
    assert_eq!(body["output"].as_array().unwrap().len(), 1);

    let (headers, request) = observed.lock().unwrap().take().unwrap();
    assert_eq!(request["model"], "grok-4.6");
    assert!(!request_contains_compaction_trigger(&request));
    assert_eq!(request["tools"], json!([]));
    assert_eq!(request["stream"], true);
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "Bearer channel-summarizer-key"
    );
    assert!(headers.get(INSTANCE_GATEWAY_HEADER).is_none());
    assert_ne!(
        headers.get(AUTHORIZATION).unwrap(),
        "Bearer official-must-not-leave"
    );
}

#[tokio::test]
async fn remote_v2_summarizer_collects_upstream_sse_into_one_envelope() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_grok_responses_stream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("summarizer-sse-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "grok-managed".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    let state = bind_channel_state(
        "summarizer-sse-instance",
        &key,
        routes,
        upstream,
        CompactMode::RemoteV2,
        CompactionSupport::GatewaySummarizer,
    )
    .await;
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(
                    json!({
                        "model": "grok-managed",
                        "reasoning": {"effort": "xhigh"},
                        "input": [
                            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "continue"}]},
                            {"type": "compaction_trigger"}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let item = compaction_output_item(&body).expect("one compaction item");
    assert_eq!(
        item["encrypted_content"],
        compaction::encode_envelope("GROK_CONTRACT_OK")
    );
    let (_, request) = observed.lock().unwrap().take().unwrap();
    assert_eq!(request["stream"], true);
    assert_eq!(request["reasoning"]["effort"], "medium");
    assert_eq!(request["tools"], json!([]));
}

#[tokio::test]
async fn remote_v2_chat_summarizer_expands_envelope_before_chat_adapter() {
    let observed = Arc::new(StdMutex::new(None));
    async fn mock_chat_upstream(
        State(observed): State<Arc<StdMutex<Option<(HeaderMap, Value)>>>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        *observed.lock().unwrap() = Some((headers, body.clone()));
        Json(json!({
            "id": "chatcmpl-a",
            "object": "chat.completion",
            "model": body["model"],
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "chat summary"},
                "finish_reason": "stop"
            }]
        }))
    }
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_chat_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("chat-summarizer-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::ChatCompletions,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    upstream.api_key = Some(Arc::new(ApiKey::for_test("channel-chat-summarizer-key")));
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "grok-chat-managed".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    routes.routes[0].wire_api = WireApi::ChatCompletions;
    routes.routes[0].adapter_kind = "chat_completions_to_responses".to_string();
    routes.routes[0].capabilities.supports_tools = false;
    let state = bind_channel_state(
        "chat-summarizer-instance",
        &key,
        routes,
        upstream,
        CompactMode::RemoteV2,
        CompactionSupport::GatewaySummarizer,
    )
    .await;

    let envelope = compaction::encode_envelope("prior summary");
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .header(AUTHORIZATION, "Bearer official-must-not-leave")
                .body(Body::from(
                    json!({
                        "model": "grok-chat-managed",
                        "tools": [{"type": "function", "name": "shell"}],
                        "input": [
                            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "continue"}]},
                            {"type": "compaction", "encrypted_content": envelope},
                            {"type": "compaction_trigger"}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let item = compaction_output_item(&body).expect("one compaction item");
    assert!(item["encrypted_content"]
        .as_str()
        .unwrap()
        .starts_with(compaction::ENVELOPE_PREFIX));
    assert_eq!(body["output"].as_array().unwrap().len(), 1);

    let (headers, request) = observed.lock().unwrap().take().unwrap();
    assert_eq!(request["model"], "grok-4.6");
    assert_eq!(request["stream"], true);
    assert!(request.get("input").is_none());
    assert!(request.get("tools").is_none());
    let messages = request["messages"].as_array().unwrap();
    assert!(messages
        .iter()
        .any(|message| message["role"] == "user" && message["content"] == "continue"));
    assert!(messages
        .iter()
        .any(|message| message["role"] == "assistant" && message["content"] == "prior summary"));
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "Bearer channel-chat-summarizer-key"
    );
    assert_ne!(
        headers.get(AUTHORIZATION).unwrap(),
        "Bearer official-must-not-leave"
    );
}

#[tokio::test]
async fn remote_v2_ordinary_chat_turn_expands_envelope_before_adapter() {
    let observed = Arc::new(StdMutex::new(None));
    async fn mock_chat_upstream(
        State(observed): State<Arc<StdMutex<Option<(HeaderMap, Value)>>>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        *observed.lock().unwrap() = Some((headers, body.clone()));
        Json(json!({
            "id": "chatcmpl-b",
            "object": "chat.completion",
            "model": body["model"],
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "next turn"},
                "finish_reason": "stop"
            }]
        }))
    }
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_chat_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("chat-continue-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::ChatCompletions,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "grok-chat-managed".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    routes.routes[0].wire_api = WireApi::ChatCompletions;
    routes.routes[0].adapter_kind = "chat_completions_to_responses".to_string();
    routes.routes[0].capabilities.supports_tools = false;
    let state = bind_channel_state(
        "chat-continue-instance",
        &key,
        routes,
        upstream,
        CompactMode::RemoteV2,
        CompactionSupport::GatewaySummarizer,
    )
    .await;

    let envelope = compaction::encode_envelope("prior summary");
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(
                    json!({
                        "model": "grok-chat-managed",
                        "input": [
                            {"type": "compaction", "encrypted_content": envelope},
                            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next"}]}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let (headers, request) = observed.lock().unwrap().take().unwrap();
    assert!(headers.get(AUTHORIZATION).is_none());
    let messages = request["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[0]["content"], "prior summary");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "next");
    assert!(request.get("input").is_none());
}

#[tokio::test]
async fn remote_v2_ordinary_responses_turn_expands_envelope() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("responses-continue-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "grok-managed".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    let state = bind_channel_state(
        "responses-continue-instance",
        &key,
        routes,
        upstream,
        CompactMode::RemoteV2,
        CompactionSupport::GatewaySummarizer,
    )
    .await;
    let envelope = compaction::encode_envelope("prior summary");
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(
                    json!({
                        "model": "grok-managed",
                        "tools": [{"type": "function", "name": "shell"}],
                        "input": [
                            {"type": "compaction", "encrypted_content": envelope},
                            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next"}]}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let (_, request) = observed.lock().unwrap().take().unwrap();
    let input = request["input"].as_array().unwrap();
    assert_eq!(input[0]["type"], "message");
    assert_eq!(input[0]["role"], "assistant");
    assert_eq!(input[0]["content"][0]["text"], "prior summary");
    assert_eq!(input[1]["type"], "message");
    assert_eq!(request["tools"][0]["name"], "shell");
    assert!(!request_contains_compaction_trigger(&request));
}

#[tokio::test]
async fn remote_v2_empty_summary_fails_closed_without_retry() {
    let hits = Arc::new(StdMutex::new(0u32));
    async fn empty_summary(State(hits): State<Arc<StdMutex<u32>>>) -> Json<Value> {
        *hits.lock().unwrap() += 1;
        Json(json!({
            "id": "response-empty",
            "object": "response",
            "model": "grok-4.6",
            "status": "completed",
            "output": [{
                "id": "message-empty",
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "   "}]
            }]
        }))
    }
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(empty_summary))
        .with_state(hits.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("empty-summary-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "grok-managed".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    let state = bind_channel_state(
        "empty-summary-instance",
        &key,
        routes,
        upstream,
        CompactMode::RemoteV2,
        CompactionSupport::GatewaySummarizer,
    )
    .await;
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(compact_trigger_body("grok-managed")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("compaction_summary_empty"));
    assert_eq!(*hits.lock().unwrap(), 1);
}

#[tokio::test]
async fn fake_gpt_relay_compaction_never_reaches_upstream() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("fake-gpt-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "gpt-4o".to_string();
    routes.routes[0].upstream_model_id = "gpt-4o".to_string();
    let state = bind_channel_state(
        "fake-gpt-instance",
        &key,
        routes,
        upstream,
        CompactMode::RemoteV2,
        CompactionSupport::Unsupported,
    )
    .await;
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(compact_trigger_body("gpt-4o")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("not_native_gpt_compaction"));
    assert!(observed.lock().unwrap().is_none());
}

#[tokio::test]
async fn remote_v2_foreign_envelope_never_reaches_upstream() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("foreign-envelope-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "grok-managed".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    let state = bind_channel_state(
        "foreign-envelope-instance",
        &key,
        routes,
        upstream,
        CompactMode::RemoteV2,
        CompactionSupport::GatewaySummarizer,
    )
    .await;
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(
                    json!({
                        "model": "grok-managed",
                        "input": [
                            {
                                "type": "compaction",
                                "encrypted_content": "openai-compaction-v1:abc"
                            },
                            {"type": "compaction_trigger"}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("incompatible_compaction_envelope"));
    assert!(observed.lock().unwrap().is_none());
}

#[tokio::test]
async fn remote_v2_summarizer_folds_reasoning_ciphertext_before_upstream() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("reasoning-fold-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "grok-managed".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    let state = bind_channel_state(
        "reasoning-fold-instance",
        &key,
        routes,
        upstream,
        CompactMode::RemoteV2,
        CompactionSupport::GatewaySummarizer,
    )
    .await;
    let envelope = compaction::encode_envelope("prior summary");
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(
                    json!({
                        "model": "grok-managed",
                        "tools": [{"type": "function", "name": "shell"}],
                        "input": [
                            {
                                "type": "reasoning",
                                "encrypted_content": "provider-blob",
                                "summary": [{"type": "summary_text", "text": "visible thought"}]
                            },
                            {"type": "compaction", "encrypted_content": envelope},
                            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next"}]},
                            {"type": "compaction_trigger"}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let item = compaction_output_item(&body).expect("one compaction item");
    assert!(item["encrypted_content"]
        .as_str()
        .unwrap()
        .starts_with(compaction::ENVELOPE_PREFIX));
    let (_, request) = observed.lock().unwrap().take().unwrap();
    assert_eq!(request["model"], "grok-4.6");
    assert!(!request_contains_compaction_trigger(&request));
    assert_eq!(request["tools"], json!([]));
    assert_eq!(request["stream"], true);
    let input = request["input"].as_array().unwrap();
    assert!(input
        .iter()
        .all(|item| item.get("encrypted_content").is_none()));
    assert!(input.iter().any(|item| {
        item["role"] == "assistant" && item["content"][0]["text"] == "visible thought"
    }));
    assert!(input.iter().any(|item| {
        item["role"] == "assistant" && item["content"][0]["text"] == "prior summary"
    }));
    assert!(input
        .iter()
        .any(|item| item["role"] == "user" && item["content"][0]["text"] == "next"));
}

#[tokio::test]
async fn ordinary_turn_foreign_compaction_envelope_never_reaches_upstream() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("ordinary-foreign-envelope-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "grok-managed".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    let state = bind_channel_state(
        "ordinary-foreign-envelope-instance",
        &key,
        routes,
        upstream,
        CompactMode::RemoteV2,
        CompactionSupport::GatewaySummarizer,
    )
    .await;
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(
                    json!({
                        "model": "grok-managed",
                        "input": [
                            {
                                "type": "compaction",
                                "encrypted_content": "openai-compaction-v1:abc"
                            },
                            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next"}]}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("incompatible_compaction_envelope"));
    assert!(observed.lock().unwrap().is_none());
}
