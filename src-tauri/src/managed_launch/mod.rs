//! 受管理启动方案：工作区写入、启动准备、隔离 home。
//!
//! 不启动进程、不转发模型请求、不改用户默认 ~/.codex。
//! 由 `commands/launch`、`commands/instances` 和 Gateway 绑定装配调用。

mod binding;
mod home_layout;
mod models;
mod persistence;
mod prepare;
mod preset_validation;
mod storage;
mod store;
mod validation;
mod workspace;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_home;
#[cfg(test)]
mod tests_prepare;
#[cfg(test)]
mod tests_workspace;

pub use models::{
    CreateManagedLaunchPresetInput, ManagedLaunchStore, ManagedLaunchTransport,
    ManagedModelSelectionInput, ManagedPresetCompositionSummary, ManagedPresetSaved,
};

#[cfg(test)]
pub use models::PreparedManagedLaunch;
