//! 受管理实例生命周期 IPC。
//!
//! 覆盖快照、启动、调和、停止、删除，以及 terminal/FIFO/sidecar 启动编排。
//! 不写入工作区渠道配置；RouteSet 与凭据仍由 ManagedLaunchStore / ProviderStore 提供。
//! 由工作区启动按钮和实例详情调用。

use super::system::{codex_version, resolve_current_codex_home};
use super::CODEX_LAUNCH_IMPORT_LOCK;
use crate::codex_control::{
    managed_app_compatibility_enabled, CodexControlSnapshot, CodexControlStore,
    CodexInstanceSummary, LaunchCodexInput,
};
use crate::codex_installation::resolve_codex_executable;
use crate::launcher_protocol::append_startup_log;
use crate::managed_codex_config::project_isolated_auth_json;
use crate::managed_gateway::{GatewayBinding, ManagedGatewayClient, ManagedGatewayCredential};
use crate::managed_launch::{ManagedLaunchStore, ManagedLaunchTransport};
use crate::managed_spawn::launch_codex_in_terminal;
use crate::providers::ProviderStore;
use crate::{routing, OFFICIAL_ACCOUNT_AUTH_ENABLED};
use zeroize::Zeroize;

#[tauri::command]
pub async fn get_codex_control_snapshot(
    control: tauri::State<'_, CodexControlStore>,
    managed_runtime: tauri::State<'_, ManagedGatewayClient>,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
) -> Result<CodexControlSnapshot, String> {
    let snapshot = control.snapshot()?;
    for instance_id in managed_launch.terminal_active_bindings()? {
        cleanup_managed_instance(&managed_runtime, &managed_launch, &instance_id).await?;
    }
    if snapshot
        .instances
        .iter()
        .any(|instance| crate::codex_control::InstanceState::is_finished(instance.state.as_str()))
    {
        control.snapshot()
    } else {
        Ok(snapshot)
    }
}

