use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

pub const LAUNCHER_PROTOCOL_VERSION: u32 = 1;
pub const MAX_STARTUP_LOG_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LauncherState {
    Running,
    Exited,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LauncherReceipt {
    pub protocol_version: u32,
    pub instance_id: String,
    pub launch_nonce: String,
    pub sequence: u64,
    pub state: LauncherState,
    pub launcher_pid: u32,
    pub codex_pid: u32,
    pub process_start_identity: String,
    pub codex_executable: String,
    pub codex_home: String,
    pub occurred_at_ms: u64,
    pub exit_code: Option<i32>,
}

impl LauncherReceipt {
    pub fn running(
        instance_id: String,
        launch_nonce: String,
        codex_pid: u32,
        process_start_identity: String,
        codex_executable: String,
        codex_home: String,
    ) -> Self {
        Self {
            protocol_version: LAUNCHER_PROTOCOL_VERSION,
            instance_id,
            launch_nonce,
            sequence: 1,
            state: LauncherState::Running,
            launcher_pid: std::process::id(),
            codex_pid,
            process_start_identity,
            codex_executable,
            codex_home,
            occurred_at_ms: unix_time_millis(),
            exit_code: None,
        }
    }

    pub fn exited_from(running: &Self, exit_code: Option<i32>) -> Self {
        let mut receipt = running.clone();
        receipt.sequence = running.sequence + 1;
        receipt.state = LauncherState::Exited;
        receipt.occurred_at_ms = unix_time_millis();
        receipt.exit_code = exit_code;
        receipt
    }
}

pub fn write_receipt_atomically(path: &Path, receipt: &LauncherReceipt) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "启动回执路径缺少父目录。".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建启动回执目录：{error}"))?;
    set_directory_permissions(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let payload =
        serde_json::to_vec(receipt).map_err(|error| format!("无法序列化启动回执：{error}"))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("无法创建临时启动回执：{error}"))?;
    if let Err(error) = file.write_all(&payload).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法写入启动回执：{error}"));
    }
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("无法发布启动回执：{error}")
    })
}

pub fn read_receipt(path: &Path) -> Result<LauncherReceipt, String> {
    let payload = fs::read(path).map_err(|error| format!("无法读取启动回执：{error}"))?;
    let receipt: LauncherReceipt =
        serde_json::from_slice(&payload).map_err(|error| format!("启动回执格式无效：{error}"))?;
    if receipt.protocol_version != LAUNCHER_PROTOCOL_VERSION {
        return Err(format!(
            "启动回执协议版本 {} 不受支持。",
            receipt.protocol_version
        ));
    }
    Ok(receipt)
}

/// Append a bounded, line-oriented lifecycle event for a managed Codex
/// instance. The launcher deliberately writes only event names and non-secret
/// metadata; the control plane applies a second redaction pass before exposing
/// the log to the UI.
pub fn append_startup_log(path: &Path, event: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "启动日志路径缺少父目录。".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建启动日志目录：{error}"))?;
    set_directory_permissions(parent)?;
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.len() as usize >= MAX_STARTUP_LOG_BYTES {
            return Ok(());
        }
    }
    let safe_event = event
        .chars()
        .filter(|character| *character != '\0' && *character != '\r' && *character != '\n')
        .take(4_096)
        .collect::<String>();
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("无法打开启动日志：{error}"))?;
    writeln!(file, "{safe_event}").map_err(|error| format!("无法写入启动日志：{error}"))?;
    file.sync_data()
        .map_err(|error| format!("无法同步启动日志：{error}"))?;
    Ok(())
}

pub fn process_start_identity(pid: u32) -> Result<String, String> {
    crate::support::process_start_identity(pid)
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg_attr(not(unix), allow(unused_variables))]
fn set_directory_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("无法保护启动回执目录：{error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_round_trip_replaces_running_with_exited() {
        let directory = std::env::temp_dir().join(format!("3r-launcher-{}", uuid::Uuid::new_v4()));
        let path = directory.join("status.json");
        let running = LauncherReceipt::running(
            "instance-a".to_string(),
            "nonce-a".to_string(),
            std::process::id(),
            "identity".to_string(),
            "/tmp/codex".to_string(),
            "/tmp/codex-home".to_string(),
        );
        write_receipt_atomically(&path, &running).unwrap();
        assert_eq!(read_receipt(&path).unwrap(), running);
        let exited = LauncherReceipt::exited_from(&running, Some(7));
        write_receipt_atomically(&path, &exited).unwrap();
        assert_eq!(read_receipt(&path).unwrap(), exited);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn current_process_has_a_stable_start_identity() {
        let first = process_start_identity(std::process::id()).unwrap();
        let second = process_start_identity(std::process::id()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn startup_log_is_line_safe_and_bounded() {
        let directory =
            std::env::temp_dir().join(format!("3r-launcher-log-{}", uuid::Uuid::new_v4()));
        let path = directory.join("startup.log");
        append_startup_log(&path, "first\nsecond\r\0third").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "firstsecondthird\n");
        append_startup_log(&path, &"x".repeat(MAX_STARTUP_LOG_BYTES + 1)).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() as usize <= MAX_STARTUP_LOG_BYTES + 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchTarget {
    Cli,
    ManagedApp,
}

impl LaunchTarget {
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::ManagedApp => "managed_app",
        }
    }

    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "cli" => Some(Self::Cli),
            "managed_app" => Some(Self::ManagedApp),
            _ => None,
        }
    }

    /// Accept the historical dashed spelling used by old generated launch scripts.
    pub fn parse_alias(value: &str) -> Option<Self> {
        match value {
            "managed-app" => Some(Self::ManagedApp),
            other => Self::from_wire(other),
        }
    }

    pub fn is_managed_app(self) -> bool {
        matches!(self, Self::ManagedApp)
    }

    pub fn wire_is_managed_app(value: &str) -> bool {
        value == Self::ManagedApp.as_wire()
    }
}

#[cfg(test)]
mod launch_target_tests {
    use super::LaunchTarget;

    #[test]
    fn wire_values_round_trip_and_alias_stays_at_the_boundary() {
        assert_eq!(LaunchTarget::from_wire("cli"), Some(LaunchTarget::Cli));
        assert_eq!(
            LaunchTarget::from_wire("managed_app"),
            Some(LaunchTarget::ManagedApp)
        );
        assert_eq!(LaunchTarget::from_wire("managed-app"), None);
        assert!(LaunchTarget::parse_alias("managed-app")
            .unwrap()
            .is_managed_app());
        assert!(LaunchTarget::wire_is_managed_app("managed_app"));
        assert!(!LaunchTarget::wire_is_managed_app("managed-app"));
        assert_eq!(LaunchTarget::ManagedApp.as_wire(), "managed_app");
    }
}
