//! 官方账户公开类型、错误码和本地辅助函数。
//!
//! 不访问 app-server 进程。由 service / authorize / account 共用。

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{params, Transaction};
use serde::{Deserialize, Serialize};

use crate::database::resource_guard::{CommandError, ResourceKind, ResourceRef};
use crate::official_auth::{
    AccountKind, AccountRateLimits, AccountSnapshot, LoginCompleted, OfficialAuthError,
};

pub(crate) const AUTH_POLL_TIMEOUT: Duration = Duration::from_millis(75);
pub(crate) const QUOTA_CACHE_TTL_SECONDS: i64 = 5 * 60;
const WORKSPACE_LABEL_MAX_BYTES: usize = 160;
pub(crate) const OP_AUTHORIZE: &str = "official_account_authorize";
pub(crate) const OP_REFRESH: &str = "official_account_refresh";
pub(crate) const OP_DISCONNECT: &str = "official_account_disconnect";
pub(crate) const OP_DELETE: &str = "official_account_delete";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartAuthorizationInput {
    pub mode: AuthorizationMode,
    pub account_id: Option<String>,
    pub workspace_label: Option<String>,
    pub codex_executable: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationMode {
    Browser,
    DeviceCode,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationStarted {
    pub attempt_id: String,
    pub account_id: String,
    pub credential_revision_id: String,
    pub state: AuthorizationState,
    // This prompt is returned to the caller only and is never written to SQLite.
    pub prompt: AuthorizationPrompt,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AuthorizationPrompt {
    Browser {
        auth_url: String,
    },
    DeviceCode {
        verification_url: String,
        user_code: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationState {
    Authorizing,
    AwaitingUser,
    Verifying,
    Authorized,
    Cancelled,
    Failed,
    Interrupted,
}

impl AuthorizationState {
    pub(crate) fn from_db(value: &str) -> Self {
        match value {
            "authorizing" => Self::Authorizing,
            "awaiting_user" => Self::AwaitingUser,
            "verifying" => Self::Verifying,
            "authorized" => Self::Authorized,
            "cancelled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            _ => Self::Failed,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationStatus {
    pub attempt_id: String,
    pub account_id: Option<String>,
    pub credential_revision_id: Option<String>,
    pub state: AuthorizationState,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountActionInput {
    pub account_id: String,
    pub codex_executable: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialAccountStatus {
    pub account_id: String,
    pub masked_identity: Option<String>,
    pub workspace_label: Option<String>,
    pub plan_label: Option<String>,
    pub current_credential_revision_id: Option<String>,
    pub auth_state: String,
    pub auth_error_code: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialAccountError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub retryable: bool,
}

impl OfficialAccountError {
    pub(crate) fn from_auth(error: OfficialAuthError) -> Self {
        let code = auth_error_code(&error);
        service_error(code, matches!(error, OfficialAuthError::Timeout { .. }))
    }
}

impl std::fmt::Display for OfficialAccountError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OfficialAccountError {}

impl From<CommandError> for OfficialAccountError {
    fn from(error: CommandError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            resources: error.resources,
            instance_ids: error.instance_ids,
            operation_id: error.operation_id,
            retryable: error.retryable,
        }
    }
}

pub(crate) struct AccountProjection {
    pub(crate) masked_identity: Option<String>,
    pub(crate) plan_label: Option<String>,
}

pub(crate) fn insert_quota_snapshot(
    transaction: &Transaction<'_>,
    account_id: &str,
    revision_id: &str,
    quota: &AccountRateLimits,
    refreshed_at: i64,
) -> Result<(), OfficialAccountError> {
    let primary = quota.rate_limits.primary.as_ref();
    let secondary = quota.rate_limits.secondary.as_ref();
    transaction
        .execute(
            "INSERT INTO codex_quota_snapshots (
                account_id, official_credential_revision_id, source,
                primary_used_percent, primary_window_minutes, primary_resets_at,
                secondary_used_percent, secondary_window_minutes, secondary_resets_at,
                refreshed_at, expires_at, error_code
             ) VALUES (?1, ?2, 'official_app_server', ?3, ?4, ?5, ?6, ?7, ?8,
                       ?9, ?10, NULL)",
            params![
                account_id,
                revision_id,
                primary.map(|window| window.used_percent),
                primary.and_then(|window| window.window_duration_mins),
                primary.and_then(|window| window.resets_at),
                secondary.map(|window| window.used_percent),
                secondary.and_then(|window| window.window_duration_mins),
                secondary.and_then(|window| window.resets_at),
                refreshed_at,
                refreshed_at + QUOTA_CACHE_TTL_SECONDS,
            ],
        )
        .map_err(|_| service_error("database_error", true))?;
    Ok(())
}

pub(crate) fn account_projection(
    snapshot: AccountSnapshot,
) -> Result<AccountProjection, OfficialAccountError> {
    match snapshot.account {
        Some(AccountKind::ChatGpt { email, plan_type }) => Ok(AccountProjection {
            masked_identity: email.as_deref().map(mask_email),
            plan_label: plan_type.and_then(clean_projection),
        }),
        None if snapshot.requires_openai_auth => {
            Err(service_error("official_account_needs_reauth", false))
        }
        _ => Err(service_error("official_account_type_unsupported", false)),
    }
}

fn mask_email(email: &str) -> String {
    let email = email.trim();
    let Some((local, domain)) = email.rsplit_once('@') else {
        return "***".to_owned();
    };
    let first = local.chars().next().unwrap_or('*');
    let last = local
        .chars()
        .nth_back(0)
        .filter(|_| local.chars().count() > 1);
    match last {
        Some(last) => format!("{first}***{last}@{domain}"),
        None => format!("{first}***@{domain}"),
    }
}

fn clean_projection(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 80 || value.chars().any(char::is_control) {
        None
    } else {
        Some(value.to_owned())
    }
}

pub(crate) fn validate_workspace_label(
    value: Option<String>,
) -> Result<Option<String>, OfficialAccountError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > WORKSPACE_LABEL_MAX_BYTES || value.chars().any(char::is_control) {
        return Err(service_error("workspace_label_invalid", false));
    }
    Ok(Some(value.to_owned()))
}

pub(crate) fn required_id(field: &str, value: &str) -> Result<String, OfficialAccountError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 160
        || matches!(value, "." | "..")
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_:.".contains(character))
    {
        return Err(service_error(
            match field {
                "attempt_id" => "authorization_attempt_invalid",
                _ => "official_account_id_invalid",
            },
            false,
        ));
    }
    Ok(value.to_owned())
}

pub(crate) fn required_path(field: &str, value: &str) -> Result<PathBuf, OfficialAccountError> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(service_error(
            if field == "codex_executable" {
                "codex_executable_invalid"
            } else {
                "path_invalid"
            },
            false,
        ));
    }
    Ok(PathBuf::from(value))
}

pub(crate) fn prepare_managed_home(path: &Path) -> Result<(), OfficialAccountError> {
    fs::create_dir_all(path).map_err(|_| service_error("managed_home_io_failed", true))?;
    set_directory_permissions(path)?;
    let config_path = path.join("config.toml");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut config = options
        .open(config_path)
        .map_err(|_| service_error("managed_home_io_failed", true))?;
    config
        .write_all(b"cli_auth_credentials_store = \"file\"\nforced_login_method = \"chatgpt\"\n")
        .map_err(|_| service_error("managed_home_io_failed", true))?;
    config
        .sync_all()
        .map_err(|_| service_error("managed_home_io_failed", true))?;
    Ok(())
}

pub(crate) fn managed_home_matches(
    root: &Path,
    account_id: &str,
    revision_id: &str,
    actual: &Path,
) -> bool {
    root.join(account_id).join(revision_id) == actual
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> Result<(), OfficialAccountError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| service_error("managed_home_io_failed", true))
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) -> Result<(), OfficialAccountError> {
    Ok(())
}

pub(crate) fn authorization_resources(
    account_id: &str,
    old_revision_id: Option<&str>,
    new_revision_id: &str,
) -> Vec<ResourceRef> {
    let mut resources = vec![ResourceRef::logical(
        ResourceKind::OfficialAccount,
        account_id,
    )];
    if let Some(revision_id) = old_revision_id {
        resources.push(ResourceRef::revision(
            ResourceKind::OfficialCredentialRevision,
            revision_id,
            revision_id,
        ));
    }
    resources.push(ResourceRef::revision(
        ResourceKind::OfficialCredentialRevision,
        new_revision_id,
        new_revision_id,
    ));
    resources
}

pub(crate) fn current_resources(account_id: &str, revision_id: &str) -> Vec<ResourceRef> {
    vec![
        ResourceRef::logical(ResourceKind::OfficialAccount, account_id),
        ResourceRef::revision(
            ResourceKind::OfficialCredentialRevision,
            revision_id,
            revision_id,
        ),
    ]
}

pub(crate) fn auth_error_code(error: &OfficialAuthError) -> &'static str {
    match error {
        OfficialAuthError::InvalidExecutable => "codex_executable_invalid",
        OfficialAuthError::InvalidCodexHome => "managed_home_invalid",
        OfficialAuthError::ProtocolIncompatible { .. }
        | OfficialAuthError::ProtocolMalformed
        | OfficialAuthError::UnexpectedResponseId
        | OfficialAuthError::UnexpectedLoginResponse => "official_auth_protocol_incompatible",
        OfficialAuthError::Rpc { .. } => "official_auth_rejected",
        OfficialAuthError::Timeout { .. } => "official_auth_timeout",
        OfficialAuthError::SessionPoisoned | OfficialAuthError::TransportClosed => {
            "official_auth_session_closed"
        }
        OfficialAuthError::Io { .. } | OfficialAuthError::CommandFailed { .. } => {
            "official_auth_unavailable"
        }
    }
}

pub(crate) fn service_error(code: &str, retryable: bool) -> OfficialAccountError {
    let message = match code {
        "configuration_in_use" => "该账户正被运行中的 Codex 使用。",
        "resource_operation_in_progress" => "该账户已有操作正在进行。",
        "official_account_not_found" => "找不到指定的官方账户。",
        "authorization_attempt_not_found" | "authorization_attempt_not_active" => {
            "找不到活动的授权尝试。"
        }
        "official_account_needs_reauth" => "官方账户需要重新授权。",
        "official_account_type_unsupported" => "Codex 返回了不支持的账户类型。",
        "codex_executable_invalid" => "Codex 可执行文件无效。",
        "official_auth_protocol_incompatible" => "Codex 官方认证协议不兼容。",
        "official_auth_timeout" => "Codex 官方认证操作超时。",
        "official_auth_rejected" => "Codex 官方认证操作被拒绝。",
        "official_auth_region_unsupported" => {
            "当前网络区域不支持官方 Codex 授权，请切换到受支持的网络后重试。"
        }
        "official_auth_session_closed" => "Codex 官方认证会话已关闭。",
        "workspace_label_invalid" => "工作区标签无效。",
        "managed_home_io_failed" | "managed_home_invalid" => "无法准备隔离认证目录。",
        "managed_home_cleanup_failed" => "官方账户已删除，但仍有受管理认证目录需要重试清理。",
        "database_error" => "无法更新官方账户数据库。",
        _ => "Codex 官方账户操作失败。",
    };
    OfficialAccountError {
        code: code.to_owned(),
        message: message.to_owned(),
        resources: Vec::new(),
        instance_ids: Vec::new(),
        operation_id: None,
        retryable,
    }
}

pub(crate) fn login_completion_error_code(completed: &LoginCompleted) -> &'static str {
    let Some(error) = completed.transient_error.as_deref() else {
        return "official_login_failed";
    };
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("unsupported_country_region_territory")
        || normalized.contains("country, region, or territory not supported")
    {
        "official_auth_region_unsupported"
    } else {
        "official_login_failed"
    }
}

pub(crate) fn now_seconds() -> i64 {
    crate::support::unix_now_seconds()
}
