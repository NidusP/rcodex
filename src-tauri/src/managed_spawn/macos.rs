//! macOS：用临时脚本拉起 `codex-launcher`。
//!
//! 受管理 App 直接 `/bin/sh` 执行脚本；CLI 打开 iTerm 或 Terminal。
//! 准入密钥走 FIFO。不解析 Codex.app。

use std::time::Duration;
use zeroize::Zeroize;

use crate::launcher_protocol::append_startup_log;

const TERMINAL_LAUNCHER_PREFIX: &str = "rcodex-";
const TERMINAL_LAUNCHER_SUFFIX: &str = ".command";
const TERMINAL_LAUNCHER_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub(super) fn launch(
    instance_id: &str,
    launch_nonce: &str,
    status_file: &std::path::Path,
    codex_home: &std::path::Path,
    credential_kind: &str,
    secret_ref: Option<&str>,
    instance_key_ref: Option<&str>,
    profile: Option<&str>,
    model: Option<&str>,
    working_directory: Option<&str>,
    launch_secret: Option<&str>,
    launch_target: &str,
    electron_user_data_dir: Option<&std::path::Path>,
    sqlite_home: Option<&std::path::Path>,
) -> Result<(), String> {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};

    let log_file = status_file.with_file_name("startup.log");
    let _ = append_startup_log(&log_file, "terminal_launch_requested");
    cleanup_stale_terminal_launchers();
    let executable =
        std::env::current_exe().map_err(|error| format!("无法定位 R Codex 应用：{error}"))?;
    let helper = executable
        .parent()
        .ok_or_else(|| "无法定位 R Codex 启动器目录。".to_string())?
        .join("codex-launcher");
    if !helper.is_file() {
        let _ = append_startup_log(&log_file, "launcher_binary_missing");
        return Err("未找到受管理 Codex 启动器；请重新构建 R Codex 应用。".to_string());
    }
    let script = std::env::temp_dir().join(format!("rcodex-{}.command", uuid::Uuid::new_v4()));
    let secret_file = if launch_secret.is_some() {
        let path = status_file.with_file_name("launch-secret.fifo");
        create_secret_fifo(&path)?;
        Some(path)
    } else {
        None
    };
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\\"'\\\"'"));
    let mut arguments = vec![
        quote("--target"),
        quote(launch_target),
        quote("--credential-kind"),
        quote(credential_kind),
        quote("--instance-id"),
        quote(instance_id),
        quote("--launch-nonce"),
        quote(launch_nonce),
        quote("--status-file"),
        quote(&status_file.display().to_string()),
        quote("--log-file"),
        quote(&log_file.display().to_string()),
        quote("--codex-home"),
        quote(&codex_home.display().to_string()),
    ];
    if let Some(secret_ref) = secret_ref {
        arguments.push(quote("--secret-ref"));
        arguments.push(quote(secret_ref));
    }
    if let Some(instance_key_ref) = instance_key_ref {
        arguments.push(quote("--instance-key-ref"));
        arguments.push(quote(instance_key_ref));
    }
    if let Some(profile) = profile {
        arguments.push(quote("--profile"));
        arguments.push(quote(profile));
    }
    if let Some(model) = model {
        arguments.push(quote("--model"));
        arguments.push(quote(model));
    }
    if let Some(directory) = working_directory.filter(|value| !value.trim().is_empty()) {
        arguments.push(quote("--working-directory"));
        arguments.push(quote(directory));
    }
    if let Some(secret_file) = secret_file.as_deref() {
        arguments.push(quote("--secret-file"));
        arguments.push(quote(&secret_file.display().to_string()));
    }
    if let Some(electron_user_data_dir) = electron_user_data_dir {
        arguments.push(quote("--electron-user-data-dir"));
        arguments.push(quote(&electron_user_data_dir.display().to_string()));
    }
    if let Some(sqlite_home) = sqlite_home {
        arguments.push(quote("--sqlite-home"));
        arguments.push(quote(&sqlite_home.display().to_string()));
    }
    let content = render_terminal_launch_script(&quote(&helper.display().to_string()), &arguments);
    fs::write(&script, content).map_err(|error| {
        if let Some(secret_file) = secret_file.as_deref() {
            let _ = fs::remove_file(secret_file);
        }
        let _ = append_startup_log(
            &log_file,
            &format!("terminal_script_write_failed error={error}"),
        );
        format!("无法创建安全 Codex 启动器：{error}")
    })?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).map_err(|error| {
        if let Some(secret_file) = secret_file.as_deref() {
            let _ = fs::remove_file(secret_file);
        }
        let _ = append_startup_log(
            &log_file,
            &format!("terminal_script_permission_failed error={error}"),
        );
        format!("无法保护 Codex 启动器权限：{error}")
    })?;
    if crate::launcher_protocol::LaunchTarget::wire_is_managed_app(&launch_target) {
        // A managed desktop App has its own window and must not depend on an
        // interactive Terminal session to execute the temporary script. The
        // CLI path opens iTerm2 when present, otherwise Terminal, so users
        // retain an interactive shell; the App path spawns the same supervised launcher directly.
        Command::new("/bin/sh")
            .arg(&script)
            .spawn()
            .map_err(|error| {
                if let Some(secret_file) = secret_file.as_deref() {
                    let _ = fs::remove_file(secret_file);
                }
                let _ = append_startup_log(
                    &log_file,
                    &format!("app_spawn_failed target={launch_target} error={error}"),
                );
                format!("无法启动 Codex App：{error}")
            })?;
        let _ = append_startup_log(
            &log_file,
            &format!("app_spawn_dispatched target={launch_target}"),
        );
        if let (Some(secret_file), Some(launch_secret)) = (secret_file, launch_secret) {
            dispatch_secret_handoff(secret_file, launch_secret.to_string(), log_file.clone());
        }
        return Ok(());
    }
    // Do not rely on the user's `.command` file association. Finder/open can
    // otherwise hand the file to an editor or another shell integration and
    // return success before the launcher ever runs, leaving the instance in
    // the ambiguous `orphaned` state with no receipt. Prefer iTerm2 when it is
    // installed; otherwise use Terminal.
    let preferred_terminal = macos_cli_terminal_app();
    let (status, terminal_app) = open_cli_script_in_macos_terminal(&script, preferred_terminal)
        .map_err(|error| {
            if let Some(secret_file) = secret_file.as_deref() {
                let _ = fs::remove_file(secret_file);
            }
            format!("无法在终端启动 Codex：{error}")
        })?;
    if status.success() {
        let _ = append_startup_log(
            &log_file,
            &format!("terminal_open_dispatched app={terminal_app}"),
        );
        if let (Some(secret_file), Some(launch_secret)) = (secret_file, launch_secret) {
            dispatch_secret_handoff(secret_file, launch_secret.to_string(), log_file.clone());
        }
        Ok(())
    } else {
        if let Some(secret_file) = secret_file.as_deref() {
            let _ = fs::remove_file(secret_file);
        }
        let _ = append_startup_log(&log_file, &format!("terminal_open_failed status={status}"));
        Err(format!("终端启动器以状态 {status} 退出。"))
    }
}

