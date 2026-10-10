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
use crate::providers::ProviderStore;
use crate::{routing, OFFICIAL_ACCOUNT_AUTH_ENABLED};
use zeroize::Zeroize;

#[cfg(target_os = "macos")]
const TERMINAL_LAUNCHER_PREFIX: &str = "rcodex-";
#[cfg(target_os = "macos")]
const TERMINAL_LAUNCHER_SUFFIX: &str = ".command";
#[cfg(target_os = "macos")]
const TERMINAL_LAUNCHER_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

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
        let transport = managed_launch.launch_transport(&input.launch_preset_id)?;
        if crate::launcher_protocol::LaunchTarget::wire_is_managed_app(&launch_target)
            && transport != ManagedLaunchTransport::Gateway
        {
            return Err("managed_app_requires_gateway".to_string());
        }
        let gateway_base_url = match transport {
            ManagedLaunchTransport::Gateway => {
                let gateway = managed_runtime.ensure_serving().await?;
                Some(
                    gateway
                        .base_url
                        .as_deref()
                        .ok_or_else(|| "managed_gateway_not_serving".to_string())?
                        .parse::<url::Url>()
                        .map_err(|_| "managed_gateway_url_invalid".to_string())?,
                )
            }
            ManagedLaunchTransport::DirectResponses => None,
        };
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
        if prepared.transport_mode == ManagedLaunchTransport::Gateway {
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
            .or_else(|| {
                prepared
                    .direct_base_url
                    .as_deref()
                    .and_then(|value| url::Url::parse(value).ok())
                    .and_then(|url| url.host_str().map(str::to_owned))
            })
            .unwrap_or_else(|| "provider".to_string());
        let _ = append_startup_log(
            &launch_log,
            &format!(
                "managed_route_snapshot target={} route_set={} provider={} model={} upstream_model={} transport={} endpoint_host={}",
                launch_target,
                prepared.route_set.id,
                match prepared.transport_mode {
                    ManagedLaunchTransport::Gateway => "rcodex",
                    ManagedLaunchTransport::DirectResponses => "rcodex",
                },
                default_route.public_model_id,
                default_route.upstream_model_id,
                match prepared.transport_mode {
                    ManagedLaunchTransport::Gateway => "gateway",
                    ManagedLaunchTransport::DirectResponses => "direct_responses",
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
        let (credential_kind, secret_ref, instance_key_ref, mut launch_secret) =
            match prepared.transport_mode {
                ManagedLaunchTransport::Gateway => {
                    let Some(instance_key) = prepared.instance_key.take() else {
                        control.mark_launch_failed(&prepared.instance_id, true)?;
                        let _ = cleanup_managed_instance(
                            &managed_runtime,
                            &managed_launch,
                            &prepared.instance_id,
                        )
                        .await;
                        return Err("managed_gateway_key_missing".to_string());
                    };
                    let Some(instance_key_ref) = prepared.instance_key_ref.as_deref() else {
                        control.mark_launch_failed(&prepared.instance_id, true)?;
                        let _ = cleanup_managed_instance(
                            &managed_runtime,
                            &managed_launch,
                            &prepared.instance_id,
                        )
                        .await;
                        return Err("managed_gateway_key_missing".to_string());
                    };
                    let _ = append_startup_log(&launch_log, "instance_key_launch_copy_loaded");
                    (
                        "managed_route_set",
                        None,
                        Some(instance_key_ref),
                        instance_key.expose().to_string(),
                    )
                }
                ManagedLaunchTransport::DirectResponses => {
                    let secret_ref = prepared
                        .direct_secret_ref
                        .as_deref()
                        .ok_or_else(|| "managed_direct_provider_missing".to_string())?;
                    let _ = append_startup_log(&launch_log, "provider_key_parent_read_requested");
                    let secret = match provider_store.load_secret_for_launch(secret_ref) {
                        Ok(secret) => secret,
                        Err(error) => {
                            let _ = append_startup_log(
                                &launch_log,
                                &format!("provider_key_parent_read_failed error={error}"),
                            );
                            control.mark_launch_failed(&prepared.instance_id, true)?;
                            cleanup_managed_instance(
                                &managed_runtime,
                                &managed_launch,
                                &prepared.instance_id,
                            )
                            .await?;
                            return Err(error);
                        }
                    };
                    let _ = append_startup_log(&launch_log, "provider_key_parent_loaded");
                    ("api_key", Some(secret_ref), None, secret)
                }
            };
        let launch_result = launch_codex_in_terminal(
            &prepared.instance_id,
            &prepared.launch_nonce,
            &prepared.status_file,
            &prepared.launch_codex_home,
            credential_kind,
            secret_ref,
            instance_key_ref,
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

fn launch_codex_in_terminal(
    instance_id: &str,
    launch_nonce: &str,
    status_file: &std::path::Path,
    codex_home: &std::path::Path,
    credential_kind: &str,
    secret_ref: Option<&str>,
    instance_key_ref: Option<&str>,
    profile: Option<&str>,
    model: Option<&str>,
    working_directory: Option<&str>,
    launch_secret: Option<&str>,
    launch_target: &str,
    electron_user_data_dir: Option<&std::path::Path>,
    sqlite_home: Option<&std::path::Path>,
) -> Result<(), String> {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};

    let log_file = status_file.with_file_name("startup.log");
    let _ = append_startup_log(&log_file, "terminal_launch_requested");
    cleanup_stale_terminal_launchers();
    let executable =
        std::env::current_exe().map_err(|error| format!("无法定位 R Codex 应用：{error}"))?;
    let helper = executable
        .parent()
        .ok_or_else(|| "无法定位 R Codex 启动器目录。".to_string())?
        .join("codex-launcher");
    if !helper.is_file() {
        let _ = append_startup_log(&log_file, "launcher_binary_missing");
        return Err("未找到受管理 Codex 启动器；请重新构建 R Codex 应用。".to_string());
    }
    let script = std::env::temp_dir().join(format!("rcodex-{}.command", uuid::Uuid::new_v4()));
    let secret_file = if launch_secret.is_some() {
        let path = status_file.with_file_name("launch-secret.fifo");
        create_secret_fifo(&path)?;
        Some(path)
    } else {
        None
    };
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\\"'\\\"'"));
    let mut arguments = vec![
        quote("--target"),
        quote(launch_target),
        quote("--credential-kind"),
        quote(credential_kind),
        quote("--instance-id"),
        quote(instance_id),
        quote("--launch-nonce"),
        quote(launch_nonce),
        quote("--status-file"),
        quote(&status_file.display().to_string()),
        quote("--log-file"),
        quote(&log_file.display().to_string()),
        quote("--codex-home"),
        quote(&codex_home.display().to_string()),
    ];
    if let Some(secret_ref) = secret_ref {
        arguments.push(quote("--secret-ref"));
        arguments.push(quote(secret_ref));
    }
    if let Some(instance_key_ref) = instance_key_ref {
        arguments.push(quote("--instance-key-ref"));
        arguments.push(quote(instance_key_ref));
    }
    if let Some(profile) = profile {
        arguments.push(quote("--profile"));
        arguments.push(quote(profile));
    }
    if let Some(model) = model {
        arguments.push(quote("--model"));
        arguments.push(quote(model));
    }
    if let Some(directory) = working_directory.filter(|value| !value.trim().is_empty()) {
        arguments.push(quote("--working-directory"));
        arguments.push(quote(directory));
    }
    if let Some(secret_file) = secret_file.as_deref() {
        arguments.push(quote("--secret-file"));
        arguments.push(quote(&secret_file.display().to_string()));
    }
    if let Some(electron_user_data_dir) = electron_user_data_dir {
        arguments.push(quote("--electron-user-data-dir"));
        arguments.push(quote(&electron_user_data_dir.display().to_string()));
    }
    if let Some(sqlite_home) = sqlite_home {
        arguments.push(quote("--sqlite-home"));
        arguments.push(quote(&sqlite_home.display().to_string()));
    }
    let content = render_terminal_launch_script(&quote(&helper.display().to_string()), &arguments);
    fs::write(&script, content).map_err(|error| {
        if let Some(secret_file) = secret_file.as_deref() {
            let _ = fs::remove_file(secret_file);
        }
        let _ = append_startup_log(
            &log_file,
            &format!("terminal_script_write_failed error={error}"),
        );
        format!("无法创建安全 Codex 启动器：{error}")
    })?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).map_err(|error| {
        if let Some(secret_file) = secret_file.as_deref() {
            let _ = fs::remove_file(secret_file);
        }
        let _ = append_startup_log(
            &log_file,
            &format!("terminal_script_permission_failed error={error}"),
        );
        format!("无法保护 Codex 启动器权限：{error}")
    })?;
    if crate::launcher_protocol::LaunchTarget::wire_is_managed_app(&launch_target) {
        // A managed desktop App has its own window and must not depend on an
        // interactive Terminal session to execute the temporary script. The
        // CLI path opens iTerm2 when present, otherwise Terminal, so users
        // retain an interactive shell; the App path spawns the same supervised launcher directly.
        Command::new("/bin/sh")
            .arg(&script)
            .spawn()
            .map_err(|error| {
                if let Some(secret_file) = secret_file.as_deref() {
                    let _ = fs::remove_file(secret_file);
                }
                let _ = append_startup_log(
                    &log_file,
                    &format!("app_spawn_failed target={launch_target} error={error}"),
                );
                format!("无法启动 Codex App：{error}")
            })?;
        let _ = append_startup_log(
            &log_file,
            &format!("app_spawn_dispatched target={launch_target}"),
        );
        if let (Some(secret_file), Some(launch_secret)) = (secret_file, launch_secret) {
            dispatch_secret_handoff(secret_file, launch_secret.to_string(), log_file.clone());
        }
        return Ok(());
    }
    // Do not rely on the user's `.command` file association. Finder/open can
    // otherwise hand the file to an editor or another shell integration and
    // return success before the launcher ever runs, leaving the instance in
    // the ambiguous `orphaned` state with no receipt. Prefer iTerm2 when it is
    // installed; otherwise use Terminal.
    let preferred_terminal = macos_cli_terminal_app();
    let (status, terminal_app) = open_cli_script_in_macos_terminal(&script, preferred_terminal)
        .map_err(|error| {
            if let Some(secret_file) = secret_file.as_deref() {
                let _ = fs::remove_file(secret_file);
            }
            format!("无法在终端启动 Codex：{error}")
        })?;
    if status.success() {
        let _ = append_startup_log(
            &log_file,
            &format!("terminal_open_dispatched app={terminal_app}"),
        );
        if let (Some(secret_file), Some(launch_secret)) = (secret_file, launch_secret) {
            dispatch_secret_handoff(secret_file, launch_secret.to_string(), log_file.clone());
        }
        Ok(())
    } else {
        if let Some(secret_file) = secret_file.as_deref() {
            let _ = fs::remove_file(secret_file);
        }
        let _ = append_startup_log(&log_file, &format!("terminal_open_failed status={status}"));
        Err(format!("终端启动器以状态 {status} 退出。"))
    }
}

#[cfg(target_os = "macos")]
fn macos_cli_terminal_app() -> &'static str {
    let mut candidates = vec![
        std::path::PathBuf::from("/Applications/iTerm.app"),
        std::path::PathBuf::from("/Applications/iTerm2.app"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        candidates.push(home.join("Applications/iTerm.app"));
        candidates.push(home.join("Applications/iTerm2.app"));
    }
    macos_cli_terminal_app_from(candidates)
}

#[cfg(target_os = "macos")]
fn macos_cli_terminal_app_from(
    candidates: impl IntoIterator<Item = std::path::PathBuf>,
) -> &'static str {
    for path in candidates {
        if !path.is_dir() {
            continue;
        }
        match path.file_name().and_then(|name| name.to_str()) {
            Some("iTerm.app") => return "iTerm",
            Some("iTerm2.app") => return "iTerm2",
            _ => {}
        }
    }
    "Terminal"
}

