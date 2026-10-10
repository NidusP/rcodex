//! Sidecar 进程内控制面：监听、登记绑定、恢复和停止。
//!
//! 不转换 Responses/Chat，不读第三方 Key 以外的当前 binding 凭据。
//! `bin/codex-gateway` 的入口是 `run_sidecar`。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{net::TcpListener, sync::Notify};

use crate::conversation_archive::{ArchiveCapture, ConversationArchive};
use crate::diagnostics::DiagnosticsStore;
use crate::providers::{normalize_api_base_url, ProviderStore};
use crate::route_store::RouteStore;
#[cfg(test)]
use crate::routing::WireApi;
use crate::routing::{GatewayTimeoutSnapshot, RouteSetRevision};
use crate::usage::UsageStore;

use super::errors::api_error;
use super::helpers::gateway_trace;
use super::runtime_types::{
    GatewayBinding, GatewayBindingRegistration, ManagedGatewayRuntime, ProviderUpstreamRuntime,
    SidecarReady,
};
use super::{LOOPBACK_HOST, SIDECAR_PROTOCOL_VERSION};
use crate::providers::GatewayUpstream;
use std::time::Duration;

#[derive(Clone)]
pub(super) struct SidecarControlState {
    runtime: Arc<ManagedGatewayRuntime>,
    provider_store: Arc<ProviderStore>,
    archive: ConversationArchive,
    control_token: Arc<String>,
    shutdown: Arc<Notify>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArchiveControlPolicy {
    enabled: bool,
}

pub(super) async fn sidecar_archive_policy(
    State(state): State<SidecarControlState>,
    headers: HeaderMap,
    Json(input): Json<ArchiveControlPolicy>,
) -> Response {
    if !control_authorized(&headers, state.control_token.as_str()) {
        return control_denied();
    }
    let archive = state.archive.clone();
    match tokio::task::spawn_blocking(move || archive.set_enabled(input.enabled)).await {
        Ok(Ok(status)) => {
            if let Some(capture) = state.runtime.archive.read().await.as_ref() {
                capture.set_enabled(status.enabled);
            }
            Json(json!({ "enabled": status.enabled })).into_response()
        }
        _ => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "conversation_archive_policy_failed",
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UnregisterRequest {
    #[serde(rename = "instanceId", alias = "bindingId")]
    binding_id: String,
}

pub(super) fn control_authorized(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get("x-rcodex-control-token")
        .or_else(|| headers.get("x-3r-gpt-control-token"))
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

pub(super) fn control_denied() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "invalid_control_token" })),
    )
        .into_response()
}

pub(super) async fn sidecar_control_status(
    State(state): State<SidecarControlState>,
    headers: HeaderMap,
) -> Response {
    if !control_authorized(&headers, state.control_token.as_str()) {
        return control_denied();
    }
    Json(state.runtime.status().await).into_response()
}

pub(super) async fn sidecar_control_register(
    State(state): State<SidecarControlState>,
    headers: HeaderMap,
    Json(registration): Json<GatewayBindingRegistration>,
) -> Response {
    if !control_authorized(&headers, state.control_token.as_str()) {
        return control_denied();
    }
    let binding_id = registration.binding.binding_id.clone();
    let route_count = registration.binding.route_set.routes.len();
    let provider_count = registration.binding.route_set.providers.len();
    let result = if registration.credentials.is_empty() {
        state
            .runtime
            .register_binding(registration.binding, state.provider_store.as_ref())
            .await
    } else {
        state
            .runtime
            .register_binding_with_credentials(registration.binding, registration.credentials)
            .await
    };
    match result {
        Ok(()) => {
            gateway_trace(&[
                ("event", "register"),
                ("binding", &binding_id),
                ("routes", &route_count.to_string()),
                ("providers", &provider_count.to_string()),
            ]);
            Json(json!({ "ok": true })).into_response()
        }
        Err(error) => {
            gateway_trace(&[
                ("event", "register_failed"),
                ("binding", &binding_id),
                ("code", &error),
            ]);
            (StatusCode::CONFLICT, Json(json!({ "error": error }))).into_response()
        }
    }
}

pub(super) async fn sidecar_control_unregister(
    State(state): State<SidecarControlState>,
    headers: HeaderMap,
    Json(request): Json<UnregisterRequest>,
) -> Response {
    if !control_authorized(&headers, state.control_token.as_str()) {
        return control_denied();
    }
    let removed = state.runtime.unregister_binding(&request.binding_id).await;
    Json(json!({ "ok": true, "removed": removed })).into_response()
}

