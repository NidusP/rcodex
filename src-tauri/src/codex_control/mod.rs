//! 受管理实例控制面：快照、预设、官方启动准备和生命周期。
//!
//! 不转发模型请求、不投影隔离 config.toml、不读取用户默认 ~/.codex。
//! 由 commands/instances、commands/launch 和首页快照调用。

mod helpers;
mod launch;
mod lifecycle;
mod models;
mod presets;
mod snapshot;
mod store;

#[cfg(test)]
mod tests;

pub use helpers::managed_app_compatibility_enabled;
pub use models::{
    CodexControlSnapshot, CodexInstanceSummary, CreateLaunchPresetInput, CredentialKind, EntryKind,
    InstanceState, LaunchCodexInput, LaunchPresetSummary, RenameLaunchPresetInput,
    UpdateLaunchPresetInput,
};
pub use store::CodexControlStore;

pub(crate) const ACTIVE_INSTANCE_STATES: &str =
    "'launching','running','stopping','orphaned','degraded'";

#[cfg(test)]
pub(crate) use helpers::{resolve_workspace, sanitize_startup_log};
#[cfg(test)]
pub(crate) use presets::validate_preset_input;
