//! Windows GUI 启动路由：商店包身份或普通 exe。
//!
//! 控制面不调用这里。launcher 在拉起 GUI 前选择路径。
//! 没有包注册时，WindowsApps 路径必须失败，不能回退默认实例。

use std::path::Path;

use crate::codex_installation::windows::{
    is_windowsapps_launch_path, normalized_windows_path_text, WindowsCodexInstall, WindowsSpawnKind,
};
use crate::codex_installation::windows_store::package_family_from_path;

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

/// GUI 所在目录。CreateProcess 的 cwd 必须是这里，不能是用户项目目录。
pub fn gui_directory(gui: &Path) -> Option<String> {
    let text = normalized_windows_path_text(gui);
    let (parent, file) = text.rsplit_once('\\')?;
    if parent.is_empty() || file.is_empty() {
        None
    } else {
        Some(parent.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectExeLaunchPlan {
    pub program: String,
    pub current_dir: String,
    pub args: Vec<String>,
}

/// DirectExe 的程序、cwd 和参数。项目目录只进 argv。
pub fn direct_exe_launch_plan(
    gui: &Path,
    electron_user_data_dir: &Path,
    project_directory: Option<&Path>,
) -> Result<DirectExeLaunchPlan, String> {
    let current_dir =
        gui_directory(gui).ok_or_else(|| "无法定位 Codex 桌面程序目录。".to_string())?;
    let mut args = with_unique_user_data_dir_arg(&[], electron_user_data_dir);
    if let Some(project) = project_directory {
        let text = normalized_windows_path_text(project);
        if !text.is_empty() {
            args.push(text);
        }
    }
    Ok(DirectExeLaunchPlan {
        program: normalized_windows_path_text(gui),
        current_dir,
        args,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnmanagedWindowsLaunch {
    StoreAppFolder { program: String, args: Vec<String> },
    DirectExe { gui: String, current_dir: String },
}

/// 打开默认 Codex：商店走开始菜单 AppsFolder，散装走 GUI 且 cwd 为 gui 目录。
pub fn unmanaged_windows_launch(
    install: &WindowsCodexInstall,
) -> Result<UnmanagedWindowsLaunch, String> {
    match install.spawn {
        WindowsSpawnKind::Package => {
            let family = package_family_from_path(Path::new(&install.gui))
                .or_else(|| package_family_from_path(Path::new(&install.cli)))
                .or_else(|| package_family_from_path(Path::new(&install.root)))
                .ok_or_else(|| "未找到 Codex 商店包家族，无法打开默认窗口。".to_string())?;
            Ok(UnmanagedWindowsLaunch::StoreAppFolder {
                program: "explorer.exe".to_string(),
                args: vec![format!("shell:AppsFolder\\{family}!App")],
            })
        }
        WindowsSpawnKind::Direct => {
            let current_dir = gui_directory(Path::new(&install.gui))
                .ok_or_else(|| "无法定位 Codex 桌面程序目录。".to_string())?;
            Ok(UnmanagedWindowsLaunch::DirectExe {
                gui: install.gui.clone(),
                current_dir,
            })
        }
    }
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

    #[test]
    fn direct_exe_uses_gui_directory_and_unique_user_data_dir() {
        use crate::codex_installation::windows::install_from_cli;
        let gui = Path::new(r"D:\Apps\Codex\app\ChatGPT.exe");
        let plan = direct_exe_launch_plan(
            gui,
            Path::new(r"E:\isolated\electron"),
            Some(Path::new(r"C:\Users\a\project")),
        )
        .unwrap();
        assert_eq!(plan.program, r"D:\Apps\Codex\app\ChatGPT.exe");
        assert_eq!(plan.current_dir, r"D:\Apps\Codex\app");
        assert_eq!(
            plan.args
                .iter()
                .filter(|arg| arg.starts_with("--user-data-dir="))
                .count(),
            1
        );
        assert!(plan
            .args
            .iter()
            .any(|arg| arg == r"--user-data-dir=E:\isolated\electron"));
        assert!(plan.args.iter().any(|arg| arg == r"C:\Users\a\project"));
        assert_ne!(plan.current_dir, r"C:\Users\a\project");
        let install =
            install_from_cli(Path::new(r"D:\Apps\Codex\app\resources\codex.exe")).unwrap();
        assert_eq!(
            unmanaged_windows_launch(&install).unwrap(),
            UnmanagedWindowsLaunch::DirectExe {
                gui: r"D:\Apps\Codex\app\ChatGPT.exe".to_string(),
                current_dir: r"D:\Apps\Codex\app".to_string(),
            }
        );
    }

    #[test]
    fn store_default_launch_uses_apps_folder() {
        use crate::codex_installation::windows::install_from_cli;
        let install = install_from_cli(Path::new(concat!(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.1013.3656.0_arm64__2p2nqsd0c76g0",
            r"\app\resources\codex.exe"
        )))
        .unwrap();
        assert_eq!(
            unmanaged_windows_launch(&install).unwrap(),
            UnmanagedWindowsLaunch::StoreAppFolder {
                program: "explorer.exe".to_string(),
                args: vec![r"shell:AppsFolder\OpenAI.Codex_2p2nqsd0c76g0!App".to_string()],
            }
        );
    }
}