pub(super) async fn sidecar_control_stop(
    State(state): State<SidecarControlState>,
    headers: HeaderMap,
) -> Response {
    if !control_authorized(&headers, state.control_token.as_str()) {
        return control_denied();
    }
    match state.runtime.stop_if_unused().await {
        Ok(status) => {
            state.shutdown.notify_waiters();
            Json(status).into_response()
        }
        Err(error) => (StatusCode::CONFLICT, Json(json!({ "error": error }))).into_response(),
    }
}

/// Entry point used by the packaged `codex-gateway` sidecar binary. The
/// process owns the data-plane listener and its in-memory instance bindings;
/// the desktop app only talks to the authenticated control endpoints.
pub async fn run_sidecar(
    database_path: PathBuf,
    ready_path: PathBuf,
    control_token: String,
    data_port: u16,
) -> Result<(), String> {
    let database = crate::database::Database::open_path(&database_path)?;
    let vault = Arc::new(crate::secrets::DatabaseApiKeyVault::new(
        database.connection(),
    ));
    let provider_store = Arc::new(ProviderStore::from_database(
        database.clone(),
        vault.clone(),
    )?);
    let diagnostics = DiagnosticsStore::from_database(database.clone());
    let usage = UsageStore::from_database(database.clone());
    let runtime = Arc::new(ManagedGatewayRuntime::new());
    runtime.set_archive_database_now(database.clone());
    runtime.set_diagnostics_now(diagnostics);
    runtime.set_usage_now(usage);
    let archive_root = database_path
        .parent()
        .ok_or_else(|| "conversation_archive_root_missing".to_string())?;
    let archive = ConversationArchive::open(archive_root)?;
    runtime.set_archive_now(ArchiveCapture::start(archive.clone()));
    runtime.ensure_serving_on_port(Some(data_port)).await?;
    let data_status = runtime.status().await;
    gateway_trace(&[
        ("event", "sidecar_start"),
        ("data_url", data_status.base_url.as_deref().unwrap_or("-")),
        ("bindings", &data_status.active_bindings.to_string()),
    ]);

    let control_listener = TcpListener::bind((LOOPBACK_HOST, 0))
        .await
        .map_err(|error| format!("managed_gateway_control_bind_failed:{error}"))?;
    let control_port = control_listener
        .local_addr()
        .map_err(|error| format!("managed_gateway_control_address_failed:{error}"))?
        .port();
    let control_base_url = format!("http://{LOOPBACK_HOST}:{control_port}");
    let ready = SidecarReady {
        protocol_version: SIDECAR_PROTOCOL_VERSION,
        pid: std::process::id(),
        data_base_url: data_status
            .base_url
            .ok_or_else(|| "managed_gateway_data_url_missing".to_string())?,
        control_base_url,
        control_token,
    };
    write_sidecar_ready(&ready_path, &ready)?;

    // Publish readiness before rebuilding persisted bindings. Keychain reads
    // can block on macOS authorization; delaying the ready file made every
    // new launch wait for that unrelated recovery work. The control/data
    // listeners are safe to serve while restoration runs: new registrations
    // are checked against the same in-memory registry and a damaged binding
    // is skipped without affecting other instances.
    let restore_database = database.clone();
    let restore_runtime = runtime.clone();
    let restore_provider_store = provider_store.clone();
    tokio::spawn(async move {
        // Rebuild only bindings whose database record still points at a live
        // managed instance. Exited/failed instances are intentionally
        // excluded. Instance admission uses the hash in SQLite, not a
        // keychain round-trip.
        let _ =
            restore_active_bindings(&restore_database, &restore_runtime, &restore_provider_store)
                .await;
    });

    let shutdown = Arc::new(Notify::new());
    let control_state = SidecarControlState {
        runtime,
        provider_store,
        archive,
        control_token: Arc::new(ready.control_token.clone()),
        shutdown: shutdown.clone(),
    };
    let router = Router::new()
        .route("/control/status", post(sidecar_control_status))
        .route("/control/register", post(sidecar_control_register))
        .route("/control/unregister", post(sidecar_control_unregister))
        .route("/control/stop", post(sidecar_control_stop))
        .route("/control/archive-policy", post(sidecar_archive_policy))
        .with_state(control_state);
    let serve = axum::serve(control_listener, router).with_graceful_shutdown(async move {
        shutdown.notified().await;
    });
    let result = serve
        .await
        .map_err(|error| format!("managed_gateway_control_failed:{error}"));
    let _ = fs::remove_file(ready_path);
    result
}

