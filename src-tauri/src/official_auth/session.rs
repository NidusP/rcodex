//! CodexAppServerSession：login、logout 和 rate limits 的 JSON-RPC 会话。
//!
//! 不探测 schema、不持久化凭据。由 Adapter.start_session 创建。

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use super::protocol::{
    parse_account_snapshot, parse_https_url, parse_notification, AccountRateLimits,
    AccountRateLimitsWire, AccountReadWire, AccountSnapshot, AuthNotification, BrowserLoginOptions,
    CancelLoginStatus, CancelLoginWire, ClientInfo, EmptyResponse, Incoming, InitializeResponse,
    LoginCompleted, LoginStart, LoginStartWire, OfficialAuthError,
};
use super::transport::JsonLineTransport;

const FORBIDDEN_AUTH_METHODS: &[&str] = &[
    "account/chatgptAuthTokens/refresh",
    "account/login/start:chatgptAuthTokens",
];

pub struct CodexAppServerSession {
    transport: Box<dyn JsonLineTransport>,
    codex_home: PathBuf,
    request_timeout: Duration,
    next_request_id: u64,
    notifications: VecDeque<AuthNotification>,
    active_login_id: Option<String>,
    supports_rate_limits: bool,
    poisoned: bool,
}

impl CodexAppServerSession {
    pub(crate) fn initialize(
        transport: Box<dyn JsonLineTransport>,
        codex_home: PathBuf,
        client: ClientInfo,
        request_timeout: Duration,
        supports_rate_limits: bool,
    ) -> Result<Self, OfficialAuthError> {
        let mut session = Self {
            transport,
            codex_home,
            request_timeout,
            next_request_id: 1,
            notifications: VecDeque::new(),
            active_login_id: None,
            supports_rate_limits,
            poisoned: false,
        };

        let response: InitializeResponse = session.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": client.name,
                    "title": client.title,
                    "version": client.version,
                }
            }),
            "initialize",
        )?;

        let reported_home = fs::canonicalize(&response.codex_home)
            .map_err(|_| OfficialAuthError::InvalidCodexHome)?;
        if reported_home != session.codex_home {
            session.poisoned = true;
            return Err(OfficialAuthError::InvalidCodexHome);
        }

        session
            .transport
            .send(&json!({ "method": "initialized" }))?;
        Ok(session)
    }

    pub fn read_account(&mut self) -> Result<AccountSnapshot, OfficialAuthError> {
        self.read_account_with_refresh(false)
    }

    /// Read the current account and optionally ask Codex to refresh its
    /// ChatGPT credentials before returning the snapshot.
    pub fn read_account_with_refresh(
        &mut self,
        refresh_token: bool,
    ) -> Result<AccountSnapshot, OfficialAuthError> {
        let response: AccountReadWire = self.request(
            "account/read",
            json!({ "refreshToken": refresh_token }),
            "account_read",
        )?;
        parse_account_snapshot(response)
    }

    pub fn start_browser_login(
        &mut self,
        options: BrowserLoginOptions,
    ) -> Result<LoginStart, OfficialAuthError> {
        if self.active_login_id.is_some() {
            return Err(OfficialAuthError::ProtocolIncompatible {
                reason: "a login attempt is already active".to_owned(),
            });
        }
        let response: LoginStartWire = self.request(
            "account/login/start",
            json!({
                "type": "chatgpt",
                "useHostedLoginSuccessPage": options.use_hosted_login_success_page,
                "appBrand": options.app_brand.as_wire_value(),
            }),
            "login_start",
        )?;
        let result = match response {
            LoginStartWire::ChatGpt { login_id, auth_url } => LoginStart::Browser {
                login_id,
                auth_url: parse_https_url(&auth_url)?,
            },
            _ => return Err(OfficialAuthError::UnexpectedLoginResponse),
        };
        self.active_login_id = Some(result.login_id().to_owned());
        Ok(result)
    }

    pub fn start_device_code_login(&mut self) -> Result<LoginStart, OfficialAuthError> {
        if self.active_login_id.is_some() {
            return Err(OfficialAuthError::ProtocolIncompatible {
                reason: "a login attempt is already active".to_owned(),
            });
        }
        let response: LoginStartWire = self.request(
            "account/login/start",
            json!({ "type": "chatgptDeviceCode" }),
            "device_code_login_start",
        )?;
        let result = match response {
            LoginStartWire::ChatGptDeviceCode {
                login_id,
                verification_url,
                user_code,
            } => LoginStart::DeviceCode {
                login_id,
                verification_url: parse_https_url(&verification_url)?,
                user_code,
            },
            _ => return Err(OfficialAuthError::UnexpectedLoginResponse),
        };
        self.active_login_id = Some(result.login_id().to_owned());
        Ok(result)
    }

    #[cfg(test)]
    pub fn wait_for_login_completion(
        &mut self,
        login_id: &str,
        timeout: Duration,
    ) -> Result<LoginCompleted, OfficialAuthError> {
        if self.active_login_id.as_deref() != Some(login_id) {
            return Err(OfficialAuthError::ProtocolIncompatible {
                reason: "login id is not active in this session".to_owned(),
            });
        }

        let deadline = Instant::now() + timeout;
        loop {
            if let Some(index) = self.notifications.iter().position(|notification| {
                matches!(
                    notification,
                    AuthNotification::LoginCompleted(completed)
                        if completed.login_id.as_deref().map_or(true, |id| id == login_id)
                )
            }) {
                let AuthNotification::LoginCompleted(completed) =
                    self.notifications.remove(index).expect("index exists")
                else {
                    unreachable!();
                };
                self.active_login_id = None;
                return Ok(completed);
            }

            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| self.poison_timeout("login_completion"))?;
            match self.receive_message(remaining)? {
                Incoming::Notification(notification) => self.notifications.push_back(notification),
                Incoming::Ignored => {}
                Incoming::Response { .. } => {
                    self.poisoned = true;
                    return Err(OfficialAuthError::UnexpectedResponseId);
                }
            }
        }
    }

    pub fn poll_login_completion(
        &mut self,
        login_id: &str,
        timeout: Duration,
    ) -> Result<Option<LoginCompleted>, OfficialAuthError> {
        if self.poisoned {
            return Err(OfficialAuthError::SessionPoisoned);
        }
        if self.active_login_id.as_deref() != Some(login_id) {
            return Err(OfficialAuthError::ProtocolIncompatible {
                reason: "login id is not active in this session".to_owned(),
            });
        }

        let deadline = Instant::now() + timeout;
        loop {
            if let Some(completed) = self.take_login_completion(login_id) {
                self.active_login_id = None;
                return Ok(Some(completed));
            }

            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(None);
            };
            let Some(incoming) = self.poll_message(remaining)? else {
                return Ok(None);
            };
            match incoming {
                Incoming::Notification(notification) => self.notifications.push_back(notification),
                Incoming::Ignored => {}
                Incoming::Response { .. } => {
                    self.poisoned = true;
                    return Err(OfficialAuthError::UnexpectedResponseId);
                }
            }
        }
    }

    #[cfg(test)]
    pub fn next_notification(
        &mut self,
        timeout: Duration,
    ) -> Result<AuthNotification, OfficialAuthError> {
        if let Some(notification) = self.notifications.pop_front() {
            return Ok(notification);
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| self.poison_timeout("auth_notification"))?;
            match self.receive_message(remaining)? {
                Incoming::Notification(notification) => return Ok(notification),
                Incoming::Ignored => {}
                Incoming::Response { .. } => {
                    self.poisoned = true;
                    return Err(OfficialAuthError::UnexpectedResponseId);
                }
            }
        }
    }

    pub fn cancel_login(&mut self, login_id: &str) -> Result<CancelLoginStatus, OfficialAuthError> {
        if self.active_login_id.as_deref() != Some(login_id) {
            return Err(OfficialAuthError::ProtocolIncompatible {
                reason: "login id is not active in this session".to_owned(),
            });
        }
        let response: CancelLoginWire = self.request(
            "account/login/cancel",
            json!({ "loginId": login_id }),
            "login_cancel",
        )?;
        self.active_login_id = None;
        Ok(match response.status.as_str() {
            "canceled" => CancelLoginStatus::Canceled,
            "notFound" => CancelLoginStatus::NotFound,
            _ => {
                return Err(OfficialAuthError::ProtocolMalformed);
            }
        })
    }

    pub fn logout(&mut self) -> Result<(), OfficialAuthError> {
        let _: EmptyResponse = self.request("account/logout", Value::Null, "account_logout")?;
        self.active_login_id = None;
        let deadline = Instant::now() + self.request_timeout;
        loop {
            if self.read_account()?.account.is_none() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(self.poison_timeout("account_logout_confirmation"));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn read_rate_limits(&mut self) -> Result<Option<AccountRateLimits>, OfficialAuthError> {
        if !self.supports_rate_limits {
            return Ok(None);
        }
        let response: AccountRateLimitsWire = self.request(
            "account/rateLimits/read",
            Value::Null,
            "account_rate_limits",
        )?;
        Ok(Some(AccountRateLimits {
            rate_limits: response.rate_limits,
            by_limit_id: response.rate_limits_by_limit_id.unwrap_or_default(),
        }))
    }

    fn request<T: DeserializeOwned>(
        &mut self,
        method: &'static str,
        params: Value,
        operation: &'static str,
    ) -> Result<T, OfficialAuthError> {
        if self.poisoned {
            return Err(OfficialAuthError::SessionPoisoned);
        }
        if FORBIDDEN_AUTH_METHODS.contains(&method) {
            self.poisoned = true;
            return Err(OfficialAuthError::ProtocolIncompatible {
                reason: "external token authentication is forbidden".to_owned(),
            });
        }

        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        let mut request = json!({ "id": id, "method": method });
        if !params.is_null() {
            request["params"] = params;
        }
        self.transport.send(&request)?;

        let deadline = Instant::now() + self.request_timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| self.poison_timeout(operation))?;
            match self.receive_message(remaining)? {
                Incoming::Response {
                    id: response_id,
                    result,
                    has_error,
                    error_code,
                } => {
                    if response_id != id {
                        self.poisoned = true;
                        return Err(OfficialAuthError::UnexpectedResponseId);
                    }
                    if has_error {
                        return Err(OfficialAuthError::Rpc { code: error_code });
                    }
                    return serde_json::from_value(result.unwrap_or(Value::Null)).map_err(|_| {
                        self.poisoned = true;
                        OfficialAuthError::ProtocolMalformed
                    });
                }
                Incoming::Notification(notification) => self.notifications.push_back(notification),
                Incoming::Ignored => {}
            }
        }
    }

    fn receive_message(&mut self, timeout: Duration) -> Result<Incoming, OfficialAuthError> {
        let value = match self.transport.receive(timeout) {
            Ok(value) => value,
            Err(OfficialAuthError::Timeout { operation: _ }) => {
                return Err(self.poison_timeout("app_server_response"));
            }
            Err(error) => {
                self.poisoned = true;
                return Err(error);
            }
        };

        self.decode_incoming(value)
    }

    fn poll_message(&mut self, timeout: Duration) -> Result<Option<Incoming>, OfficialAuthError> {
        let value = match self.transport.receive(timeout) {
            Ok(value) => value,
            Err(OfficialAuthError::Timeout { .. }) => return Ok(None),
            Err(error) => {
                self.poisoned = true;
                return Err(error);
            }
        };
        self.decode_incoming(value).map(Some)
    }

    fn decode_incoming(&mut self, value: Value) -> Result<Incoming, OfficialAuthError> {
        if let Some(method) = value.get("method").and_then(Value::as_str) {
            if value.get("id").is_some() {
                let id = value.get("id").cloned().unwrap_or(Value::Null);
                self.transport.send(&json!({
                    "id": id,
                    "error": { "code": -32601, "message": "unsupported server request" }
                }))?;
                if method == "account/chatgptAuthTokens/refresh" {
                    return Ok(Incoming::Ignored);
                }
                return Ok(Incoming::Ignored);
            }
            return parse_notification(method, value.get("params").cloned().unwrap_or(Value::Null));
        }

        let id = value.get("id").and_then(Value::as_u64).ok_or_else(|| {
            self.poisoned = true;
            OfficialAuthError::ProtocolMalformed
        })?;
        let error_code = value
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64);
        let has_error = value.get("error").is_some();
        if value.get("result").is_none() && !has_error {
            self.poisoned = true;
            return Err(OfficialAuthError::ProtocolMalformed);
        }
        Ok(Incoming::Response {
            id,
            result: value.get("result").cloned(),
            has_error,
            error_code,
        })
    }

    fn take_login_completion(&mut self, login_id: &str) -> Option<LoginCompleted> {
        let index = self.notifications.iter().position(|notification| {
            matches!(
                notification,
                AuthNotification::LoginCompleted(completed)
                    if completed.login_id.as_deref().map_or(true, |id| id == login_id)
            )
        })?;
        let AuthNotification::LoginCompleted(completed) = self.notifications.remove(index)? else {
            unreachable!();
        };
        Some(completed)
    }

    fn poison_timeout(&mut self, operation: &'static str) -> OfficialAuthError {
        self.poisoned = true;
        OfficialAuthError::Timeout { operation }
    }
}

impl Drop for CodexAppServerSession {
    fn drop(&mut self) {
        self.transport.shutdown();
    }
}
