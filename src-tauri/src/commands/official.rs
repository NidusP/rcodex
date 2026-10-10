//! 官方账户授权 IPC。
//!
//! start/poll/cancel/open/refresh/disconnect/delete 全部经过同一道发布门。
//! 当前产品关闭官方 OAuth；命令保留注册，Store 仍保存已有账户数据。
//! 控制面不再挂载官方账户面板。

use crate::codex_installation::resolve_codex_executable;
use crate::official_accounts::{
    AccountActionInput, AuthorizationMode, AuthorizationPrompt, AuthorizationStarted,
    AuthorizationStatus, OfficialAccountError, OfficialAccountService, OfficialAccountStatus,
    StartAuthorizationInput,
};
use crate::OFFICIAL_ACCOUNT_AUTH_ENABLED;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartCodexAccountAuthorizationInput {
    account_id: Option<String>,
    workspace_label: Option<String>,
    mode: Option<AuthorizationMode>,
}

#[tauri::command]
pub async fn start_codex_account_authorization(
    input: StartCodexAccountAuthorizationInput,
    service: tauri::State<'_, OfficialAccountService>,
) -> Result<AuthorizationStarted, OfficialAccountError> {
    ensure_official_account_auth_enabled()?;
    let codex_executable = resolve_codex_executable().map_err(official_command_error)?;
    let service = service.inner().clone();
    let worker = service.clone();
    let started = tauri::async_runtime::spawn_blocking(move || {
        worker.start_authorization(StartAuthorizationInput {
            mode: input.mode.unwrap_or(AuthorizationMode::Browser),
            account_id: input.account_id,
            workspace_label: input.workspace_label,
            codex_executable: codex_executable.display().to_string(),
        })
    })
    .await
    .map_err(|_| official_command_error("authorization_worker_failed"))??;

    if let AuthorizationPrompt::Browser { auth_url } = &started.prompt {
        if let Err(error) = open_official_authorization_url(auth_url) {
            let attempt_id = started.attempt_id.clone();
            let service = service.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                service.cancel_authorization(&attempt_id)
            })
            .await;
            return Err(official_command_error(error));
        }
    }
    Ok(started)
}

#[tauri::command]
pub async fn poll_codex_account_authorization(
    attempt_id: String,
    service: tauri::State<'_, OfficialAccountService>,
) -> Result<AuthorizationStatus, OfficialAccountError> {
    ensure_official_account_auth_enabled()?;
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.poll_authorization(&attempt_id))
        .await
        .map_err(|_| official_command_error("authorization_worker_failed"))?
}

#[tauri::command]
pub async fn cancel_codex_account_authorization(
    attempt_id: String,
    service: tauri::State<'_, OfficialAccountService>,
) -> Result<AuthorizationStatus, OfficialAccountError> {
    ensure_official_account_auth_enabled()?;
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.cancel_authorization(&attempt_id))
        .await
        .map_err(|_| official_command_error("authorization_worker_failed"))?
}

#[tauri::command]
pub async fn open_codex_account_authorization_prompt(
    attempt_id: String,
    service: tauri::State<'_, OfficialAccountService>,
) -> Result<(), OfficialAccountError> {
    ensure_official_account_auth_enabled()?;
    let service = service.inner().clone();
    let prompt_url =
        tauri::async_runtime::spawn_blocking(move || service.authorization_prompt_url(&attempt_id))
            .await
            .map_err(|_| official_command_error("authorization_worker_failed"))??;
    open_official_authorization_url(&prompt_url).map_err(official_command_error)
}

#[tauri::command]
pub async fn refresh_codex_account_status(
    account_id: String,
    service: tauri::State<'_, OfficialAccountService>,
) -> Result<OfficialAccountStatus, OfficialAccountError> {
    ensure_official_account_auth_enabled()?;
    run_official_account_action(account_id, service, |service, input| {
        service.refresh_account_status(input)
    })
    .await
}

#[tauri::command]
pub async fn disconnect_codex_account(
    account_id: String,
    service: tauri::State<'_, OfficialAccountService>,
) -> Result<OfficialAccountStatus, OfficialAccountError> {
    ensure_official_account_auth_enabled()?;
    run_official_account_action(account_id, service, |service, input| {
        service.disconnect_account(input)
    })
    .await
}

#[tauri::command]
pub async fn delete_codex_account(
    account_id: String,
    service: tauri::State<'_, OfficialAccountService>,
) -> Result<OfficialAccountStatus, OfficialAccountError> {
    ensure_official_account_auth_enabled()?;
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service.delete_account(AccountActionInput {
            account_id,
            codex_executable: String::new(),
        })
    })
    .await
    .map_err(|_| official_command_error("authorization_worker_failed"))?
}

async fn run_official_account_action<F>(
    account_id: String,
    service: tauri::State<'_, OfficialAccountService>,
    action: F,
) -> Result<OfficialAccountStatus, OfficialAccountError>
where
    F: FnOnce(
            OfficialAccountService,
            AccountActionInput,
        ) -> Result<OfficialAccountStatus, OfficialAccountError>
        + Send
        + 'static,
{
    let codex_executable = resolve_codex_executable().map_err(official_command_error)?;
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        action(
            service,
            AccountActionInput {
                account_id,
                codex_executable: codex_executable.display().to_string(),
            },
        )
    })
    .await
    .map_err(|_| official_command_error("authorization_worker_failed"))?
}

fn official_command_error(message: impl Into<String>) -> OfficialAccountError {
    OfficialAccountError {
        code: "official_auth_command_failed".to_string(),
        message: message.into(),
        resources: Vec::new(),
        instance_ids: Vec::new(),
        operation_id: None,
        retryable: true,
    }
}

fn ensure_official_account_auth_enabled() -> Result<(), OfficialAccountError> {
    if OFFICIAL_ACCOUNT_AUTH_ENABLED {
        return Ok(());
    }
    Err(official_command_error("official_auth_disabled"))
}

#[cfg(target_os = "macos")]
fn open_official_authorization_url(auth_url: &str) -> Result<(), String> {
    let status = std::process::Command::new("open")
        .arg(auth_url)
        .status()
        .map_err(|_| "authorization_browser_open_failed".to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("authorization_browser_open_failed".to_string())
    }
}

#[cfg(not(target_os = "macos"))]
fn open_official_authorization_url(_auth_url: &str) -> Result<(), String> {
    Err("authorization_browser_open_unsupported".to_string())
}
