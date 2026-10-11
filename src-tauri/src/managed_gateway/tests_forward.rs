//! 协议转换与上游转发测试。

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
use crate::diagnostics::DiagnosticsStore;
use crate::usage::UsageStore;
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
    providers::{CreateProviderInput, ProviderStore, TransportMode},
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
async fn upstream_plain_text_errors_are_preserved_for_diagnostics() {
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new().route("/error", post(mock_plain_text_502));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/error"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        upstream_error_detail(response, None).await.as_deref(),
        Some("Unknown error")
    );
}

#[tokio::test]
async fn provider_route_isolates_headers_and_rewrites_models() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let vault = Arc::new(MemoryVault::default());
    let database = Database::in_memory();
    let store = ProviderStore::from_database(database.clone(), vault).unwrap();
    let provider = store
        .create(CreateProviderInput {
            name: "Relay".to_string(),
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "provider-sentinel".to_string(),
            transport_mode: TransportMode::GatewayPassthrough,
            gateway_timeout_settings: None,
        })
        .unwrap();
    let upstream = store.upstream_for_provider(&provider.id).unwrap();
    let instance_key = InstanceGatewayKey::for_test("instance-sentinel");
    let runtime = ManagedGatewayRuntime::new();
    runtime.set_diagnostics_now(DiagnosticsStore::from_database(database.clone()));
    runtime.set_usage_now(UsageStore::from_database(database.clone()));
    database
        .connection()
        .lock()
        .unwrap()
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let status = runtime.ensure_serving().await.unwrap();
    runtime
        .register_binding(
            GatewayBinding {
                binding_id: "instance-a".to_string(),
                route_set: route_set(provider.id, upstream.provider_revision_id),
                admission_key_hash: instance_gateway_key_hash(&instance_key),
            },
            &store,
        )
        .await
        .unwrap();

    let response = reqwest::Client::new()
        .post(format!("{}/responses", status.base_url.unwrap()))
        .header(INSTANCE_GATEWAY_HEADER, instance_key.expose())
        .header(AUTHORIZATION, "Bearer official-sentinel")
        .json(&json!({"model":"deepseek-managed","input":"hello","metadata":{"session_id":"redacted"}}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let response_body = response.json::<Value>().await.unwrap();
    assert_eq!(response_body["model"], "deepseek-managed");

    let (headers, request_body) = observed.lock().unwrap().take().unwrap();
    assert_eq!(request_body["model"], "deepseek-chat");
    assert!(request_body.get("metadata").is_none());
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "Bearer provider-sentinel"
    );
    assert!(headers.get(INSTANCE_GATEWAY_HEADER).is_none());
    assert!(headers.values().all(|value| !value
        .as_bytes()
        .windows(17)
        .any(|part| part == b"official-sentinel")));
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let diagnostics: (Option<i64>, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT input_tokens, output_tokens, total_tokens FROM gateway_request_diagnostics",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(diagnostics, (Some(12), Some(7), Some(19)));
}

#[tokio::test]
async fn provider_plain_text_502_is_returned_with_upstream_detail() {
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new().route("/v1/responses", post(mock_plain_text_502));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let vault = Arc::new(MemoryVault::default());
    let database = Database::in_memory();
    let store = ProviderStore::from_database(database, vault).unwrap();
    let provider = store
        .create(CreateProviderInput {
            name: "Relay 502".to_string(),
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "provider-sentinel".to_string(),
            transport_mode: TransportMode::GatewayPassthrough,
            gateway_timeout_settings: None,
        })
        .unwrap();
    let upstream = store.upstream_for_provider(&provider.id).unwrap();
    let key = InstanceGatewayKey::for_test("plain-502-instance");
    let runtime = ManagedGatewayRuntime::new();
    let status = runtime.ensure_serving().await.unwrap();
    runtime
        .register_binding(
            GatewayBinding {
                binding_id: "plain-502-instance".to_string(),
                route_set: route_set(provider.id, upstream.provider_revision_id),
                admission_key_hash: instance_gateway_key_hash(&key),
            },
            &store,
        )
        .await
        .unwrap();
    let response = reqwest::Client::new()
        .post(format!("{}/responses", status.base_url.unwrap()))
        .header(INSTANCE_GATEWAY_HEADER, key.expose())
        .json(&json!({"model":"deepseek-managed","input":"hello"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = response.json::<Value>().await.unwrap();
    let message = body["error"]["message"].as_str().unwrap();
    assert!(!message.trim().is_empty());
    assert_eq!(body["message"], message);
    assert_eq!(body["error"]["code"], "upstream_error");
    assert!(message.contains("Unknown error"));
    assert!(message.contains(&format!("http://127.0.0.1:{port}/v1/responses")));
    assert!(message.contains(&format!("127.0.0.1:{}", gateway_listen_port())));
    assert_eq!(
        body["error"]["upstream_url"],
        format!("http://127.0.0.1:{port}/v1/responses")
    );
}

#[test]
fn codex_visible_error_message_is_never_empty() {
    let message = codex_visible_error_message(
        "upstream_error",
        "upstream",
        Some("https://relay.example/v1/responses"),
        Some("Unknown error"),
    );
    assert!(message.contains("https://relay.example/v1/responses"));
    assert!(message.contains("Unknown error"));
    assert!(!message.trim().is_empty());
    let local = codex_visible_error_message("model_not_in_route_set", "gateway", None, None);
    assert!(local.contains("RouteSet"));
    assert!(!local.trim().is_empty());
    let reasoning =
        codex_visible_error_message("incompatible_reasoning_envelope", "gateway", None, None);
    assert!(reasoning.contains("推理密文"));
    assert!(!reasoning.contains("压缩信封"));
    let envelope =
        codex_visible_error_message("incompatible_compaction_envelope", "gateway", None, None);
    assert!(envelope.contains("压缩信封"));
    assert!(!envelope.contains("推理密文"));
}

#[test]
fn safe_upstream_url_strips_userinfo_and_query() {
    assert_eq!(
        safe_upstream_url("https://user:secret@relay.example/v1/responses?token=abc#frag"),
        "https://relay.example/v1/responses"
    );
    assert_eq!(safe_upstream_url("not a url"), "[invalid-upstream-url]");
}

#[test]
fn annotate_upstream_detail_prefixes_url_without_secrets() {
    assert_eq!(
        annotate_upstream_detail(
            "https://user:key@relay.example/v1/responses",
            Some("Unknown error".into())
        )
        .as_deref(),
        Some("https://relay.example/v1/responses · Unknown error")
    );
}

#[tokio::test]
async fn stream_request_with_sync_upstream_json_is_completed_as_sse() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let vault = Arc::new(MemoryVault::default());
    let database = Database::in_memory();
    let store = ProviderStore::from_database(database.clone(), vault).unwrap();
    let provider = store
        .create(CreateProviderInput {
            name: "Relay".to_string(),
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "provider-sentinel".to_string(),
            transport_mode: TransportMode::GatewayPassthrough,
            gateway_timeout_settings: None,
        })
        .unwrap();
    let upstream = store.upstream_for_provider(&provider.id).unwrap();
    let instance_key = InstanceGatewayKey::for_test("instance-sentinel");
    let runtime = ManagedGatewayRuntime::new();
    runtime.set_diagnostics_now(DiagnosticsStore::from_database(database.clone()));
    runtime.set_usage_now(UsageStore::from_database(database.clone()));
    database
        .connection()
        .lock()
        .unwrap()
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let status = runtime.ensure_serving().await.unwrap();
    runtime
        .register_binding(
            GatewayBinding {
                binding_id: "instance-sync-stream".to_string(),
                route_set: route_set(provider.id, upstream.provider_revision_id),
                admission_key_hash: instance_gateway_key_hash(&instance_key),
            },
            &store,
        )
        .await
        .unwrap();

    let response = reqwest::Client::new()
        .post(format!("{}/responses", status.base_url.unwrap()))
        .header(INSTANCE_GATEWAY_HEADER, instance_key.expose())
        .json(&json!({"model":"deepseek-managed","stream":true,"input":"hello"}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    let body = response.text().await.unwrap();
    assert!(body.contains("event: response.created"));
    assert!(body.contains("event: response.output_item.done"));
    assert!(body.contains("event: response.completed"));
    assert!(body.contains("\"model\":\"deepseek-managed\""));
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let usage: (Option<i64>, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT input_tokens, output_tokens, total_tokens FROM gateway_request_diagnostics",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(usage, (Some(12), Some(7), Some(19)));
}

#[tokio::test]
async fn streamed_responses_usage_is_persisted_after_the_final_event() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_grok_responses_stream))
        .with_state(observed);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let vault = Arc::new(MemoryVault::default());
    let database = Database::in_memory();
    let store = ProviderStore::from_database(database.clone(), vault).unwrap();
    let provider = store
        .create(CreateProviderInput {
            name: "Stream relay".to_string(),
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "provider-sentinel".to_string(),
            transport_mode: TransportMode::GatewayPassthrough,
            gateway_timeout_settings: None,
        })
        .unwrap();
    let upstream = store.upstream_for_provider(&provider.id).unwrap();
    let instance_key = InstanceGatewayKey::for_test("stream-instance-sentinel");
    let runtime = ManagedGatewayRuntime::new();
    runtime.set_diagnostics_now(DiagnosticsStore::from_database(database.clone()));
    runtime.set_usage_now(UsageStore::from_database(database.clone()));
    database
        .connection()
        .lock()
        .unwrap()
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let status = runtime.ensure_serving().await.unwrap();
    runtime
        .register_binding(
            GatewayBinding {
                binding_id: "instance-stream-usage".to_string(),
                route_set: route_set(provider.id, upstream.provider_revision_id),
                admission_key_hash: instance_gateway_key_hash(&instance_key),
            },
            &store,
        )
        .await
        .unwrap();

    let response = reqwest::Client::new()
        .post(format!("{}/responses", status.base_url.unwrap()))
        .header(INSTANCE_GATEWAY_HEADER, instance_key.expose())
        .json(&json!({"model":"deepseek-managed","stream":true,"input":"hello"}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let body = response.text().await.unwrap();
    assert!(body.contains("response.completed"));
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let usage: (Option<i64>, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT input_tokens, output_tokens, total_tokens FROM gateway_request_diagnostics",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(usage, (Some(1), Some(1), Some(2)));
}

#[tokio::test]
async fn admission_and_model_lookup_are_exact_with_no_fallback() {
    let state = ManagedGatewayAppState {
        registry: Arc::new(RwLock::new(BTreeMap::new())),
        diagnostics: Arc::new(RwLock::new(None)),
        usage: Arc::new(RwLock::new(None)),
        archive: Arc::new(RwLock::new(None)),
        in_flight_requests: Arc::new(AtomicUsize::new(0)),
    };
    let key = InstanceGatewayKey::for_test("instance-a");
    state.registry.write().await.insert(
        "instance-a".to_string(),
        Arc::new(BindingRuntime {
            binding_id: "instance-a".to_string(),
            archive_home: None,
            admission_key_hash: instance_gateway_key_hash(&key),
            route_set: route_set("provider-a".to_string(), "revision-a".to_string()),
            upstreams: BTreeMap::new(),
        }),
    );
    let app = managed_gateway_router(state);

    let missing_key = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"model":"deepseek-managed"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_key.status(), StatusCode::UNAUTHORIZED);

    let legacy_header = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(LEGACY_INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(r#"{"model":"deepseek"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(legacy_header.status(), StatusCode::NOT_FOUND);

    let conflicting_headers = app
        .clone()
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .header(LEGACY_INSTANCE_GATEWAY_HEADER, "different-key")
                .body(Body::from(r#"{"model":"deepseek"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(conflicting_headers.status(), StatusCode::UNAUTHORIZED);

    let missing_model = app
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(r#"{"model":"deepseek"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_model.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn luna_without_alias_or_fallback_is_not_in_route_set() {
    let key = InstanceGatewayKey::for_test("no-alias-key");
    let state = ManagedGatewayAppState {
        registry: Arc::new(RwLock::new(BTreeMap::new())),
        diagnostics: Arc::new(RwLock::new(None)),
        usage: Arc::new(RwLock::new(None)),
        archive: Arc::new(RwLock::new(None)),
        in_flight_requests: Arc::new(AtomicUsize::new(0)),
    };
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "grok-4.6".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    state.registry.write().await.insert(
        "no-alias".to_string(),
        Arc::new(BindingRuntime {
            binding_id: "no-alias".to_string(),
            archive_home: None,
            admission_key_hash: instance_gateway_key_hash(&key),
            route_set: routes,
            upstreams: BTreeMap::new(),
        }),
    );
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .body(Body::from(r#"{"model":"gpt-5.6-luna"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn official_route_remains_closed_without_a_verified_contract() {
    let state = ManagedGatewayAppState {
        registry: Arc::new(RwLock::new(BTreeMap::new())),
        diagnostics: Arc::new(RwLock::new(None)),
        usage: Arc::new(RwLock::new(None)),
        archive: Arc::new(RwLock::new(None)),
        in_flight_requests: Arc::new(AtomicUsize::new(0)),
    };
    let key = InstanceGatewayKey::for_test("official-instance");
    let mut route_set = route_set("provider-a".to_string(), "revision-a".to_string());
    route_set.requires_openai_auth = true;
    route_set.official_account_id = Some("account-a".to_string());
    route_set.official_credential_revision_id = Some("credential-a".to_string());
    route_set.providers.clear();
    route_set.routes[0].source = RouteSource::Official {
        account_id: "account-a".to_string(),
        credential_revision_id: "credential-a".to_string(),
    };
    state.registry.write().await.insert(
        "official-instance".to_string(),
        Arc::new(BindingRuntime {
            binding_id: "official-instance".to_string(),
            archive_home: None,
            admission_key_hash: instance_gateway_key_hash(&key),
            route_set,
            upstreams: BTreeMap::new(),
        }),
    );
    let response = managed_gateway_router(state)
        .oneshot(
            axum::http::Request::post("/v1/responses")
                .header(CONTENT_TYPE, "application/json")
                .header(INSTANCE_GATEWAY_HEADER, key.expose())
                .header(AUTHORIZATION, "Bearer official-sentinel")
                .body(Body::from(
                    r#"{"model":"deepseek-managed","input":"hello"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("official_route_unavailable"));
}

#[test]
fn sse_rewrite_changes_only_model_fields() {
    let mut value = json!({
        "type": "response.created",
        "response": {"model": "deepseek-chat", "text": "deepseek-chat"}
    });
    rewrite_model_fields(&mut value, "deepseek-chat", "deepseek-managed");
    assert_eq!(value["response"]["model"], "deepseek-managed");
    assert_eq!(value["response"]["text"], "deepseek-chat");
}

#[test]
fn request_usage_accepts_responses_and_chat_shapes_without_estimating() {
    let responses = usage_from_event_value(&json!({
        "response": {"usage": {"input_tokens": 12, "output_tokens": 7, "total_tokens": 19}}
    }))
    .unwrap();
    assert_eq!(responses.input_tokens, Some(12));
    assert_eq!(responses.output_tokens, Some(7));
    assert_eq!(responses.total_tokens, Some(19));

    let chat = usage_from_event_value(&json!({
        "usage": {"prompt_tokens": 3, "completion_tokens": 2}
    }))
    .unwrap();
    assert_eq!(chat.input_tokens, Some(3));
    assert_eq!(chat.output_tokens, Some(2));
    assert_eq!(chat.total_tokens, Some(5));
    assert!(usage_from_event_value(&json!({"output": "text"})).is_none());
}

#[test]
fn unsupported_optional_web_search_is_removed_without_dropping_supported_tools() {
    let route = route_set("provider-a".to_string(), "revision-a".to_string())
        .routes
        .remove(0);
    let mut request = serde_json::Map::from_iter([
        (
            "tools".to_string(),
            json!([
                {"type": "web_search"},
                {"type": "function", "name": "local_tool"}
            ]),
        ),
        ("tool_choice".to_string(), json!("auto")),
    ]);
    filter_unsupported_optional_tools(&route, &mut request);
    assert_eq!(request["tools"].as_array().unwrap().len(), 1);
    assert_eq!(request["tools"][0]["name"], "local_tool");
    assert_eq!(request["tool_choice"], "auto");

    request.insert("tools".to_string(), json!([{"type": "web_search_preview"}]));
    request.insert("parallel_tool_calls".to_string(), json!(true));
    filter_unsupported_optional_tools(&route, &mut request);
    assert!(request.get("tools").is_none());
    assert!(request.get("tool_choice").is_none());
    assert!(request.get("parallel_tool_calls").is_none());
}

#[test]
fn routed_agent_message_becomes_a_clean_public_message() {
    let mut request = serde_json::Map::from_iter([(
        "input".to_string(),
        json!([
            {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "parent task"}],
                "internal_chat_message_metadata_passthrough": {"thread_id": "private"}
            },
            {
                "type": "agent_message",
                "id": "amsg_private",
                "author": "/root/child",
                "recipient": "/root",
                "internal_chat_message_metadata_passthrough": {"turn_id": "private"},
                "content": [
                    {"type": "encrypted_content", "encrypted_content": "Inspect the target file."},
                    {"type": "input_text", "text": "Return concise findings."}
                ]
            }
        ]),
    )]);

    assert_eq!(normalize_routed_agent_messages(&mut request), Ok(()));
    let input = request["input"].as_array().unwrap();
    assert!(input.iter().all(|item| item
        .get("internal_chat_message_metadata_passthrough")
        .is_none()));
    assert_eq!(input[1]["type"], "message");
    assert_eq!(input[1]["role"], "user");
    assert!(input[1].get("id").is_none());
    assert!(input[1].get("author").is_none());
    assert!(input[1].get("recipient").is_none());
    assert_eq!(
        input[1]["content"][0]["text"],
        "Agent message [[\"author\",\"/root/child\"],[\"recipient\",\"/root\"]]"
    );
    assert_eq!(input[1]["content"][1]["text"], "Inspect the target file.");
    assert_eq!(input[1]["content"][2]["text"], "Return concise findings.");
    assert!(input[1]["content"]
        .as_array()
        .unwrap()
        .iter()
        .all(|part| part["type"] == "input_text"));
}

#[test]
fn routed_agent_message_rejects_opaque_or_invalid_content() {
    for content in [
        json!([{"type": "encrypted_content", "encrypted_content": "3r-compaction-v1:opaque"}]),
        json!([{"type": "encrypted_content", "encrypted_content": "gAAAAABopaque"}]),
        json!([{"type": "output_text", "text": "not input"}]),
        json!([]),
    ] {
        let mut request = serde_json::Map::from_iter([(
            "input".to_string(),
            json!([{"type": "agent_message", "content": content}]),
        )]);
        assert_eq!(
            normalize_routed_agent_messages(&mut request),
            Err("unreadable_agent_message")
        );
    }
}

#[test]
fn failed_subtask_message_instructs_parent_to_continue() {
    let mut request = serde_json::Map::from_iter([(
        "input".to_string(),
        json!([{
            "type": "agent_message",
            "author": "/root/child",
            "recipient": "/root",
            "content": [{
                "type": "input_text",
                "text": "Message Type: FINAL_ANSWER\nPayload:\nAgent errored: upstream unavailable"
            }]
        }]),
    )]);

    assert_eq!(normalize_routed_agent_messages(&mut request), Ok(()));
    let content = request["input"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["text"], SUBTASK_FAILURE_RECOVERY_NOTICE);
    assert_eq!(
        content.last().unwrap()["text"],
        "Message Type: FINAL_ANSWER\nPayload:\nAgent errored: upstream unavailable"
    );
}

#[tokio::test]
async fn routed_agent_message_is_normalized_before_provider_forward() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("agent-message-forward-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let state = bind_channel_state(
        "agent-message-forward-instance",
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
                        "input": [{
                            "type": "agent_message",
                            "author": "/root/child",
                            "recipient": "/root",
                            "internal_chat_message_metadata_passthrough": {"turn_id": "private"},
                            "content": [{
                                "type": "encrypted_content",
                                "encrypted_content": "Inspect the target file."
                            }]
                        }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let (_, request) = observed.lock().unwrap().take().unwrap();
    assert_eq!(request["model"], "deepseek-chat");
    let item = &request["input"][0];
    assert_eq!(item["type"], "message");
    assert_eq!(item["role"], "user");
    assert!(item.get("author").is_none());
    assert!(item.get("recipient").is_none());
    assert!(item
        .get("internal_chat_message_metadata_passthrough")
        .is_none());
    assert_eq!(item["content"][1]["text"], "Inspect the target file.");
}

#[test]
fn relay_compat_applies_only_to_non_gpt_family() {
    let mut route = route_set("provider-a".to_string(), "revision-a".to_string())
        .routes
        .remove(0);
    assert!(needs_relay_compat(&route));

    route.public_model_id = "gpt-5.4".to_string();
    route.upstream_model_id = "gpt-5.4".to_string();
    assert!(!needs_relay_compat(&route));

    route.public_model_id = "hashed-public".to_string();
    route.upstream_model_id = "o3-mini".to_string();
    assert!(!needs_relay_compat(&route));

    route.public_model_id = "gpt-5.6-luna".to_string();
    route.upstream_model_id = "grok-4.6".to_string();
    assert!(needs_relay_compat(&route));
}

#[tokio::test]
async fn gpt_family_responses_are_forwarded_without_relay_compat() {
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_upstream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let key = InstanceGatewayKey::for_test("gpt-passthrough-key");
    let mut upstream = fixture_upstream(
        &format!("http://127.0.0.1:{port}"),
        WireApi::Responses,
        test_timeouts(),
    )
    .unwrap();
    upstream.provider_id = "provider-a".to_string();
    upstream.provider_revision_id = "revision-a".to_string();
    let mut routes = route_set("provider-a".to_string(), "revision-a".to_string());
    routes.routes[0].public_model_id = "gpt-managed".to_string();
    routes.routes[0].upstream_model_id = "gpt-5.4".to_string();
    let state = bind_channel_state(
        "gpt-passthrough-instance",
        &key,
        routes,
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
                        "model": "gpt-managed",
                        "metadata": {"session_id": "keep-me"},
                        "tools": [
                            {"type": "web_search"},
                            {"type": "function", "name": "local_tool"}
                        ],
                        "input": [{
                            "type": "agent_message",
                            "author": "/root/child",
                            "recipient": "/root",
                            "internal_chat_message_metadata_passthrough": {"turn_id": "private"},
                            "content": [{
                                "type": "encrypted_content",
                                "encrypted_content": "Inspect the target file."
                            }]
                        }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let (_, request) = observed.lock().unwrap().take().unwrap();
    assert_eq!(request["model"], "gpt-5.4");
    assert_eq!(request["metadata"]["session_id"], "keep-me");
    assert_eq!(request["tools"].as_array().unwrap().len(), 2);
    assert_eq!(request["tools"][0]["type"], "web_search");
    let item = &request["input"][0];
    assert_eq!(item["type"], "agent_message");
    assert_eq!(item["author"], "/root/child");
    assert_eq!(item["recipient"], "/root");
    assert_eq!(
        item["internal_chat_message_metadata_passthrough"]["turn_id"],
        "private"
    );
}

#[test]
fn chat_adapter_accepts_codex_multi_message_text_and_ignores_reasoning_metadata() {
    let input = json!({
        "model": "grok-upstream",
        "input": [
            {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Follow policy."}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hello"}]}
        ],
        "reasoning": {"effort": "low"},
        "previous_response_id": null,
        "stream": true
    });
    let chat = response_request_to_chat(&input).unwrap();
    assert_eq!(chat["model"], "grok-upstream");
    assert_eq!(chat["messages"][0]["role"], "system");
    assert_eq!(chat["messages"][0]["content"], "Follow policy.");
    assert_eq!(chat["messages"][1]["role"], "user");
    assert_eq!(chat["messages"][1]["content"], "Hello");
    assert_eq!(chat["stream"], true);
    assert!(chat.get("reasoning").is_none());
}
