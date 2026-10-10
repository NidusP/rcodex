//! 官方认证驱动：把 CodexAppServerAdapter 接到账户服务。
//!
//! 不写 SQLite。由 OfficialAccountService 在授权和账户动作里调用。

use std::{path::Path, time::Duration};

use crate::official_auth::{
    AccountRateLimits, AccountSnapshot, BrowserLoginOptions, CancelLoginStatus, ClientInfo,
    CodexAppServerAdapter, CodexAppServerProbe, CodexAppServerSession, LoginCompleted, LoginStart,
    OfficialAuthError,
};

pub(crate) trait AuthDriver: Send + Sync {
    fn probe(&self, executable: &Path) -> Result<Box<dyn ProbedAuth>, OfficialAuthError>;
}

pub(crate) trait ProbedAuth: Send {
    fn metadata(&self) -> &CodexAppServerProbe;
    fn start_session(&self, codex_home: &Path) -> Result<Box<dyn AuthSession>, OfficialAuthError>;
}

pub(crate) trait AuthSession: Send {
    fn start_browser_login(&mut self) -> Result<LoginStart, OfficialAuthError>;
    fn start_device_code_login(&mut self) -> Result<LoginStart, OfficialAuthError>;
    fn poll_login_completion(
        &mut self,
        login_id: &str,
        timeout: Duration,
    ) -> Result<Option<LoginCompleted>, OfficialAuthError>;
    fn cancel_login(&mut self, login_id: &str) -> Result<CancelLoginStatus, OfficialAuthError>;
    fn read_account(&mut self) -> Result<AccountSnapshot, OfficialAuthError>;
    /// Refresh the official credential before reading its account snapshot.
    /// Implementations that cannot force a refresh may use the regular read.
    fn refresh_account(&mut self) -> Result<AccountSnapshot, OfficialAuthError> {
        self.read_account()
    }
    fn read_rate_limits(&mut self) -> Result<Option<AccountRateLimits>, OfficialAuthError>;
    fn logout(&mut self) -> Result<(), OfficialAuthError>;
}

pub(crate) struct ProcessAuthDriver;

impl AuthDriver for ProcessAuthDriver {
    fn probe(&self, executable: &Path) -> Result<Box<dyn ProbedAuth>, OfficialAuthError> {
        Ok(Box::new(ProcessProbedAuth(CodexAppServerAdapter::probe(
            executable,
        )?)))
    }
}

struct ProcessProbedAuth(CodexAppServerAdapter);

impl ProbedAuth for ProcessProbedAuth {
    fn metadata(&self) -> &CodexAppServerProbe {
        self.0.probe_result()
    }

    fn start_session(&self, codex_home: &Path) -> Result<Box<dyn AuthSession>, OfficialAuthError> {
        Ok(Box::new(self.0.start_session(
            codex_home,
            ClientInfo::new("rcodex", "R Codex", env!("CARGO_PKG_VERSION")),
        )?))
    }
}

impl AuthSession for CodexAppServerSession {
    fn start_browser_login(&mut self) -> Result<LoginStart, OfficialAuthError> {
        CodexAppServerSession::start_browser_login(self, BrowserLoginOptions::default())
    }

    fn start_device_code_login(&mut self) -> Result<LoginStart, OfficialAuthError> {
        CodexAppServerSession::start_device_code_login(self)
    }

    fn poll_login_completion(
        &mut self,
        login_id: &str,
        timeout: Duration,
    ) -> Result<Option<LoginCompleted>, OfficialAuthError> {
        CodexAppServerSession::poll_login_completion(self, login_id, timeout)
    }

    fn cancel_login(&mut self, login_id: &str) -> Result<CancelLoginStatus, OfficialAuthError> {
        CodexAppServerSession::cancel_login(self, login_id)
    }

    fn read_account(&mut self) -> Result<AccountSnapshot, OfficialAuthError> {
        CodexAppServerSession::read_account(self)
    }

    fn refresh_account(&mut self) -> Result<AccountSnapshot, OfficialAuthError> {
        CodexAppServerSession::read_account_with_refresh(self, true)
    }

    fn read_rate_limits(&mut self) -> Result<Option<AccountRateLimits>, OfficialAuthError> {
        CodexAppServerSession::read_rate_limits(self)
    }

    fn logout(&mut self) -> Result<(), OfficialAuthError> {
        CodexAppServerSession::logout(self)
    }
}

pub(crate) struct PendingAuthorization {
    pub(crate) attempt_id: String,
    pub(crate) account_id: String,
    pub(crate) revision_id: String,
    pub(crate) operation_id: String,
    pub(crate) login_id: String,
    pub(crate) prompt_url: String,
    pub(crate) session: Box<dyn AuthSession>,
}