fn macos_cli_terminal_app() -> &'static str {
    let mut candidates = vec![
        std::path::PathBuf::from("/Applications/iTerm.app"),
        std::path::PathBuf::from("/Applications/iTerm2.app"),
    ];
    if let Some(home) = crate::support::user_home() {
        candidates.push(home.join("Applications/iTerm.app"));
        candidates.push(home.join("Applications/iTerm2.app"));
    }
    macos_cli_terminal_app_from(candidates)
}

fn macos_cli_terminal_app_from(
    candidates: impl IntoIterator<Item = std::path::PathBuf>,
) -> &'static str {
    for path in candidates {
        if !path.is_dir() {
            continue;
        }
        match path.file_name().and_then(|name| name.to_str()) {
            Some("iTerm.app") => return "iTerm",
            Some("iTerm2.app") => return "iTerm2",
            _ => {}
        }
    }
    "Terminal"
}

fn open_cli_script_in_macos_terminal(
    script: &std::path::Path,
    preferred: &'static str,
) -> Result<(std::process::ExitStatus, &'static str), std::io::Error> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-a")
        .arg(preferred)
        .arg(script)
        .status()?;
    if status.success() {
        return Ok((status, preferred));
    }
    if preferred == "Terminal" {
        return Ok((status, "Terminal"));
    }
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-a")
        .arg("Terminal")
        .arg(script)
        .status()?;
    Ok((status, "Terminal"))
}

