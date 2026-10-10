//! 启动方案与项目导入 IPC。
//!
//! 管理 CLI/App 启动方案、登录渠道选择，以及从默认 Codex 只读导入会话副本。
//! 不启动进程、不改源历史；启动编排在 instances。
//! 由工作区配置和导入面板调用。

use super::system::resolve_current_codex_home;
use super::CODEX_LAUNCH_IMPORT_LOCK;
use crate::codex_control::{
    CodexControlStore, CreateLaunchPresetInput, LaunchPresetSummary, RenameLaunchPresetInput,
    UpdateLaunchPresetInput,
};
use crate::managed_launch::{
    CreateManagedLaunchPresetInput, ManagedLaunchStore, ManagedPresetCompositionSummary,
    ManagedPresetSaved,
};
use crate::{routing, session_import, OFFICIAL_ACCOUNT_AUTH_ENABLED};

#[tauri::command]
pub fn create_launch_preset(
    input: CreateLaunchPresetInput,
    control: tauri::State<'_, CodexControlStore>,
) -> Result<LaunchPresetSummary, String> {
    control.create_preset(input)
}

#[tauri::command]
pub fn create_managed_launch_preset(
    input: CreateManagedLaunchPresetInput,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
) -> Result<ManagedPresetSaved, String> {
    if !OFFICIAL_ACCOUNT_AUTH_ENABLED
        && (input.official_account_id.is_some()
            || input
                .models
                .iter()
                .any(|model| matches!(model.source_kind, routing::RouteKind::Official)))
    {
        return Err("official_auth_disabled".to_string());
    }
    managed_launch.create_preset(input)
}

#[tauri::command]
pub fn get_managed_launch_preset_compositions(
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
) -> Result<Vec<ManagedPresetCompositionSummary>, String> {
    managed_launch.compositions()
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetManagedLoginProviderInput {
    launch_preset_id: String,
    expected_version: i64,
    provider_id: Option<String>,
}

#[tauri::command]
pub fn set_managed_login_provider(
    input: SetManagedLoginProviderInput,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
) -> Result<ManagedPresetSaved, String> {
    managed_launch.set_managed_login_provider(
        &input.launch_preset_id,
        input.expected_version,
        input.provider_id.as_deref(),
    )
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectManagedLaunchPresetInput {
    launch_preset_id: String,
}

#[tauri::command]
pub fn select_managed_launch_preset(
    input: SelectManagedLaunchPresetInput,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
) -> Result<ManagedPresetSaved, String> {
    managed_launch.select_preset(&input.launch_preset_id)
}

#[tauri::command]
pub async fn list_importable_session_projects(
) -> Result<Vec<session_import::ImportableProject>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        session_import::list_importable_projects(&resolve_current_codex_home())
    })
    .await
    .map_err(|_| "session_import_worker_failed".to_string())?
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportSessionsInput {
    launch_preset_id: String,
    cwd: String,
    confirmed_stopped: bool,
}

#[tauri::command]
pub async fn import_sessions_from_default(
    input: ImportSessionsInput,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
) -> Result<session_import::SessionImportResult, String> {
    let _operation = CODEX_LAUNCH_IMPORT_LOCK
        .try_lock()
        .map_err(|_| "resource_operation_in_progress".to_string())?;
    let (isolated, isolated_sqlite) =
        managed_launch.isolated_codex_storage_for_preset(&input.launch_preset_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        session_import::import_project_sessions(
            &resolve_current_codex_home(),
            &isolated,
            &isolated_sqlite,
            &input.cwd,
            input.confirmed_stopped,
        )
    })
    .await
    .map_err(|_| "session_import_worker_failed".to_string())?
}

#[tauri::command]
pub fn update_launch_preset(
    launch_preset_id: String,
    input: UpdateLaunchPresetInput,
    control: tauri::State<'_, CodexControlStore>,
) -> Result<LaunchPresetSummary, String> {
    control.update_preset(&launch_preset_id, input)
}

#[tauri::command]
pub fn rename_launch_preset(
    launch_preset_id: String,
    input: RenameLaunchPresetInput,
    control: tauri::State<'_, CodexControlStore>,
) -> Result<LaunchPresetSummary, String> {
    control.rename_preset(&launch_preset_id, &input.name, input.expected_version)
}

#[tauri::command]
pub fn delete_launch_preset(
    launch_preset_id: String,
    control: tauri::State<'_, CodexControlStore>,
) -> Result<(), String> {
    control.delete_preset(&launch_preset_id)
}