#[tauri::command]
pub async fn launch_codex(
    input: LaunchCodexInput,
    managed_runtime: tauri::State<'_, ManagedGatewayClient>,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
    control: tauri::State<'_, CodexControlStore>,
    provider_store: tauri::State<'_, ProviderStore>,
    _app: tauri::AppHandle,
) -> Result<CodexInstanceSummary, String> {
    let _operation = CODEX_LAUNCH_IMPORT_LOCK
        .try_lock()
        .map_err(|_| "resource_operation_in_progress".to_string())?;
    if !OFFICIAL_ACCOUNT_AUTH_ENABLED
        && control
            .preset_credential_kind(&input)
            .as_deref()
            .is_ok_and(crate::codex_control::CredentialKind::wire_is_official)
    {
        return Err("official_auth_disabled".to_string());
    }
    let codex_executable = resolve_codex_executable()?;
    let codex_version = codex_version(&codex_executable)?;
    if managed_launch.is_ready_managed_preset(&input.launch_preset_id)? {
        let launch_target = input.launch_target.as_deref().unwrap_or("cli");
        if matches!(launch_target, "compat_cli" | "compat_app") {
            return Err("compatibility_mode_removed".to_string());
        }
        if crate::launcher_protocol::LaunchTarget::from_wire(launch_target).is_none() {
            return Err("invalid_launch_target".to_string());
        }
        if crate::launcher_protocol::LaunchTarget::wire_is_managed_app(&launch_target)
            && !managed_app_compatibility_enabled()
        {
            return Err("managed_app_unavailable".to_string());
        }
        let _transport = managed_launch.launch_transport(&input.launch_preset_id)?;
        let gateway = managed_runtime.ensure_serving().await?;
        let gateway_base_url = Some(
            gateway
                .base_url
                .as_deref()
                .ok_or_else(|| "managed_gateway_not_serving".to_string())?
                .parse::<url::Url>()
                .map_err(|_| "managed_gateway_url_invalid".to_string())?,
        );
        let mut prepared = match managed_launch.prepare_launch_with_target(
            &input.launch_preset_id,
            input.expected_version,
            input.workspace_path.as_deref(),
            &codex_executable,
            &codex_version,
            gateway_base_url.as_ref(),
            &resolve_current_codex_home(),
            launch_target,
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = managed_runtime.stop_if_unused().await;
                return Err(error);
            }
        };
        if let Some(provider_id) = prepared.login_provider_id.clone() {
            let auth_result = provider_store
                .load_current_provider_key(&provider_id)
                .and_then(|key| {
                    project_isolated_auth_json(
                        &prepared.managed_codex_home,
                        &resolve_current_codex_home(),
                        key.expose(),
                    )
                });
            if let Err(error) = auth_result {
                control.mark_launch_failed(&prepared.instance_id, true)?;
                let _ = cleanup_managed_instance(
                    &managed_runtime,
                    &managed_launch,
                    &prepared.instance_id,
                )
                .await;
                return Err(error);
            }
        }
        {
            let provider_store_for_launch = (*provider_store).clone();
            let provider_revisions = prepared.route_set.providers.clone();
            let credentials_result: Result<Vec<ManagedGatewayCredential>, String> =
                match tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    tokio::task::spawn_blocking(move || {
                        provider_revisions
                            .iter()
                            .map(|provider| {
                                let upstream = provider_store_for_launch
                                    .upstream_for_revision(&provider.provider_revision_id)?;
                                if upstream.provider_id != provider.provider_id
                                    || upstream.provider_revision_id
                                        != provider.provider_revision_id
                                {
                                    return Err("provider_revision_mismatch".to_string());
                                }
                                Ok(ManagedGatewayCredential {
                                    provider_id: upstream.provider_id,
                                    provider_revision_id: upstream.provider_revision_id,
                                    base_url: upstream.base_url,
                                    provider_name: upstream.provider_name,
                                    transport_mode: upstream.transport_mode,
                                    api_key: upstream.api_key.expose().to_string(),
                                })
                            })
                            .collect::<Result<Vec<_>, String>>()
                    }),
                )
                .await
                {
                    Err(_) => Err("managed_gateway_credential_timeout".to_string()),
                    Ok(join_result) => match join_result {
                        Err(_) => Err("managed_gateway_credential_worker_failed".to_string()),
                        Ok(result) => result,
                    },
                };
            let credentials = match credentials_result {
                Ok(credentials) => credentials,
                Err(error) => {
                    control.mark_launch_failed(&prepared.instance_id, true)?;
                    let _ = cleanup_managed_instance(
                        &managed_runtime,
                        &managed_launch,
                        &prepared.instance_id,
                    )
                    .await;
                    return Err(error);
                }
            };
            // A stable managed-App instance may have an old in-memory Gateway
            // binding left behind after the App exited before UI cleanup ran.
            // Remove it before registering the fresh immutable RouteSet so a
            // restart cannot fail with a duplicate binding/opaque 502.
            let _ = managed_runtime
                .unregister_binding(&prepared.instance_id)
                .await;
            let instance_key_hash = match prepared.instance_key_hash.clone() {
                Some(value) => value,
                None => {
                    control.mark_launch_failed(&prepared.instance_id, true)?;
                    let _ = cleanup_managed_instance(
                        &managed_runtime,
                        &managed_launch,
                        &prepared.instance_id,
                    )
                    .await;
                    return Err("managed_gateway_key_missing".to_string());
                }
            };
            if let Err(error) = managed_runtime
                .register_binding_with_credentials(
                    GatewayBinding {
                        binding_id: prepared.instance_id.clone(),
                        route_set: prepared.route_set.clone(),
                        admission_key_hash: instance_key_hash,
                    },
                    credentials,
                )
                .await
            {
                control.mark_launch_failed(&prepared.instance_id, true)?;
                let _ = cleanup_managed_instance(
                    &managed_runtime,
                    &managed_launch,
                    &prepared.instance_id,
                )
                .await;
                return Err(error);
            }
        }
        let launch_log = prepared.status_file.with_file_name("startup.log");
        let default_route = prepared
            .route_set
            .routes
            .first()
            .ok_or_else(|| "route_set_has_no_models".to_string())?;
        let endpoint_host = gateway_base_url
            .as_ref()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| "gateway".to_string());
        let _ = append_startup_log(
            &launch_log,
            &format!(
                "managed_route_snapshot target={} route_set={} provider={} model={} upstream_model={} transport={} endpoint_host={}",
                launch_target,
                prepared.route_set.id,
                match prepared.transport_mode {
                    ManagedLaunchTransport::Gateway => "rcodex",
                },
                default_route.public_model_id,
                default_route.upstream_model_id,
                match prepared.transport_mode {
                    ManagedLaunchTransport::Gateway => "gateway",
                },
                endpoint_host
            ),
        );
        for server_id in &prepared.filtered_mcp_servers {
            let _ = append_startup_log(
                &launch_log,
                &format!(
                    "mcp_filtered_without_official_auth server={server_id} policy=external_api"
                ),
            );
        }
        for route in &prepared.route_set.routes {
            let source = match &route.source {
                routing::RouteSource::ApiProvider {
                    provider_id,
                    provider_revision_id,
                } => format!("api_provider:{provider_id}@{provider_revision_id}"),
                routing::RouteSource::Official {
                    account_id,
                    credential_revision_id,
                } => format!("official:{account_id}@{:?}", credential_revision_id),
            };
            let _ = append_startup_log(
                &launch_log,
                &format!(
                    "managed_route_mapping public_model={} upstream_model={} source={source}",
                    route.public_model_id, route.upstream_model_id
                ),
            );
        }
        let Some(instance_key) = prepared.instance_key.take() else {
            control.mark_launch_failed(&prepared.instance_id, true)?;
            let _ =
                cleanup_managed_instance(&managed_runtime, &managed_launch, &prepared.instance_id)
                    .await;
            return Err("managed_gateway_key_missing".to_string());
        };
        let Some(instance_key_ref) = prepared.instance_key_ref.as_deref() else {
            control.mark_launch_failed(&prepared.instance_id, true)?;
            let _ =
                cleanup_managed_instance(&managed_runtime, &managed_launch, &prepared.instance_id)
                    .await;
            return Err("managed_gateway_key_missing".to_string());
        };
        let _ = append_startup_log(&launch_log, "instance_key_launch_copy_loaded");
        let credential_kind = "managed_route_set";
        let secret_ref = None;
        let mut launch_secret = instance_key.expose().to_string();
        let launch_result = launch_codex_in_terminal(
            &prepared.instance_id,
            &prepared.launch_nonce,
            &prepared.status_file,
            &prepared.launch_codex_home,
            credential_kind,
            secret_ref,
            Some(instance_key_ref),
            Some(prepared.profile_name.as_str()),
            Some(default_route.public_model_id.as_str()),
            prepared.workspace_path.as_deref(),
            Some(&launch_secret),
            launch_target,
            prepared.electron_user_data_dir.as_deref(),
            prepared.sqlite_home.as_deref(),
        );
        launch_secret.zeroize();
        if let Err(error) = launch_result {
            control.mark_launch_failed(&prepared.instance_id, true)?;
            cleanup_managed_instance(&managed_runtime, &managed_launch, &prepared.instance_id)
                .await?;
            return Err(error);
        }
        let result = await_launch_confirmation(&control, &prepared.instance_id).await;
        // A late terminal authorization still starts the CLI with the key that
        // matches this binding. Tearing the binding down here makes that CLI
        // unable to reach the Gateway.
        let unconfirmed = result
            .as_ref()
            .err()
            .is_some_and(|error| error == "launch_unconfirmed");
        if !unconfirmed
            && (result.as_ref().is_ok_and(|instance| {
                crate::codex_control::InstanceState::is_finished(instance.state.as_str())
            }) || result.is_err())
        {
            let _ =
                cleanup_managed_instance(&managed_runtime, &managed_launch, &prepared.instance_id)
                    .await;
        }
        return result;
    }
    if control.preset_credential_kind(&input)? == "official" {
        let prepared =
            control.prepare_official_launch(&input, &codex_executable, &codex_version)?;
        if let Err(error) = launch_codex_in_terminal(
            &prepared.instance_id,
            &prepared.launch_nonce,
            &prepared.status_file,
            &prepared.managed_codex_home,
            "official",
            None,
            None,
            None,
            None,
            prepared.workspace_path.as_deref(),
            None,
            "cli",
            None,
            None,
        ) {
            control.mark_launch_failed(&prepared.instance_id, true)?;
            return Err(error);
        }
        return await_launch_confirmation(&control, &prepared.instance_id).await;
    }
    // All API Key launches now use an immutable managed RouteSet and the
    // shared Managed Gateway. Legacy v5 presets are intentionally preserved
    // in the database, but their migration marks them as needing model
    // selection instead of silently rebuilding a single-Provider route.
    if input
        .launch_target
        .as_deref()
        .is_some_and(|target| target != "cli")
    {
        return Err("managed_app_requires_managed_preset".to_string());
    }
    Err("launch_preset_needs_model_selection".to_string())
}