fn create_secret_fifo(path: &std::path::Path) -> Result<(), String> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let _ = std::fs::remove_file(path);
    let bytes = path.as_os_str().as_bytes();
    let c_path = CString::new(bytes).map_err(|_| "启动凭据管道路径无效。".to_string())?;
    // SAFETY: c_path is a valid NUL-terminated path and the mode restricts
    // access to the owning user. The return code is checked immediately.
    let result = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    if result != 0 {
        return Err(format!(
            "无法创建一次性启动凭据管道：{}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn dispatch_secret_handoff(
    path: std::path::PathBuf,
    mut secret: String,
    log_file: std::path::PathBuf,
) {
    use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt, time::Duration};
    std::thread::spawn(move || {
        // Terminal may take several seconds to start and execute the generated
        // script. Keep the FIFO alive for the whole handoff window; deleting it
        // after a successful write would race with the launcher's subsequent
        // open/read and turn a valid credential into ENOENT.
        for _ in 0..400 {
            let result = OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path);
            match result {
                Ok(mut file) => {
                    // A FIFO has no durable backing store; sync_all is not a
                    // meaningful success condition and may return EINVAL even
                    // after the bytes were delivered to the reader.
                    let write_result = file.write_all(secret.as_bytes());
                    secret.zeroize();
                    let event = if write_result.is_ok() {
                        "secret_handoff_written"
                    } else {
                        "secret_handoff_write_failed"
                    };
                    let _ = append_startup_log(&log_file, event);
                    return;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::WouldBlock
                    ) || error.raw_os_error() == Some(libc::ENXIO) =>
                {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
        secret.zeroize();
        let _ = std::fs::remove_file(&path);
        let _ = append_startup_log(&log_file, "secret_handoff_timeout");
    });
}

fn render_terminal_launch_script(helper: &str, arguments: &[String]) -> String {
    format!(
        "#!/bin/sh\ncleanup() {{ rm -f \"$0\"; }}\ntrap cleanup EXIT HUP INT TERM\n{helper} {}\nexit $?\n",
        arguments.join(" "),
    )
}

fn cleanup_stale_terminal_launchers() {
    let now = std::time::SystemTime::now();
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !is_managed_terminal_launcher(file_name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if now
            .duration_since(modified)
            .is_ok_and(|age| age > TERMINAL_LAUNCHER_MAX_AGE)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn is_managed_terminal_launcher(file_name: &str) -> bool {
    file_name.starts_with(TERMINAL_LAUNCHER_PREFIX) && file_name.ends_with(TERMINAL_LAUNCHER_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_terminal_launcher_script_cleans_itself_after_codex_exits() {
        let script = render_terminal_launch_script(
            "'/Applications/R Codex.app/Contents/MacOS/codex-launcher'",
            &["'--target'".to_string(), "'cli'".to_string()],
        );
        assert!(script.contains("trap cleanup EXIT HUP INT TERM"));
        assert!(script.contains("rm -f \"$0\""));
        assert!(script.contains("codex-launcher' '--target' 'cli'"));
        assert!(!script.contains("exec '/Applications"));
    }

    #[test]
    fn managed_terminal_launcher_name_is_scoped_to_rcodex_scripts() {
        assert!(is_managed_terminal_launcher("rcodex-a1b2.command"));
        assert!(!is_managed_terminal_launcher("codex.command"));
        assert!(!is_managed_terminal_launcher("rcodex-a1b2.sh"));
    }

    #[test]
    fn secret_handoff_survives_delayed_launcher_read() {
        let directory =
            std::env::temp_dir().join(format!("3r-secret-handoff-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let fifo = directory.join("launch-secret.fifo");
        let log_file = directory.join("startup.log");
        create_secret_fifo(&fifo).unwrap();
        dispatch_secret_handoff(
            fifo.clone(),
            "secret-sentinel".to_string(),
            log_file.clone(),
        );

        // Model Terminal taking time to start the generated script. The
        // parent writer must keep the FIFO present until the launcher reads it.
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(fifo.exists());
        let value = std::fs::read(&fifo).unwrap();
        assert_eq!(value, b"secret-sentinel");
        assert!(fifo.exists());
        std::fs::remove_file(&fifo).unwrap();

        for _ in 0..20 {
            if log_file.exists() {
                let log = std::fs::read_to_string(&log_file).unwrap();
                if log.contains("secret_handoff_written") {
                    std::fs::remove_dir_all(&directory).unwrap();
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::fs::remove_dir_all(&directory).unwrap();
        panic!("secret handoff writer did not report success");
    }

    #[test]
    fn macos_cli_prefers_iterm_when_the_app_bundle_exists() {
        let root = std::env::temp_dir().join(format!("3r-iterm-detect-{}", uuid::Uuid::new_v4()));
        let iterm = root.join("iTerm.app");
        std::fs::create_dir_all(&iterm).unwrap();
        assert_eq!(macos_cli_terminal_app_from(vec![iterm]), "iTerm");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn macos_cli_falls_back_to_terminal_when_iterm_is_absent() {
        let missing =
            std::env::temp_dir().join(format!("3r-iterm-missing-{}", uuid::Uuid::new_v4()));
        assert_eq!(macos_cli_terminal_app_from(vec![missing]), "Terminal");
    }
}
