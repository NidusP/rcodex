//! 控制面如何拉起 `codex-launcher`。
//!
//! 不准备隔离目录、不写 RouteSet。macOS 用临时脚本；Windows App 直接 spawn，CLI 走终端。
//! 由 `commands/instances` 在快照落盘后调用。

mod args;

use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod windows;
#[cfg(any(test, target_os = "windows"))]
mod windows_cli;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(target_os = "macos"))]
use windows as platform;

/// 把已准备好的受管理启动交给平台调度器。
pub fn launch_codex_in_terminal(
    instance_id: &str,
    launch_nonce: &str,
    status_file: &Path,
    codex_home: &Path,
    credential_kind: &str,
    secret_ref: Option<&str>,
    instance_key_ref: Option<&str>,
    profile: Option<&str>,
    model: Option<&str>,
    working_directory: Option<&str>,
    launch_secret: Option<&str>,
    launch_target: &str,
    electron_user_data_dir: Option<&Path>,
    sqlite_home: Option<&Path>,
) -> Result<(), String> {
    platform::launch(
        instance_id,
        launch_nonce,
        status_file,
        codex_home,
        credential_kind,
        secret_ref,
        instance_key_ref,
        profile,
        model,
        working_directory,
        launch_secret,
        launch_target,
        electron_user_data_dir,
        sqlite_home,
    )
}
