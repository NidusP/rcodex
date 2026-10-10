//! 本机回环 Gateway：sidecar 生命周期、绑定、协议转换和转发。
//!
//! 不读用户默认认证，不把官方 Authorization 发给第三方，不在普通日志里写正文或 Key。
//! 数据面由 `bin/codex-gateway` 调用 `run_sidecar`；控制面由 `ManagedGatewayClient` 管理。

mod compaction;
mod diagnostics;
mod errors;
mod forward;
mod helpers;
mod http;
mod lifecycle;
mod protocol;
mod runtime;
mod runtime_types;
mod sidecar;
mod sse;
mod stream;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_codex;
#[cfg(test)]
mod tests_compaction;
#[cfg(test)]
mod tests_forward;
#[cfg(test)]
mod tests_runtime;

pub use lifecycle::run_sidecar;
pub use runtime_types::{GatewayBinding, ManagedGatewayCredential, ManagedGatewayStatus};
pub use sidecar::ManagedGatewayClient;

pub(super) const LOOPBACK_HOST: &str = "127.0.0.1";
/// Stable loopback data-plane port owned by the R Codex sidecar.
pub const GATEWAY_PORT: u16 = 43123;
pub(super) const SIDECAR_PROTOCOL_VERSION: u32 = 2;
pub(super) const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
// 解析预算和档案预算独立：记录关闭后，恶意/异常上游仍不能无限占用内存。
pub(super) const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
pub(super) const MAX_SSE_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub(super) const SUBTASK_FAILURE_RECOVERY_NOTICE: &str =
    "Delegated subtask failed before producing a usable result. Treat its result as unavailable; continue the parent task yourself if possible.";
