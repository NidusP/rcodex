//! Gateway 控制面 IPC。
//!
//! 负责停止 sidecar 和汇报聚合绑定状态。不承载模型流量，也不修改 RouteSet。
//! 由控制面 UI 调用；数据面仍是 Codex 到本机回环 Gateway。

use crate::{codex_control::CodexControlStore, managed_gateway::ManagedGatewayClient};

/// Status returned by the Tauri commands consumed by the UI. The data plane
/// is always the Managed Gateway, so the status reports aggregate bindings
/// rather than a single bound Provider.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayStatus {
    state: GatewayState,
    base_url: Option<String>,
    bind_host: String,
    port: Option<u16>,
    active_bindings: usize,
    in_flight_requests: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GatewayState {
    Stopped,
    Serving,
    Unavailable,
}

#[tauri::command]
pub async fn stop_gateway(
    managed_runtime: tauri::State<'_, ManagedGatewayClient>,
) -> Result<GatewayStatus, String> {
    let managed = managed_runtime.status().await?;
    if managed.state == "unavailable" {
        return Ok(gateway_status_from_managed(managed));
    }
    managed_runtime
        .stop_if_unused()
        .await
        .map(gateway_status_from_managed)
}

#[tauri::command]
pub async fn restart_unavailable_gateway(
    managed_runtime: tauri::State<'_, ManagedGatewayClient>,
) -> Result<GatewayStatus, String> {
    managed_runtime
        .restart_unavailable()
        .await
        .map(gateway_status_from_managed)
}

#[tauri::command]
pub async fn cleanup_unavailable_gateway(
    managed_runtime: tauri::State<'_, ManagedGatewayClient>,
    control: tauri::State<'_, CodexControlStore>,
) -> Result<GatewayStatus, String> {
    if control.has_active_managed_instances()? {
        return Err("managed_gateway_cleanup_active_instances".to_string());
    }
    managed_runtime
        .cleanup_unavailable()
        .await
        .map(gateway_status_from_managed)
}

#[tauri::command]
pub async fn gateway_status(
    managed_runtime: tauri::State<'_, ManagedGatewayClient>,
) -> Result<GatewayStatus, String> {
    managed_runtime
        .status()
        .await
        .map(gateway_status_from_managed)
}

fn gateway_status_from_managed(
    status: crate::managed_gateway::ManagedGatewayStatus,
) -> GatewayStatus {
    GatewayStatus {
        state: match status.state.as_str() {
            "serving" => GatewayState::Serving,
            "unavailable" => GatewayState::Unavailable,
            _ => GatewayState::Stopped,
        },
        base_url: status.base_url,
        bind_host: status.bind_host.to_string(),
        port: status.port,
        active_bindings: status.active_bindings,
        in_flight_requests: status.in_flight_requests,
    }
}