#[cfg(target_os = "macos")]
fn open_cli_script_in_macos_terminal(
    script: &std::path::Path,
    preferred: &'static str,
) -> Result<(std::process::ExitStatus, &'static str), std::io::Error> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-a")
        .arg(preferred)
        .arg(script)
        .status()?;
    if status.success() {
        return Ok((status, preferred));
    }
    if preferred == "Terminal" {
        return Ok((status, "Terminal"));
    }
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-a")
        .arg("Terminal")
        .arg(script)
        .status()?;
    Ok((status, "Terminal"))
}

#[cfg(target_os = "macos")]
fn create_secret_fifo(path: &std::path::Path) -> Result<(), String> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let _ = std::fs::remove_file(path);
    let bytes = path.as_os_str().as_bytes();
    let c_path = CString::new(bytes).map_err(|_| "启动凭据管道路径无效。".to_string())?;
    // SAFETY: c_path is a valid NUL-terminated path and the mode restricts
    // access to the owning user. The return code is checked immediately.
    let result = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    if result != 0 {
        return Err(format!(
            "无法创建一次性启动凭据管道：{}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn dispatch_secret_handoff(
    path: std::path::PathBuf,
    mut secret: String,
    log_file: std::path::PathBuf,
) {
    use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt, time::Duration};
    std::thread::spawn(move || {
        // Terminal may take several seconds to start and execute the generated
        // script. Keep the FIFO alive for the whole handoff window; deleting it
        // after a successful write would race with the launcher's subsequent
        // open/read and turn a valid credential into ENOENT.
        for _ in 0..400 {
            let result = OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path);
            match result {
                Ok(mut file) => {
                    // A FIFO has no durable backing store; sync_all is not a
                    // meaningful success condition and may return EINVAL even
                    // after the bytes were delivered to the reader.
                    let write_result = file.write_all(secret.as_bytes());
                    secret.zeroize();
                    let event = if write_result.is_ok() {
                        "secret_handoff_written"
                    } else {
                        "secret_handoff_write_failed"
                    };
                    let _ = append_startup_log(&log_file, event);
                    return;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::WouldBlock
                    ) || error.raw_os_error() == Some(libc::ENXIO) =>
                {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
        secret.zeroize();
        let _ = std::fs::remove_file(&path);
        let _ = append_startup_log(&log_file, "secret_handoff_timeout");
    });
}

#[cfg(target_os = "macos")]
fn render_terminal_launch_script(helper: &str, arguments: &[String]) -> String {
    format!(
        "#!/bin/sh\ncleanup() {{ rm -f \"$0\"; }}\ntrap cleanup EXIT HUP INT TERM\n{helper} {}\nexit $?\n",
        arguments.join(" "),
    )
}

#[cfg(target_os = "macos")]
fn cleanup_stale_terminal_launchers() {
    let now = std::time::SystemTime::now();
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !is_managed_terminal_launcher(file_name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if now
            .duration_since(modified)
            .is_ok_and(|age| age > TERMINAL_LAUNCHER_MAX_AGE)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(target_os = "macos")]
fn is_managed_terminal_launcher(file_name: &str) -> bool {
    file_name.starts_with(TERMINAL_LAUNCHER_PREFIX) && file_name.ends_with(TERMINAL_LAUNCHER_SUFFIX)
}

#[cfg(not(target_os = "macos"))]
fn launch_codex_in_terminal(
    _instance_id: &str,
    _launch_nonce: &str,
    _status_file: &std::path::Path,
    _codex_home: &std::path::Path,
    _credential_kind: &str,
    _secret_ref: Option<&str>,
    _instance_key_ref: Option<&str>,
    _profile: Option<&str>,
    _model: Option<&str>,
    _working_directory: Option<&str>,
    _launch_secret: Option<&str>,
    _launch_target: &str,
    _electron_user_data_dir: Option<&std::path::Path>,
    _sqlite_home: Option<&std::path::Path>,
) -> Result<(), String> {
    Err("当前版本仅在 macOS 提供受管理终端启动。Windows 计划优先使用 Windows Terminal（wt.exe），没有时再打开 cmd.exe。".to_string())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn managed_terminal_launcher_script_cleans_itself_after_codex_exits() {
        let script = render_terminal_launch_script(
            "'/Applications/R Codex.app/Contents/MacOS/codex-launcher'",
            &["'--target'".to_string(), "'cli'".to_string()],
        );
        assert!(script.contains("trap cleanup EXIT HUP INT TERM"));
        assert!(script.contains("rm -f \"$0\""));
        assert!(script.contains("codex-launcher' '--target' 'cli'"));
        assert!(!script.contains("exec '/Applications"));
    }

    #[test]
    fn managed_terminal_launcher_name_is_scoped_to_rcodex_scripts() {
        assert!(is_managed_terminal_launcher("rcodex-a1b2.command"));
        assert!(!is_managed_terminal_launcher("codex.command"));
        assert!(!is_managed_terminal_launcher("rcodex-a1b2.sh"));
    }

    #[test]
    fn secret_handoff_survives_delayed_launcher_read() {
        let directory =
            std::env::temp_dir().join(format!("3r-secret-handoff-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let fifo = directory.join("launch-secret.fifo");
        let log_file = directory.join("startup.log");
        create_secret_fifo(&fifo).unwrap();
        dispatch_secret_handoff(
            fifo.clone(),
            "secret-sentinel".to_string(),
            log_file.clone(),
        );

        // Model Terminal taking time to start the generated script. The
        // parent writer must keep the FIFO present until the launcher reads it.
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(fifo.exists());
        let value = std::fs::read(&fifo).unwrap();
        assert_eq!(value, b"secret-sentinel");
        assert!(fifo.exists());
        std::fs::remove_file(&fifo).unwrap();

        for _ in 0..20 {
            if log_file.exists() {
                let log = std::fs::read_to_string(&log_file).unwrap();
                if log.contains("secret_handoff_written") {
                    std::fs::remove_dir_all(&directory).unwrap();
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::fs::remove_dir_all(&directory).unwrap();
        panic!("secret handoff writer did not report success");
    }

    #[test]
    fn macos_cli_prefers_iterm_when_the_app_bundle_exists() {
        let root = std::env::temp_dir().join(format!("3r-iterm-detect-{}", uuid::Uuid::new_v4()));
        let iterm = root.join("iTerm.app");
        std::fs::create_dir_all(&iterm).unwrap();
        assert_eq!(macos_cli_terminal_app_from(vec![iterm]), "iTerm");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn macos_cli_falls_back_to_terminal_when_iterm_is_absent() {
        let missing =
            std::env::temp_dir().join(format!("3r-iterm-missing-{}", uuid::Uuid::new_v4()));
        assert_eq!(macos_cli_terminal_app_from(vec![missing]), "Terminal");
    }
}