pub(crate) async fn stop_active_instances_and_gateway(
    control: &crate::codex_control::CodexControlStore,
    managed_runtime: &ManagedGatewayClient,
    managed_launch: &ManagedLaunchStore,
) -> Result<(), String> {
    let active = control
        .snapshot()?
        .instances
        .into_iter()
        .filter(|instance| {
            matches!(
                instance.state.as_str(),
                "launching" | "running" | "stopping" | "orphaned" | "degraded"
            )
        })
        .map(|instance| instance.id)
        .collect::<Vec<_>>();
    for instance_id in active {
        control.stop_instance(&instance_id)?;
        cleanup_managed_instance(managed_runtime, managed_launch, &instance_id).await?;
    }
    managed_runtime.stop_if_unused().await.map(|_| ())
}

async fn cleanup_managed_instance(
    managed_runtime: &ManagedGatewayClient,
    managed_launch: &ManagedLaunchStore,
    instance_id: &str,
) -> Result<(), String> {
    // Keep all three cleanup operations on the same path and always attempt
    // the Gateway stop, even when one of the earlier operations fails. This
    // makes reconciliation retryable and prevents a database error from
    // leaving an otherwise unused sidecar running indefinitely.
    let unregister_result = match managed_runtime.unregister_binding(instance_id).await {
        Ok(()) => Ok(()),
        Err(error) if error == "managed_gateway_not_serving" => Ok(()),
        Err(error) => Err(error),
    };
    let revoke_result = managed_launch.revoke_binding_deferred(instance_id);
    let stop_result = managed_runtime.stop_if_unused().await;

    if let Err(error) = unregister_result {
        return Err(format!("managed_gateway_binding_cleanup_failed:{error}"));
    }
    revoke_result?;
    match stop_result {
        Ok(_) => Ok(()),
        Err(error) if error == "configuration_in_use" => Ok(()),
        Err(error) => Err(format!("managed_gateway_stop_failed:{error}")),
    }
}

