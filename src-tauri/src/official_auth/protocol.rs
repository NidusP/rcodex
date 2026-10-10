//! 官方认证协议类型：JSON-RPC 结果、通知和 LoginStart。
//!
//! 不启动进程。由 adapter / session / transport 共用。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt, io, path::PathBuf};
use url::Url;

#[derive(Debug)]
pub enum OfficialAuthError {
    Io {
        operation: &'static str,
        source: io::Error,
    },
    CommandFailed {
        operation: &'static str,
        status: Option<i32>,
    },
    InvalidExecutable,
    InvalidCodexHome,
    ProtocolIncompatible {
        reason: String,
    },
    ProtocolMalformed,
    Rpc {
        code: Option<i64>,
    },
    Timeout {
        operation: &'static str,
    },
    TransportClosed,
    SessionPoisoned,
    UnexpectedResponseId,
    UnexpectedLoginResponse,
}

impl fmt::Display for OfficialAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::CommandFailed { operation, status } => {
                write!(formatter, "{operation} failed with status {status:?}")
            }
            Self::InvalidExecutable => write!(formatter, "invalid Codex executable"),
            Self::InvalidCodexHome => write!(formatter, "invalid managed CODEX_HOME"),
            Self::ProtocolIncompatible { reason } => {
                write!(
                    formatter,
                    "incompatible Codex app-server protocol: {reason}"
                )
            }
            Self::ProtocolMalformed => write!(formatter, "malformed Codex app-server message"),
            Self::Rpc { code } => write!(formatter, "Codex app-server RPC failed ({code:?})"),
            Self::Timeout { operation } => write!(formatter, "{operation} timed out"),
            Self::TransportClosed => write!(formatter, "Codex app-server transport closed"),
            Self::SessionPoisoned => write!(formatter, "Codex app-server session is unusable"),
            Self::UnexpectedResponseId => write!(formatter, "unexpected app-server response id"),
            Self::UnexpectedLoginResponse => write!(formatter, "unexpected login response"),
        }
    }
}

impl std::error::Error for OfficialAuthError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub(crate) fn io_error(operation: &'static str, source: io::Error) -> OfficialAuthError {
    OfficialAuthError::Io { operation, source }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub title: Option<String>,
    pub version: String,
}

