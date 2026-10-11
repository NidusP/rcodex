//! 本机 Codex 安装与目录 IPC。
//!
//! 查询 CLI/App 安装、官方版本、打开默认 App、揭示隔离目录、同步偏好清单。
//! 不启动受管理实例，也不读取用户原 auth.json。
//! 由工作区控制带和安装状态调用。

#[cfg(target_os = "windows")]
use crate::codex_installation::resolve_codex_app_executable;
use crate::codex_installation::{
    codex_app_bundle_candidates, codex_app_bundle_from_cli, resolve_codex_executable,
};
use crate::managed_codex_config::{read_bundled_catalog, sync_default_codex_preferences};
use crate::managed_launch::ManagedLaunchStore;
use crate::special_models::{self, SpecialModelRoleSummary};

#[tauri::command]
pub fn get_codex_special_model_roles() -> Result<Vec<SpecialModelRoleSummary>, String> {
    let codex_executable = resolve_codex_executable()?;
    let bundled_catalog = read_bundled_catalog(&codex_executable)?;
    Ok(special_models::inspect_bundled_catalog(&bundled_catalog))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexAuthSummary {
    state: String,
    method: Option<String>,
    source_path: Option<String>,
    updated_at: Option<i64>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexInstallationStatus {
    installed: bool,
    executable_path: Option<String>,
    version: Option<String>,
    executable_updated_at: Option<i64>,
    app_path: Option<String>,
    app_version: Option<String>,
    app_updated_at: Option<i64>,
    auth: CodexAuthSummary,
    error: Option<String>,
}

#[tauri::command]
pub fn codex_installation_status() -> CodexInstallationStatus {
    let (app_path, app_version, app_updated_at) = codex_app_bundle_status();
    let auth = CodexAuthSummary {
        state: "not_detected".into(),
        method: None,
        source_path: None,
        updated_at: None,
    };
    match resolve_codex_executable() {
        Ok(path) => match codex_version(&path) {
            Ok(version) => CodexInstallationStatus {
                installed: true,
                executable_path: Some(path.display().to_string()),
                version: Some(version),
                executable_updated_at: file_updated_at(&path),
                app_path,
                app_version,
                app_updated_at,
                auth,
                error: None,
            },
            Err(error) => CodexInstallationStatus {
                installed: false,
                executable_path: Some(path.display().to_string()),
                version: None,
                executable_updated_at: file_updated_at(&path),
                app_path,
                app_version,
                app_updated_at,
                auth,
                error: Some(error),
            },
        },
        Err(error) => CodexInstallationStatus {
            installed: false,
            executable_path: None,
            version: None,
            executable_updated_at: None,
            app_path,
            app_version,
            app_updated_at,
            auth,
            error: Some(error),
        },
    }
}

fn file_updated_at(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

#[cfg(target_os = "macos")]
fn codex_app_bundle_status() -> (Option<String>, Option<String>, Option<i64>) {
    for bundle in codex_app_bundle_candidates() {
        let plist = bundle.join("Contents/Info.plist");
        if !plist.is_file() {
            continue;
        }
        let version = std::process::Command::new("/usr/libexec/PlistBuddy")
            .args([
                "-c",
                "Print :CFBundleShortVersionString",
                plist.to_string_lossy().as_ref(),
            ])
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    String::from_utf8(output.stdout)
                        .ok()
                        .map(|value| value.trim().to_string())
                } else {
                    None
                }
            });
        return (
            Some(bundle.display().to_string()),
            version.filter(|value| !value.is_empty()),
            file_updated_at(&bundle),
        );
    }
    (None, None, None)
}

