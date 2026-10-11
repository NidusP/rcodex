//! Windows GUI 启动路由：商店包身份或普通 exe。
//!
//! 控制面不调用这里。launcher 在拉起 GUI 前选择路径。
//! 没有包注册时，WindowsApps 路径必须失败，不能回退默认实例。

use std::path::Path;

use crate::codex_installation::windows::is_windowsapps_launch_path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredLaunch {
    pub family_name: String,
    pub app_id: String,
    pub executable: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManagedLaunchRoute {
    PackageIdentity(RegisteredLaunch),
    DirectExe,
}

pub fn managed_launch_route(
    configured: &Path,
    registered: Option<&RegisteredLaunch>,
) -> Result<ManagedLaunchRoute, String> {
    if let Some(package) = registered {
        return Ok(ManagedLaunchRoute::PackageIdentity(package.clone()));
    }
    if is_windowsapps_launch_path(configured) {
        return Err("未找到已注册的 Codex 商店包，无法启动隔离 App。".to_string());
    }
    Ok(ManagedLaunchRoute::DirectExe)
}

/// 去掉已有 user-data-dir，再追加当前隔离目录，避免落到默认窗口。
pub fn with_unique_user_data_dir_arg(args: &[String], dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "--user-data-dir" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("--user-data-dir=") {
            continue;
        }
        out.push(arg.clone());
    }
    out.push(format!("--user-data-dir={}", dir.display()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn unpackaged_path_uses_direct_exe() {
        let path = Path::new(r"C:\Users\a\AppData\Local\Programs\Codex\ChatGPT.exe");
        assert_eq!(
            managed_launch_route(path, None).unwrap(),
            ManagedLaunchRoute::DirectExe
        );
    }

    #[test]
    fn store_path_without_registration_does_not_fall_back() {
        let path = Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe\app\ChatGPT.exe",
        );
        let error = managed_launch_route(path, None).unwrap_err();
        assert!(error.contains("商店包"));
    }

    #[test]
    fn registered_store_package_uses_package_identity() {
        let path = Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe\app\ChatGPT.exe",
        );
        let registered = RegisteredLaunch {
            family_name: "OpenAI.ChatGPT-Desktop_8wekyb3d8bbwe".into(),
            app_id: "App".into(),
            executable: r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe\app\ChatGPT.exe".into(),
        };
        assert_eq!(
            managed_launch_route(path, Some(&registered)).unwrap(),
            ManagedLaunchRoute::PackageIdentity(registered)
        );
    }

    #[test]
    fn user_data_dir_is_applied_once() {
        let args = with_unique_user_data_dir_arg(
            &[
                "--foo".into(),
                "--user-data-dir".into(),
                "old".into(),
                "--user-data-dir=also-old".into(),
            ],
            Path::new("/tmp/electron"),
        );
        assert_eq!(
            args,
            vec![
                "--foo".to_string(),
                "--user-data-dir=/tmp/electron".to_string()
            ]
        );
    }
}
