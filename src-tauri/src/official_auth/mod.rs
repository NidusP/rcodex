//! Codex app-server 官方认证适配：探测、会话和 JSON-RPC 传输。
//!
//! 不保存凭据、不转发官方 Authorization 给第三方、不打开产品发布门。
//! 由 official_accounts 的 ProcessAuthDriver 调用。

mod adapter;
mod probe;
mod protocol;
mod session;
mod transport;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use adapter::{CodexAppServerAdapter, CodexAppServerProbe};
#[allow(unused_imports)]
pub use protocol::{
    AccountKind, AccountRateLimits, AccountSnapshot, AccountUpdated, AuthNotification,
    BrowserLoginOptions, CancelLoginStatus, ClientInfo, LoginAppBrand, LoginCompleted, LoginStart,
    OfficialAuthError, RateLimitSnapshot, RateLimitWindow,
};
pub use session::CodexAppServerSession;

#[cfg(test)]
pub(crate) use probe::{validate_auth_schema, ProbeDirectory, SchemaDocuments};
#[cfg(test)]
pub(crate) use transport::{
    isolated_command, read_json_lines, JsonLineTransport, ProcessTransport, TransportEvent,
};