#[cfg(not(target_os = "macos"))]
fn codex_app_bundle_status() -> (Option<String>, Option<String>, Option<i64>) {
    (None, None, None)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexUpdateStatus {
    state: String,
    current_version: Option<String>,
    latest_version: Option<String>,
    published_at: Option<String>,
    release_url: Option<String>,
    notes: Option<String>,
    checked_at: i64,
    error: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct GithubRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
    body: Option<String>,
    draft: bool,
    prerelease: bool,
}

fn version_numbers(value: &str) -> Vec<u64> {
    value
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .split(|c: char| !c.is_ascii_digit())
        .take(3)
        .filter_map(|part| part.parse().ok())
        .collect()
}

#[tauri::command]
pub async fn check_codex_updates() -> CodexUpdateStatus {
    let checked_at = chrono::Utc::now().timestamp();
    let current = resolve_codex_executable()
        .and_then(|path| codex_version(&path))
        .ok();
    let client = match reqwest::Client::builder()
        .user_agent("R-Codex-Version-Checker")
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return CodexUpdateStatus {
                state: "unavailable".into(),
                current_version: current,
                latest_version: None,
                published_at: None,
                release_url: None,
                notes: None,
                checked_at,
                error: Some(format!("无法初始化官方发布检查：{error}")),
            }
        }
    };
    // The list endpoint includes every release and its asset metadata.  It can
    // be hundreds of kilobytes and is prone to partial-body decode failures on
    // slow/proxied connections.  `/latest` is the official compact endpoint
    // for the newest stable (non-draft, non-prerelease) release.
    let response = match client
        .get("https://api.github.com/repos/openai/codex/releases/latest")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            return CodexUpdateStatus {
                state: "unavailable".into(),
                current_version: current,
                latest_version: None,
                published_at: None,
                release_url: None,
                notes: None,
                checked_at,
                error: Some("无法连接官方发布源".into()),
            };
        }
    };
    let latest = match response.error_for_status() {
        Ok(response) => match response.json::<GithubRelease>().await {
            Ok(release) => release,
            Err(error) => {
                return CodexUpdateStatus {
                    state: "unavailable".into(),
                    current_version: current,
                    latest_version: None,
                    published_at: None,
                    release_url: None,
                    notes: None,
                    checked_at,
                    error: Some(format!("官方发布内容无法解析（响应可能不完整）：{error}")),
                }
            }
        },
        Err(error) => {
            return CodexUpdateStatus {
                state: "unavailable".into(),
                current_version: current,
                latest_version: None,
                published_at: None,
                release_url: None,
                notes: None,
                checked_at,
                error: Some(format!("官方发布源返回异常：{error}")),
            }
        }
    };
    if latest.draft || latest.prerelease || version_numbers(&latest.tag_name).is_empty() {
        return CodexUpdateStatus {
            state: "unavailable".into(),
            current_version: current,
            latest_version: None,
            published_at: None,
            release_url: None,
            notes: None,
            checked_at,
            error: Some("官方发布源没有可识别的稳定版本".into()),
        };
    };
    let latest_version = latest.tag_name.trim_start_matches('v').to_string();
    let state = match current.as_deref().map(version_numbers) {
        Some(current_numbers) if current_numbers >= version_numbers(&latest_version) => {
            "up_to_date"
        }
        Some(_) => "update_available",
        None => "unavailable",
    };
    CodexUpdateStatus {
        state: state.into(),
        current_version: current,
        latest_version: Some(latest_version),
        published_at: latest.published_at,
        release_url: Some(latest.html_url),
        notes: latest.body.map(|body| body.chars().take(4000).collect()),
        checked_at,
        error: None,
    }
}

#[tauri::command]
pub fn open_codex_app(_workspace_path: Option<String>) -> Result<(), String> {
    // A workspace path would be handed to the already-running bundle. This
    // entry only starts the default app, with no isolated arguments.
    launch_codex_in_app(None)
}

