//! Gateway 诊断 IPC。
//!
//! 读取和清理事件、请求与会话摘要。不记录请求/响应正文或凭据。
//! 由诊断抽屉和实例详情调用；写入路径仍在 DiagnosticsStore。

use crate::diagnostics::{
    ConversationSummaryEvent, DiagnosticsStore, GatewayDiagnosticEvent, RequestDiagnosticEvent,
};

#[tauri::command]
pub fn gateway_recent_diagnostics(
    provider_id: Option<String>,
    diagnostics_store: tauri::State<'_, DiagnosticsStore>,
) -> Result<Vec<GatewayDiagnosticEvent>, String> {
    diagnostics_store.recent(provider_id.as_deref())
}

#[tauri::command]
pub fn clear_gateway_diagnostics(
    diagnostics_store: tauri::State<'_, DiagnosticsStore>,
) -> Result<u64, String> {
    diagnostics_store.clear_all()
}

#[tauri::command]
pub fn instance_request_diagnostics(
    instance_id: String,
    diagnostics_store: tauri::State<'_, DiagnosticsStore>,
) -> Result<Vec<RequestDiagnosticEvent>, String> {
    diagnostics_store.request_recent(&instance_id)
}

#[tauri::command]
pub fn recent_gateway_requests(
    diagnostics_store: tauri::State<'_, DiagnosticsStore>,
) -> Result<Vec<RequestDiagnosticEvent>, String> {
    diagnostics_store.recent_requests()
}

#[tauri::command]
pub fn instance_conversation_summaries(
    instance_id: String,
    diagnostics_store: tauri::State<'_, DiagnosticsStore>,
) -> Result<Vec<ConversationSummaryEvent>, String> {
    diagnostics_store.conversation_recent(&instance_id)
}

#[tauri::command]
pub fn clear_instance_diagnostics(
    instance_id: String,
    diagnostics_store: tauri::State<'_, DiagnosticsStore>,
) -> Result<u64, String> {
    diagnostics_store.clear_instance(&instance_id)
}
