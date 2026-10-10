//! 完整请求档案 IPC。
//!
//! 只暴露档案元数据、分页正文、策略开关和清空。正文不进入普通诊断或全局快照。
//! 由请求档案面板调用；采集与容量合同仍在 ConversationArchive。

use crate::conversation_archive::{
    ArchiveBodyPage, ArchiveRequestDetail, ArchiveRequestSummary, ArchiveStatus,
    ConversationArchive,
};
use crate::managed_gateway::ManagedGatewayClient;

#[tauri::command]
pub fn conversation_archive_status(
    archive: tauri::State<'_, ConversationArchive>,
) -> Result<ArchiveStatus, String> {
    archive.status()
}

#[tauri::command]
pub async fn conversation_archive_set_policy(
    enabled: bool,
    archive: tauri::State<'_, ConversationArchive>,
    gateway: tauri::State<'_, ManagedGatewayClient>,
) -> Result<ArchiveStatus, String> {
    gateway.set_archive_policy(enabled).await?;
    archive.status()
}

#[tauri::command]
pub fn conversation_archive_list(
    limit: Option<u32>,
    archive: tauri::State<'_, ConversationArchive>,
) -> Result<Vec<ArchiveRequestSummary>, String> {
    archive.list(limit.unwrap_or(50))
}

#[tauri::command]
pub fn conversation_archive_detail(
    request_id: String,
    archive: tauri::State<'_, ConversationArchive>,
) -> Result<Option<ArchiveRequestDetail>, String> {
    archive.detail(&request_id)
}

#[tauri::command]
pub fn conversation_archive_body(
    request_id: String,
    stage: String,
    offset: u64,
    limit: Option<u32>,
    archive: tauri::State<'_, ConversationArchive>,
) -> Result<ArchiveBodyPage, String> {
    archive.body_page(&request_id, &stage, offset, limit.unwrap_or(64 * 1024))
}

#[tauri::command]
pub fn conversation_archive_clear(
    archive: tauri::State<'_, ConversationArchive>,
) -> Result<(), String> {
    archive.clear()
}
