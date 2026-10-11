//! 路径、PID 和启动日志脱敏。
//!
//! 不读写 SQLite、不编排实例生命周期。Store 各方法和测试通过这些纯函数做校验与展示。
//! 时间戳走 `crate::support::unix_now_seconds`。

use std::{
    env,
    path::{Path, PathBuf},
};

pub(crate) use crate::support::resolve_workspace;
pub(super) use crate::support::set_private_directory;

pub(super) const MAX_EXPOSED_STARTUP_LOG_BYTES: usize = 32 * 1024;

/// Return whether the packaged runtime has passed the local managed-App
/// compatibility prerequisites. The probe is deliberately conservative: it
/// requires the two bundled sidecars and a discoverable Codex desktop binary,
/// while allowing an explicit kill switch for emergency rollback. It does not
/// change the user's default App or launch a second App during startup.
pub fn managed_app_compatibility_enabled() -> bool {
    #[cfg(target_os = "macos")]
    {
        env::var("THREER_CODEX_DISABLE_MANAGED_APP").as_deref() != Ok("1")
            && managed_app_runtime_probe()
    }
    #[cfg(target_os = "windows")]
    {
        env::var("THREER_CODEX_DISABLE_MANAGED_APP").as_deref() != Ok("1")
            && managed_app_runtime_probe()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

fn managed_app_runtime_probe() -> bool {
    let Ok(executable) = env::current_exe() else {
        return false;
    };
    let Some(parent) = executable.parent() else {
        return false;
    };
    let windows = cfg!(target_os = "windows");
    let launcher = if windows {
        "codex-launcher.exe"
    } else {
        "codex-launcher"
    };
    let gateway = if windows {
        "codex-gateway.exe"
    } else {
        "codex-gateway"
    };
    if !parent.join(launcher).is_file() || !parent.join(gateway).is_file() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        // The launcher resolves the CLI and derives its sibling desktop binary;
        // the probe only needs to establish that one supported desktop bundle is
        // installed. Avoid depending on PATH or a user-provided CODEX_BIN here so
        // a valid packaged install is not hidden by an unrelated shell override.
        return [
            "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
            "/Applications/Codex.app/Contents/MacOS/Codex",
        ]
        .into_iter()
        .any(|path| std::path::Path::new(path).is_file());
    }
    #[cfg(target_os = "windows")]
    {
        // First Windows testers use an unpackaged ChatGPT.exe / Codex.exe.
        // Store packages stay unavailable until package activation is recorded.
        let Ok(cli) = crate::codex_installation::resolve_codex_executable() else {
            return false;
        };
        let Ok(gui) = crate::codex_installation::resolve_codex_app_executable(&cli) else {
            return false;
        };
        return !crate::codex_installation::windows::is_windowsapps_launch_path(&gui);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

pub(crate) fn sanitize_startup_log(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            let mut safe = line
                .chars()
                .filter(|character| !character.is_control() || *character == '\t')
                .collect::<String>();
            for marker in [
                "Bearer ",
                "Authorization:",
                "OPENAI_API_KEY=",
                "CODEX_API_KEY=",
                "CODEX_ACCESS_TOKEN=",
                "THREER_INSTANCE_KEY=",
                "RCODEX_INSTANCE_KEY=",
            ] {
                if let Some(index) = safe.find(marker) {
                    safe.truncate(index);
                    safe.push_str("[redacted]");
                }
            }
            safe
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn managed_official_home_matches(
    root: &Path,
    account_id: &str,
    revision_id: &str,
    actual: &Path,
) -> bool {
    safe_resource_path_component(account_id)
        && safe_resource_path_component(revision_id)
        && root.join(account_id).join(revision_id) == actual
}

pub(super) fn safe_resource_path_component(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_:.".contains(character))
}

pub(super) fn process_exists(pid: u32) -> bool {
    crate::support::process_exists(pid)
}
