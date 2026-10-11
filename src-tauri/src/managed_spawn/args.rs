//! 控制面交给 codex-launcher 的参数列表。
//!
//! macOS 会再做 POSIX 引号；Windows 必须原样进 argv，不能再套一层 shell。
#![allow(dead_code)]

use std::path::Path;

pub struct LauncherArgSpec<'a> {
    pub launch_target: &'a str,
    pub credential_kind: &'a str,
    pub instance_id: &'a str,
    pub launch_nonce: &'a str,
    pub status_file: &'a Path,
    pub log_file: &'a Path,
    pub codex_home: &'a Path,
    pub secret_ref: Option<&'a str>,
    pub instance_key_ref: Option<&'a str>,
    pub profile: Option<&'a str>,
    pub model: Option<&'a str>,
    pub working_directory: Option<&'a str>,
    pub secret_file: Option<&'a Path>,
    pub electron_user_data_dir: Option<&'a Path>,
    pub sqlite_home: Option<&'a Path>,
}

pub fn launcher_argv(spec: &LauncherArgSpec<'_>) -> Vec<String> {
    let mut args = Vec::new();
    let mut push = |flag: &str, value: &str| {
        args.push(flag.to_string());
        args.push(value.to_string());
    };
    push("--target", spec.launch_target);
    push("--credential-kind", spec.credential_kind);
    push("--instance-id", spec.instance_id);
    push("--launch-nonce", spec.launch_nonce);
    push("--status-file", &spec.status_file.display().to_string());
    push("--log-file", &spec.log_file.display().to_string());
    push("--codex-home", &spec.codex_home.display().to_string());
    if let Some(secret_ref) = spec.secret_ref {
        push("--secret-ref", secret_ref);
    }
    if let Some(instance_key_ref) = spec.instance_key_ref {
        push("--instance-key-ref", instance_key_ref);
    }
    if let Some(profile) = spec.profile {
        push("--profile", profile);
    }
    if let Some(model) = spec.model {
        push("--model", model);
    }
    if let Some(directory) = spec
        .working_directory
        .filter(|value| !value.trim().is_empty())
    {
        push("--working-directory", directory);
    }
    if let Some(secret_file) = spec.secret_file {
        push("--secret-file", &secret_file.display().to_string());
    }
    if let Some(electron_user_data_dir) = spec.electron_user_data_dir {
        push(
            "--electron-user-data-dir",
            &electron_user_data_dir.display().to_string(),
        );
    }
    if let Some(sqlite_home) = spec.sqlite_home {
        push("--sqlite-home", &sqlite_home.display().to_string());
    }
    args
}

pub fn quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn launcher_sidecar_file_name(windows: bool) -> &'static str {
    if windows {
        "codex-launcher.exe"
    } else {
        "codex-launcher"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn spec<'a>(
        status_file: &'a Path,
        log_file: &'a Path,
        home: &'a Path,
        electron: &'a Path,
        sqlite: &'a Path,
        secret: &'a Path,
    ) -> LauncherArgSpec<'a> {
        LauncherArgSpec {
            launch_target: "app",
            credential_kind: "managed_route_set",
            instance_id: "instance-a",
            launch_nonce: "nonce-a",
            status_file,
            log_file,
            codex_home: home,
            secret_ref: None,
            instance_key_ref: Some("key-ref"),
            profile: Some("launch-a"),
            model: Some("grok-4.6"),
            working_directory: Some(r"C:\\Users\\a\\project"),
            secret_file: Some(secret),
            electron_user_data_dir: Some(electron),
            sqlite_home: Some(sqlite),
        }
    }

    #[test]
    fn windows_argv_is_unquoted_flag_value_pairs() {
        let status = PathBuf::from("status.json");
        let log = PathBuf::from("startup.log");
        let home = PathBuf::from("codex-home");
        let electron = PathBuf::from("electron-data");
        let sqlite = PathBuf::from("sqlite-home");
        let secret = PathBuf::from("launch-secret");
        let args = launcher_argv(&spec(&status, &log, &home, &electron, &sqlite, &secret));
        assert_eq!(args[0], "--target");
        assert_eq!(args[1], "app");
        assert!(!args
            .iter()
            .any(|value| value.contains('\'') || value.contains('"')));
        assert!(args.windows(2).any(|pair| pair == ["--model", "grok-4.6"]));
        assert!(args
            .windows(2)
            .any(|pair| { pair[0] == "--electron-user-data-dir" && pair[1] == "electron-data" }));
    }

    #[test]
    fn sidecar_name_uses_exe_only_on_windows() {
        assert_eq!(launcher_sidecar_file_name(true), "codex-launcher.exe");
        assert_eq!(launcher_sidecar_file_name(false), "codex-launcher");
    }

    #[test]
    fn posix_quote_wraps_and_escapes_single_quotes() {
        assert_eq!(quote_posix("cli"), "'cli'");
        assert_eq!(quote_posix("a'b"), "'a'\\''b'");
    }
}