#[tauri::command]
pub fn open_isolated_codex_home(
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
    launch_preset_id: String,
) -> Result<(), String> {
    let home = managed_launch.isolated_codex_home_for_reveal(&launch_preset_id)?;
    reveal_owned_directory(&home)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncManagedPreferencesInput {
    launch_preset_id: String,
    sections: Vec<String>,
}

#[tauri::command]
pub fn sync_managed_preferences(
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
    input: SyncManagedPreferencesInput,
) -> Result<Vec<String>, String> {
    // Config files are part of the running App's launch contract; require the
    // managed App to be stopped before replacing selected preferences.
    let home = managed_launch.isolated_codex_home_for_preset(&input.launch_preset_id)?;
    let source = crate::support::user_home()
        .ok_or_else(|| "default_codex_home_unavailable".to_string())?
        .join(".codex");
    sync_default_codex_preferences(&home, &source, &input.sections)
}

fn configure_default_codex_open(command: &mut std::process::Command, bundle: &std::path::Path) {
    command
        .arg("-n")
        .arg(bundle)
        .env_remove("CODEX_HOME")
        .env_remove("CODEX_SQLITE_HOME")
        .env_remove("CODEX_ELECTRON_USER_DATA_PATH")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN")
        .env_remove("OPENAI_API_KEY")
        .env_remove("RCODEX_INSTANCE_KEY")
        .env_remove("THREER_INSTANCE_KEY");
}

fn reveal_owned_directory(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg(path)
            .status()
            .map_err(|error| format!("无法打开目录：{error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("无法打开目录：open 以状态 {status} 退出。"))
        }
    }
    #[cfg(target_os = "windows")]
    {
        let status = std::process::Command::new("explorer")
            .arg(path)
            .status()
            .map_err(|error| format!("无法打开目录：{error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("无法打开目录：explorer 以状态 {status} 退出。"))
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = path;
        Err("当前版本仅在 macOS 和 Windows 打开隔离 CODEX_HOME。".to_string())
    }
}

#[cfg(target_os = "macos")]
fn launch_codex_in_app(_working_directory: Option<&str>) -> Result<(), String> {
    let codex = resolve_codex_executable()?;
    let bundle = codex_app_bundle_from_cli(&codex)?;
    let mut command = std::process::Command::new("/usr/bin/open");
    configure_default_codex_open(&mut command, &bundle);
    let status = command
        .status()
        .map_err(|error| format!("无法打开默认 Codex：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("无法打开默认 Codex：open 以状态 {status} 退出。"))
    }
}

#[cfg(target_os = "windows")]
fn launch_codex_in_app(_working_directory: Option<&str>) -> Result<(), String> {
    let cli = resolve_codex_executable()?;
    let gui = resolve_codex_app_executable(&cli)?;
    if crate::codex_installation::windows::is_windowsapps_launch_path(&gui) {
        return Err("当前版本请使用非商店安装的 Codex 桌面。商店包激活尚未验收。".to_string());
    }
    std::process::Command::new(&gui)
        .env_remove("CODEX_HOME")
        .env_remove("CODEX_SQLITE_HOME")
        .env_remove("CODEX_ELECTRON_USER_DATA_PATH")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN")
        .env_remove("OPENAI_API_KEY")
        .env_remove("RCODEX_INSTANCE_KEY")
        .env_remove("THREER_INSTANCE_KEY")
        .spawn()
        .map_err(|error| format!("无法打开默认 Codex：{error}"))?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn launch_codex_in_app(_working_directory: Option<&str>) -> Result<(), String> {
    Err("当前版本仅在 macOS 和 Windows 提供 Codex App 启动。".to_string())
}

pub(crate) fn resolve_current_codex_home() -> std::path::PathBuf {
    if let Some(value) = std::env::var_os("CODEX_HOME") {
        return std::path::PathBuf::from(value);
    }
    crate::support::user_home()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".codex")
}

pub(crate) fn codex_version(executable: &std::path::Path) -> Result<String, String> {
    let probe_home = std::env::temp_dir().join(format!("rcodex-version-{}", uuid::Uuid::new_v4()));
    let mut directory = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory
        .create(&probe_home)
        .map_err(|_| "codex_version_probe_home_failed".to_string())?;
    let output = std::process::Command::new(executable)
        .arg("--version")
        .env("CODEX_HOME", &probe_home)
        .env("CODEX_SQLITE_HOME", &probe_home)
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN")
        .output()
        .map_err(|error| format!("无法读取 Codex 版本：{error}"));
    let _ = std::fs::remove_dir_all(&probe_home);
    let output = output?;
    if !output.status.success() {
        return Err("Codex 版本检查失败。".to_string());
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        Err("Codex 版本检查返回空结果。".to_string())
    } else {
        Ok(version)
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::codex_installation::codex_app_bundle_from_cli;

    #[test]
    fn default_codex_open_starts_a_new_instance_without_isolation_args() {
        let bundle = codex_app_bundle_from_cli(std::path::Path::new(
            "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
        )).unwrap();
        assert_eq!(
            bundle,
            std::path::PathBuf::from("/Applications/ChatGPT.app")
        );
        assert!(codex_app_bundle_from_cli(std::path::Path::new("/usr/local/bin/codex")).is_err());

        let mut command = std::process::Command::new("/usr/bin/open");
        command
            .env("CODEX_HOME", "/tmp/isolated")
            .env("RCODEX_INSTANCE_KEY", "secret");
        configure_default_codex_open(&mut command, &bundle);
        let arguments = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            arguments,
            vec!["-n".to_string(), "/Applications/ChatGPT.app".to_string()]
        );
        assert!(arguments
            .iter()
            .all(|argument| argument != "--args" && !argument.contains("user-data-dir")));
        let removals = command
            .get_envs()
            .filter_map(|(name, value)| value.is_none().then(|| name.to_string_lossy().to_string()))
            .collect::<std::collections::HashSet<_>>();
        for name in [
            "CODEX_HOME",
            "CODEX_SQLITE_HOME",
            "CODEX_ELECTRON_USER_DATA_PATH",
            "CODEX_API_KEY",
            "CODEX_ACCESS_TOKEN",
            "OPENAI_API_KEY",
            "RCODEX_INSTANCE_KEY",
            "THREER_INSTANCE_KEY",
        ] {
            assert!(removals.contains(name), "{name}");
        }
    }
}