impl ClientInfo {
    pub fn new(
        name: impl Into<String>,
        title: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            title: Some(title.into()),
            version: version.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountKind {
    ChatGpt {
        email: Option<String>,
        plan_type: Option<String>,
    },
    ApiKey,
    AmazonBedrock,
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountSnapshot {
    pub account: Option<AccountKind>,
    pub requires_openai_auth: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserLoginOptions {
    pub use_hosted_login_success_page: bool,
    pub app_brand: LoginAppBrand,
}

impl Default for BrowserLoginOptions {
    fn default() -> Self {
        Self {
            use_hosted_login_success_page: true,
            app_brand: LoginAppBrand::Codex,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginAppBrand {
    Codex,
}

impl LoginAppBrand {
    pub(crate) fn as_wire_value(self) -> &'static str {
        match self {
            Self::Codex => "codex",
        }
    }
}

// These values are transient UX material. The adapter deliberately does not
// implement Serialize so callers cannot accidentally persist them by default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginStart {
    Browser {
        login_id: String,
        auth_url: Url,
    },
    DeviceCode {
        login_id: String,
        verification_url: Url,
        user_code: String,
    },
}

impl LoginStart {
    pub fn login_id(&self) -> &str {
        match self {
            Self::Browser { login_id, .. } | Self::DeviceCode { login_id, .. } => login_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginCompleted {
    pub login_id: Option<String>,
    pub success: bool,
    pub transient_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountUpdated {
    pub auth_mode: Option<String>,
    pub plan_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthNotification {
    LoginCompleted(LoginCompleted),
    AccountUpdated(AccountUpdated),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    pub used_percent: i64,
    #[serde(default)]
    pub window_duration_mins: Option<i64>,
    #[serde(default)]
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSnapshot {
    #[serde(default)]
    pub limit_id: Option<String>,
    #[serde(default)]
    pub limit_name: Option<String>,
    #[serde(default)]
    pub primary: Option<RateLimitWindow>,
    #[serde(default)]
    pub secondary: Option<RateLimitWindow>,
    #[serde(default)]
    pub rate_limit_reached_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountRateLimits {
    pub rate_limits: RateLimitSnapshot,
    pub by_limit_id: BTreeMap<String, RateLimitSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelLoginStatus {
    Canceled,
    NotFound,
}

#[derive(Debug)]
pub(crate) enum Incoming {
    Response {
        id: u64,
        result: Option<Value>,
        has_error: bool,
        error_code: Option<i64>,
    },
    Notification(AuthNotification),
    Ignored,
}

pub(crate) fn parse_notification(
    method: &str,
    params: Value,
) -> Result<Incoming, OfficialAuthError> {
    match method {
        "account/login/completed" => {
            let notification: LoginCompletedWire =
                serde_json::from_value(params).map_err(|_| OfficialAuthError::ProtocolMalformed)?;
            Ok(Incoming::Notification(AuthNotification::LoginCompleted(
                LoginCompleted {
                    login_id: notification.login_id,
                    success: notification.success,
                    transient_error: notification.error,
                },
            )))
        }
        "account/updated" => {
            let notification: AccountUpdatedWire =
                serde_json::from_value(params).map_err(|_| OfficialAuthError::ProtocolMalformed)?;
            Ok(Incoming::Notification(AuthNotification::AccountUpdated(
                AccountUpdated {
                    auth_mode: notification.auth_mode,
                    plan_type: notification.plan_type,
                },
            )))
        }
        _ => Ok(Incoming::Ignored),
    }
}

pub(crate) fn parse_https_url(value: &str) -> Result<Url, OfficialAuthError> {
    let url = Url::parse(value).map_err(|_| OfficialAuthError::UnexpectedLoginResponse)?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(OfficialAuthError::UnexpectedLoginResponse);
    }
    Ok(url)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitializeResponse {
    pub(crate) codex_home: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountReadWire {
    #[serde(default)]
    account: Option<Value>,
    requires_openai_auth: bool,
}

pub(crate) fn parse_account_snapshot(
    wire: AccountReadWire,
) -> Result<AccountSnapshot, OfficialAuthError> {
    let account = match wire.account {
        None | Some(Value::Null) => None,
        Some(Value::Object(object)) => {
            let kind = object
                .get("type")
                .and_then(Value::as_str)
                .ok_or(OfficialAuthError::ProtocolMalformed)?;
            Some(match kind {
                "chatgpt" => AccountKind::ChatGpt {
                    email: object
                        .get("email")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    plan_type: object
                        .get("planType")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
                "apiKey" => AccountKind::ApiKey,
                "amazonBedrock" => AccountKind::AmazonBedrock,
                other => AccountKind::Unknown(other.to_owned()),
            })
        }
        Some(_) => return Err(OfficialAuthError::ProtocolMalformed),
    };
    Ok(AccountSnapshot {
        account,
        requires_openai_auth: wire.requires_openai_auth,
    })
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(crate) enum LoginStartWire {
    #[serde(rename = "chatgpt")]
    ChatGpt {
        #[serde(rename = "loginId")]
        login_id: String,
        #[serde(rename = "authUrl")]
        auth_url: String,
    },
    #[serde(rename = "chatgptDeviceCode")]
    ChatGptDeviceCode {
        #[serde(rename = "loginId")]
        login_id: String,
        #[serde(rename = "verificationUrl")]
        verification_url: String,
        #[serde(rename = "userCode")]
        user_code: String,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Deserialize)]
pub(crate) struct CancelLoginWire {
    pub(crate) status: String,
}

#[derive(Deserialize)]
pub(crate) struct EmptyResponse {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginCompletedWire {
    #[serde(default)]
    login_id: Option<String>,
    success: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountUpdatedWire {
    #[serde(default)]
    auth_mode: Option<String>,
    #[serde(default)]
    plan_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountRateLimitsWire {
    pub(crate) rate_limits: RateLimitSnapshot,
    #[serde(default)]
    pub(crate) rate_limits_by_limit_id: Option<BTreeMap<String, RateLimitSnapshot>>,
}
