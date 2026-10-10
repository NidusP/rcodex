//! 官方账户服务：授权、刷新、断开和删除。
//!
//! 不打开官方 OAuth 发布门，不把官方 Authorization 发给第三方。
//! 由 commands/official 调用；现有账户数据仍保存在本机 SQLite。

mod account;
mod authorize;
mod driver;
mod models;
mod service;

#[cfg(test)]
mod tests;

pub use models::{
    AccountActionInput, AuthorizationMode, AuthorizationPrompt, AuthorizationStarted,
    AuthorizationStatus, OfficialAccountError, OfficialAccountStatus, StartAuthorizationInput,
};
pub use service::OfficialAccountService;

#[cfg(test)]
pub(crate) use driver::{AuthDriver, AuthSession, ProbedAuth};
#[cfg(test)]
pub(crate) use models::login_completion_error_code;
#[cfg(test)]
pub use models::AuthorizationState;