async fn cleanup_managed_instance_for_delete(
    managed_runtime: &ManagedGatewayClient,
    managed_launch: &ManagedLaunchStore,
    instance_id: &str,
) -> Result<(), String> {
    let unregister_result = match managed_runtime.unregister_binding(instance_id).await {
        Ok(()) => Ok(()),
        Err(error) if error == "managed_gateway_not_serving" => Ok(()),
        Err(error) => Err(error),
    };
    if let Err(error) = unregister_result {
        let _ = managed_runtime.stop_if_unused().await;
        return Err(format!("managed_gateway_binding_cleanup_failed:{error}"));
    }

    let revoke_result = managed_launch.revoke_binding_for_delete(instance_id);
    let delete_result = revoke_result
        .is_ok()
        .then(|| managed_launch.delete_instance_storage(instance_id));
    let stop_result = managed_runtime.stop_if_unused().await;
    if let Err(error) = revoke_result {
        return Err(error);
    }
    if let Some(Err(error)) = delete_result {
        return Err(error);
    }
    match stop_result {
        Ok(_) => Ok(()),
        Err(error) if error == "configuration_in_use" => Ok(()),
        Err(error) => Err(format!("managed_gateway_stop_failed:{error}")),
    }
}

async fn await_launch_confirmation(
    control: &CodexControlStore,
    instance_id: &str,
) -> Result<CodexInstanceSummary, String> {
    for _ in 0..50 {
        let instance = control.reconcile_instance(instance_id)?;
        if instance.state != "launching" {
            return Ok(instance);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err("launch_unconfirmed".to_string())
}

#[tauri::command]
pub async fn reconcile_codex_instance(
    instance_id: String,
    control: tauri::State<'_, CodexControlStore>,
    managed_runtime: tauri::State<'_, ManagedGatewayClient>,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
) -> Result<CodexInstanceSummary, String> {
    let instance = control.reconcile_instance(&instance_id)?;
    if crate::codex_control::InstanceState::is_finished(instance.state.as_str()) {
        cleanup_managed_instance(&managed_runtime, &managed_launch, &instance_id).await?;
        // Return the post-cleanup snapshot so callers can immediately observe
        // the terminal state and stop polling the instance.
        return control.reconcile_instance(&instance_id);
    }
    Ok(instance)
}

#[tauri::command]
pub async fn stop_codex_instance(
    instance_id: String,
    control: tauri::State<'_, CodexControlStore>,
    managed_runtime: tauri::State<'_, ManagedGatewayClient>,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
) -> Result<CodexInstanceSummary, String> {
    let instance = control.stop_instance(&instance_id)?;
    if crate::codex_control::InstanceState::is_finished(instance.state.as_str()) {
        cleanup_managed_instance(&managed_runtime, &managed_launch, &instance_id).await?;
    }
    let _ = managed_runtime.stop_if_unused().await;
    control.reconcile_instance(&instance_id)
}

#[tauri::command]
pub async fn delete_codex_instance(
    instance_id: String,
    control: tauri::State<'_, CodexControlStore>,
    managed_runtime: tauri::State<'_, ManagedGatewayClient>,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
) -> Result<(), String> {
    control.ensure_instance_deletable(&instance_id)?;
    cleanup_managed_instance_for_delete(&managed_runtime, &managed_launch, &instance_id).await?;
    control.delete_instance(&instance_id)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SafeInstanceDeleteResult {
    requested: usize,
    deleted: usize,
    skipped: Vec<serde_json::Value>,
}

#[tauri::command]
pub async fn delete_safe_codex_instances(
    control: tauri::State<'_, CodexControlStore>,
    managed_runtime: tauri::State<'_, ManagedGatewayClient>,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
) -> Result<SafeInstanceDeleteResult, String> {
    let instance_ids = control.instance_ids()?;
    let requested = instance_ids.len();
    let mut deleted = 0;
    let mut skipped = Vec::new();
    for instance_id in instance_ids {
        if let Err(error) = control.ensure_instance_deletable(&instance_id) {
            skipped.push(serde_json::json!({ "instanceId": instance_id, "state": error }));
            continue;
        }
        if let Err(error) =
            cleanup_managed_instance_for_delete(&managed_runtime, &managed_launch, &instance_id)
                .await
        {
            skipped.push(serde_json::json!({ "instanceId": instance_id, "state": error }));
            continue;
        }
        match control.delete_instance(&instance_id) {
            Ok(()) => deleted += 1,
            Err(error) => {
                skipped.push(serde_json::json!({ "instanceId": instance_id, "state": error }))
            }
        }
    }
    let _ = managed_runtime.stop_if_unused().await;
    Ok(SafeInstanceDeleteResult {
        requested,
        deleted,
        skipped,
    })
}
