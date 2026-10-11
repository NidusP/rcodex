//! 受管理 Codex Desktop 的启动环境与参数。
//!
//! 控制面不调用这里；`codex-launcher` 在拉起 GUI 前组装环境。
//! 不解析商店包、不 spawn 进程。

use std::{env, path::Path, process::Command};

#[cfg(any(test, target_os = "windows"))]
pub mod windows;
#[cfg(any(test, target_os = "windows"))]
pub mod windows_package;

/// 给受管理 Desktop 加上隔离目录、用户数据路径和回环 NO_PROXY。
pub fn configure_managed_app_command(
    command: &mut Command,
    codex_home: &Path,
    electron_user_data_dir: &Path,
    sqlite_home: &Path,
    instance_key: &str,
) {
    command
        .env("CODEX_HOME", codex_home)
        .env("CODEX_SQLITE_HOME", sqlite_home)
        .env("CODEX_ELECTRON_USER_DATA_PATH", electron_user_data_dir)
        .env("RCODEX_INSTANCE_KEY", instance_key)
        // Electron must see the user-data path in the initial argv parse. The
        // space-separated form can be treated as a late path mutation by the
        // desktop wrapper and then forwarded to the default singleton.
        .arg(format!(
            "--user-data-dir={}",
            electron_user_data_dir.display()
        ))
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN");
    // Codex Desktop honors macOS system proxy. A local Clash/V2Ray on
    // 127.0.0.1:789x answers loopback Gateway POSTs with an empty 502
    // (`content-length: 0`), which Codex prints as Unknown error. CLI
    // launches already bypass loopback; App must too.
    apply_loopback_no_proxy(command);
}

/// 默认 Codex 入口不得继承隔离环境。
pub fn configure_unmanaged_app_command(command: &mut Command, directory: &Path) {
    command
        .arg("app")
        .arg(directory)
        .env_remove("CODEX_HOME")
        .env_remove("CODEX_SQLITE_HOME")
        .env_remove("CODEX_ACCESS_TOKEN")
        .env_remove("CODEX_API_KEY")
        .env_remove("OPENAI_API_KEY");
}

/// 把回环地址并入 NO_PROXY，避免本机 Gateway 被系统代理劫持。
pub fn apply_loopback_no_proxy(command: &mut Command) {
    let no_proxy = merged_loopback_no_proxy(
        env::var("NO_PROXY").ok().as_deref(),
        env::var("no_proxy").ok().as_deref(),
    );
    command.env("NO_PROXY", &no_proxy).env("no_proxy", no_proxy);
}

/// 合并已有 NO_PROXY 并补上 127.0.0.1、localhost、::1。
pub fn merged_loopback_no_proxy(upper: Option<&str>, lower: Option<&str>) -> String {
    let mut entries = Vec::<String>::new();
    for value in [upper, lower].into_iter().flatten() {
        for entry in value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if !entries
                .iter()
                .any(|current| current.eq_ignore_ascii_case(entry))
            {
                entries.push(entry.to_string());
            }
        }
    }
    for entry in ["127.0.0.1", "localhost", "::1"] {
        if !entries
            .iter()
            .any(|current| current.eq_ignore_ascii_case(entry))
        {
            entries.push(entry.to_string());
        }
    }
    entries.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn managed_app_uses_isolated_home_and_electron_data_dir_without_provider_key() {
        let home = PathBuf::from("/tmp/3r-managed-app-home");
        let electron = PathBuf::from("/tmp/3r-managed-app-electron");
        let mut command = Command::new("/bin/true");
        command
            .env("OPENAI_API_KEY", "provider-secret-sentinel")
            .env("CODEX_ACCESS_TOKEN", "official-token-sentinel");
        configure_managed_app_command(
            &mut command,
            &home,
            &electron,
            Path::new("/tmp/3r-managed-app-sqlite"),
            "instance-sentinel",
        );

        let arguments = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            arguments,
            vec!["--user-data-dir=/tmp/3r-managed-app-electron"]
        );
        let environment = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().to_string(),
                    value.map(|item| item.to_string_lossy().to_string()),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            environment.get("CODEX_HOME"),
            Some(&Some("/tmp/3r-managed-app-home".to_string()))
        );
        assert_eq!(
            environment.get("CODEX_ELECTRON_USER_DATA_PATH"),
            Some(&Some("/tmp/3r-managed-app-electron".to_string()))
        );
        assert_eq!(
            environment.get("CODEX_SQLITE_HOME"),
            Some(&Some("/tmp/3r-managed-app-sqlite".to_string()))
        );
        assert_eq!(
            environment.get("RCODEX_INSTANCE_KEY"),
            Some(&Some("instance-sentinel".to_string()))
        );
        for name in ["OPENAI_API_KEY", "CODEX_ACCESS_TOKEN"] {
            assert_eq!(environment.get(name), Some(&None));
        }
        let no_proxy = environment
            .get("NO_PROXY")
            .and_then(|value| value.as_deref())
            .unwrap();
        assert_eq!(
            environment
                .get("no_proxy")
                .and_then(|value| value.as_deref()),
            Some(no_proxy)
        );
        for entry in ["127.0.0.1", "localhost", "::1"] {
            assert!(no_proxy.split(',').any(|current| current == entry));
        }
    }

    #[test]
    fn loopback_proxy_bypass_preserves_and_deduplicates_parent_entries() {
        assert_eq!(
            merged_loopback_no_proxy(
                Some("internal.example, 127.0.0.1"),
                Some("LOCALHOST,.corp.example,internal.example"),
            ),
            "internal.example,127.0.0.1,LOCALHOST,.corp.example,::1"
        );
        assert_eq!(
            merged_loopback_no_proxy(None, None),
            "127.0.0.1,localhost,::1"
        );
    }
}
