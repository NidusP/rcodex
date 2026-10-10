//! Runtime / 绑定 / 准入控制测试。

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
use axum::{
    body::{Body, Bytes},
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
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::RwLock;
use url::Url;

#[test]
fn control_authorization_prefers_new_header_and_accepts_legacy_header() {
    let mut headers = HeaderMap::new();
    headers.insert("x-rcodex-control-token", HeaderValue::from_static("new"));
    assert!(control_authorized(&headers, "new"));

    let mut legacy_headers = HeaderMap::new();
    legacy_headers.insert("x-3r-gpt-control-token", HeaderValue::from_static("legacy"));
    assert!(control_authorized(&legacy_headers, "legacy"));
    assert!(!control_authorized(&legacy_headers, "new"));
}

#[test]
fn old_sidecar_status_defaults_in_flight_requests_to_zero() {
    let status: runtime_types::SidecarWireStatus = serde_json::from_value(json!({
        "state": "serving",
        "baseUrl": "http://127.0.0.1:43123/v1",
        "bindHost": "127.0.0.1",
        "port": 43123,
        "activeBindings": 1,
    }))
    .unwrap();
    assert_eq!(status.in_flight_requests, 0);
    let status = ManagedGatewayStatus::from(status);
    assert_eq!(status.in_flight_requests, 0);
    assert_eq!(
        serde_json::to_value(status)
            .unwrap()
            .get("inFlightRequests")
            .and_then(Value::as_u64),
        Some(0)
    );
}

#[tokio::test]
async fn in_flight_guard_releases_after_body_eof_or_drop() {
    let counter = Arc::new(AtomicUsize::new(0));
    let response = Response::new(Body::from_stream(futures_util::stream::iter([Ok::<
        Bytes,
        axum::Error,
    >(
        Bytes::from_static(b"ok"),
    )])));
    let response = attach_in_flight_guard(response, InFlightRequestGuard::new(counter.clone()));
    assert_eq!(counter.load(Ordering::Acquire), 1);
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert_eq!(body, Bytes::from_static(b"ok"));
    assert_eq!(counter.load(Ordering::Acquire), 0);

    let response = Response::new(Body::from_stream(futures_util::stream::pending::<
        Result<Bytes, axum::Error>,
    >()));
    let response = attach_in_flight_guard(response, InFlightRequestGuard::new(counter.clone()));
    assert_eq!(counter.load(Ordering::Acquire), 1);
    drop(response);
    assert_eq!(counter.load(Ordering::Acquire), 0);
}

#[test]
fn gateway_binding_accepts_domain_names_and_emits_legacy_wire_names() {
    let binding = GatewayBinding {
        binding_id: "default-app-compatibility".to_string(),
        route_set: route_set("provider-a".to_string(), "revision-a".to_string()),
        admission_key_hash: "sha256:admission".to_string(),
    };
    let encoded = serde_json::to_value(&binding).unwrap();
    assert_eq!(encoded["instanceId"], "default-app-compatibility");
    assert_eq!(encoded["instanceKeyHash"], "sha256:admission");
    assert!(encoded.get("bindingId").is_none());
    assert!(encoded.get("admissionKeyHash").is_none());
    assert!(encoded.get("compatibilityFallback").is_none());

    let mut legacy_or_new = encoded;
    let object = legacy_or_new.as_object_mut().unwrap();
    let legacy_id = object.remove("instanceId").unwrap();
    let legacy_hash = object.remove("instanceKeyHash").unwrap();
    object.insert("bindingId".to_string(), legacy_id);
    object.insert("admissionKeyHash".to_string(), legacy_hash);
    let decoded: GatewayBinding = serde_json::from_value(legacy_or_new).unwrap();
    assert_eq!(decoded.binding_id, binding.binding_id);
    assert_eq!(decoded.admission_key_hash, binding.admission_key_hash);
}

#[tokio::test]
async fn gateway_cannot_stop_while_a_binding_is_active() {
    let runtime = ManagedGatewayRuntime::new();
    runtime.ensure_serving().await.unwrap();
    runtime.registry.write().await.insert(
        "instance-a".to_string(),
        Arc::new(BindingRuntime {
            binding_id: "instance-a".to_string(),
            archive_home: None,
            admission_key_hash: "sha256:invalid".to_string(),
            route_set: route_set("provider-a".to_string(), "revision-a".to_string()),
            upstreams: BTreeMap::new(),
        }),
    );
    assert_eq!(
        runtime.stop_if_unused().await.unwrap_err(),
        "configuration_in_use"
    );
    assert!(runtime.unregister_binding("instance-a").await);
    assert_eq!(runtime.stop_if_unused().await.unwrap().state, "stopped");
}

#[tokio::test]
async fn two_instance_keys_resolve_to_two_immutable_catalogs() {
    let state = ManagedGatewayAppState {
        registry: Arc::new(RwLock::new(BTreeMap::new())),
        diagnostics: Arc::new(RwLock::new(None)),
        usage: Arc::new(RwLock::new(None)),
        archive: Arc::new(RwLock::new(None)),
        in_flight_requests: Arc::new(AtomicUsize::new(0)),
    };
    let first_key = InstanceGatewayKey::for_test("instance-first");
    let second_key = InstanceGatewayKey::for_test("instance-second");
    let mut first = route_set("provider-a".to_string(), "revision-a".to_string());
    let mut internal_route = first.routes[0].clone();
    internal_route.id = "route-internal".to_string();
    internal_route.route_identity_id = "identity-internal".to_string();
    internal_route.public_model_id = "codex-auto-review".to_string();
    internal_route.upstream_model_id = "codex-auto-review".to_string();
    first.routes.push(internal_route);
    let mut second = route_set("provider-b".to_string(), "revision-b".to_string());
    second.id = "route-set-b".to_string();
    second.routes[0].id = "route-b".to_string();
    second.routes[0].route_identity_id = "identity-b".to_string();
    second.routes[0].public_model_id = "grok-managed".to_string();
    second.routes[0].upstream_model_id = "grok-4".to_string();
    state.registry.write().await.insert(
        "instance-first".to_string(),
        Arc::new(BindingRuntime {
            binding_id: "instance-first".to_string(),
            archive_home: None,
            admission_key_hash: instance_gateway_key_hash(&first_key),
            route_set: first,
            upstreams: BTreeMap::new(),
        }),
    );
    state.registry.write().await.insert(
        "instance-second".to_string(),
        Arc::new(BindingRuntime {
            binding_id: "instance-second".to_string(),
            archive_home: None,
            admission_key_hash: instance_gateway_key_hash(&second_key),
            route_set: second,
            upstreams: BTreeMap::new(),
        }),
    );
    let app = managed_gateway_router(state);

    for (key, expected, absent) in [
        (first_key.expose(), "deepseek-managed", "grok-managed"),
        (second_key.expose(), "grok-managed", "deepseek-managed"),
    ] {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/v1/models")
                    .header(INSTANCE_GATEWAY_HEADER, key)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let value = serde_json::from_slice::<Value>(&bytes).unwrap();
        let encoded = value.to_string();
        assert!(encoded.contains(expected));
        assert!(!encoded.contains(absent));
        assert!(!encoded.contains("codex-auto-review"));
    }
}
