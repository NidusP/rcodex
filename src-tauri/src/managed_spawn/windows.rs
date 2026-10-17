//! Windows：控制面拉起 `codex-launcher.exe`。
//!
//! 受管理 App 直接 spawn，不写 .command。CLI 用 Windows Terminal 或 cmd start。
//! 商店包激活属于 launcher，不在这里。

use std::path::Path;

#[cfg(target_os = "windows")]
use super::args::{launcher_argv, LauncherArgSpec};
#[cfg(target_os = "windows")]
use crate::launcher_protocol::{append_startup_log, LaunchTarget};

#[cfg(not(target_os = "windows"))]
const WINDOWS_LAUNCH_UNAVAILABLE: &str =
    "当前版本仅在 macOS 提供受管理终端启动。Windows 计划优先使用 Windows Terminal（wt.exe），没有时再打开 cmd.exe。";

pub(super) fn launch(
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
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (
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
        );
        return Err(WINDOWS_LAUNCH_UNAVAILABLE.to_string());
    }
    #[cfg(target_os = "windows")]
    {
        if LaunchTarget::wire_is_managed_app(launch_target) {
            launch_managed_app(
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
        } else {
            launch_managed_cli(
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
    }
}

#[cfg(target_os = "windows")]
struct PreparedLauncher {
    helper: std::path::PathBuf,
    args: Vec<String>,
    secret_file: Option<std::path::PathBuf>,
    log_file: std::path::PathBuf,
}

#[cfg(target_os = "windows")]
fn prepare_launcher(
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
) -> Result<PreparedLauncher, String> {
    use std::fs;

    let log_file = status_file.with_file_name("startup.log");
    let executable =
        std::env::current_exe().map_err(|error| format!("无法定位 R Codex 应用：{error}"))?;
    let helper = executable
        .parent()
        .ok_or_else(|| "无法定位 R Codex 启动器目录。".to_string())?;
    let Some(helper) = crate::support::first_existing_sidecar(helper, "codex-launcher") else {
        let _ = append_startup_log(&log_file, "launcher_binary_missing");
        return Err("未找到受管理 Codex 启动器；请重新构建 R Codex 应用。".to_string());
    };
    let secret_file = if launch_secret.is_some() {
        Some(status_file.with_file_name("launch-secret.txt"))
    } else {
        None
    };
    if let (Some(path), Some(secret)) = (secret_file.as_deref(), launch_secret) {
        fs::write(path, secret).map_err(|error| format!("无法写入一次性启动凭据：{error}"))?;
    }
    let args = launcher_argv(&LauncherArgSpec {
        launch_target,
        credential_kind,
        instance_id,
        launch_nonce,
        status_file,
        log_file: &log_file,
        codex_home,
        secret_ref,
        instance_key_ref,
        profile,
        model,
        working_directory,
        secret_file: secret_file.as_deref(),
        electron_user_data_dir,
        sqlite_home,
    });
    Ok(PreparedLauncher {
        helper,
        args,
        secret_file,
        log_file,
    })
}

#[cfg(target_os = "windows")]
fn launch_managed_app(
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
    use std::fs;

    let _ = append_startup_log(
        &status_file.with_file_name("startup.log"),
        "windows_app_launch_requested",
    );
    let prepared = prepare_launcher(
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
    )?;
    crate::support::command_without_console(&prepared.helper)
        .args(&prepared.args)
        .spawn()
        .map_err(|error| {
            if let Some(path) = prepared.secret_file.as_deref() {
                let _ = fs::remove_file(path);
            }
            let _ = append_startup_log(
                &prepared.log_file,
                &format!("app_spawn_failed target={launch_target} error={error}"),
            );
            format!("无法启动 Codex App：{error}")
        })?;
    let _ = append_startup_log(
        &prepared.log_file,
        &format!("app_spawn_dispatched target={launch_target}"),
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn launch_managed_cli(
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
    use std::{fs, process::Command};

    let _ = append_startup_log(
        &status_file.with_file_name("startup.log"),
        "windows_cli_launch_requested",
    );
    let prepared = prepare_launcher(
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
    )?;
    let mut specs = Vec::new();
    specs.push(super::windows_cli::windows_cli_terminal_spec(
        &prepared.helper,
        &prepared.args,
        working_directory,
        find_windows_terminal().as_deref(),
    ));
    if specs[0].kind != super::windows_cli::WindowsCliTerminalKind::CmdStart {
        specs.push(super::windows_cli::windows_cli_terminal_spec(
            &prepared.helper,
            &prepared.args,
            working_directory,
            None,
        ));
    }
    specs.push(super::windows_cli::windows_cli_direct_spec(
        &prepared.helper,
        &prepared.args,
    ));
    // CLI 必须是可见终端，不能套 CREATE_NO_WINDOW。
    let mut last_error = None;
    for spec in specs {
        match Command::new(&spec.program).args(&spec.args).spawn() {
            Ok(_) => {
                let _ = append_startup_log(
                    &prepared.log_file,
                    &format!(
                        "cli_spawn_dispatched target={launch_target} terminal={}",
                        spec.program
                    ),
                );
                return Ok(());
            }
            Err(error) => {
                let _ = append_startup_log(
                    &prepared.log_file,
                    &format!(
                        "cli_spawn_failed target={launch_target} terminal={} error={error}",
                        spec.program
                    ),
                );
                last_error = Some(error);
            }
        }
    }
    if let Some(path) = prepared.secret_file.as_deref() {
        let _ = fs::remove_file(path);
    }
    Err(format!(
        "无法启动 Codex CLI：{}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "没有可用的终端。".to_string())
    ))
}

#[cfg(target_os = "windows")]
fn find_windows_terminal() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(path_value) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path_value) {
            let candidate = directory.join("wt.exe");
            if is_usable_windows_terminal(&candidate) {
                return Some(candidate);
            }
        }
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|local| local.join(r"Microsoft\WindowsApps\wt.exe"))
        .filter(|path| is_usable_windows_terminal(path))
}

#[cfg(target_os = "windows")]
fn is_usable_windows_terminal(path: &std::path::Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}
