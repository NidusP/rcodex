//! Tauri IPC 适配层。
//!
//! 按领域把 command 从 lib.rs 拆出：只做参数、State 和错误映射。
//! 不拥有事务、sidecar 生命周期或凭据存储；那些仍在对应 Store。
//! 控制面 UI 通过 invoke 调用这里导出的命令。

mod archive;
mod diagnostics;
mod gateway;
mod instances;
mod launch;
mod official;
mod providers;
mod system;

pub(crate) use archive::*;
pub(crate) use diagnostics::*;
pub(crate) use gateway::*;
pub(crate) use instances::*;
pub(crate) use launch::*;
pub(crate) use official::*;
pub(crate) use providers::*;
pub(crate) use system::*;

/// A launch must not prepare or reopen the target while an import owns its files.
pub(crate) static CODEX_LAUNCH_IMPORT_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());