pub(super) async fn restore_active_bindings(
    database: &crate::database::Database,
    runtime: &ManagedGatewayRuntime,
    provider_store: &ProviderStore,
) -> Result<usize, String> {
    let rows = {
        let connection = database.connection();
        let connection = connection
            .lock()
            .map_err(|_| "managed_gateway_restore_database_lock_failed".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT binding.instance_id, binding.route_set_revision_id,
                        binding.instance_key_ref, binding.instance_key_hash,
                        binding.created_at
                 FROM instance_gateway_bindings binding
                 JOIN codex_instances instance ON instance.id=binding.instance_id
                 JOIN launch_configuration_revisions configuration
                   ON configuration.id=instance.launch_configuration_revision_id
                 LEFT JOIN launch_presets preset ON preset.id=configuration.launch_preset_id
                 WHERE binding.state='active'
                   AND instance.state IN ('launching','running','stopping','orphaned','degraded')
                   AND COALESCE(json_extract(configuration.config_snapshot, '$.launchTarget'), preset.entry_kind, 'cli') IN ('cli', 'managed_app')
                 ORDER BY binding.created_at",
            )
            .map_err(|error| format!("managed_gateway_restore_query_failed:{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|error| format!("managed_gateway_restore_query_failed:{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("managed_gateway_restore_decode_failed:{error}"))?;
        rows
    };
    let route_store = RouteStore::from_database(database.clone());
    let mut restored = 0;
    for (binding_id, route_set_id, _key_ref, expected_hash) in rows {
        // Admission only compares the hash stored in SQLite. The plaintext
        // instance key never needs to be read back from the OS credential store.
        let Some(route_set) = route_store.ready_route_set(&route_set_id)? else {
            continue;
        };
        if runtime
            .register_binding(
                GatewayBinding {
                    binding_id,
                    route_set,
                    admission_key_hash: expected_hash,
                },
                provider_store,
            )
            .await
            .is_ok()
        {
            restored += 1;
        }
    }
    Ok(restored)
}

pub(super) fn write_sidecar_ready(path: &Path, ready: &SidecarReady) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("managed_gateway_ready_directory_failed:{error}"))?;
    }
    let temporary = path.with_extension("json.tmp");
    let content = serde_json::to_vec(ready)
        .map_err(|error| format!("managed_gateway_ready_encode_failed:{error}"))?;
    fs::write(&temporary, content)
        .map_err(|error| format!("managed_gateway_ready_write_failed:{error}"))?;
    #[cfg(unix)]
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("managed_gateway_ready_permission_failed:{error}"))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("managed_gateway_ready_publish_failed:{error}"))
}

pub(super) fn validate_admission_key_hash(value: &str) -> Result<(), String> {
    if value.starts_with("sha256:") {
        Ok(())
    } else {
        Err("admission_key_hash_invalid".to_string())
    }
}

pub(super) fn validate_isolated_binding(route_set: &RouteSetRevision) -> Result<(), String> {
    let snapshot: Value = serde_json::from_str(&route_set.gateway_service.config_snapshot)
        .map_err(|_| "gateway_service_snapshot_invalid".to_string())?;
    if snapshot
        .get("compatibilityFallback")
        .is_some_and(|value| !value.is_null())
    {
        return Err("compatibility_mode_removed".to_string());
    }
    Ok(())
}

pub(super) fn provider_runtime(
    upstream: GatewayUpstream,
    timeouts: GatewayTimeoutSnapshot,
) -> Result<ProviderUpstreamRuntime, String> {
    timeouts.validate().map_err(|error| error.to_string())?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(timeouts.connect_seconds))
        .build()
        .map_err(|error| format!("managed_gateway_client_failed:{error}"))?;
    Ok(ProviderUpstreamRuntime {
        provider_id: upstream.provider_id,
        provider_revision_id: upstream.provider_revision_id,
        base_url: normalize_api_base_url(&upstream.base_url)?,
        api_key: Some(Arc::new(upstream.api_key)),
        client,
        timeouts,
    })
}

#[cfg(test)]
pub(super) fn fixture_upstream(
    base_url: &str,
    wire_api: WireApi,
    timeouts: GatewayTimeoutSnapshot,
) -> Result<ProviderUpstreamRuntime, String> {
    timeouts.validate().map_err(|error| error.to_string())?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(timeouts.connect_seconds))
        .build()
        .map_err(|error| format!("managed_gateway_client_failed:{error}"))?;
    Ok(ProviderUpstreamRuntime {
        provider_id: "test-provider".to_string(),
        provider_revision_id: format!("test-provider-{wire_api:?}"),
        base_url: normalize_api_base_url(base_url)?,
        api_key: None,
        client,
        timeouts,
    })
}
