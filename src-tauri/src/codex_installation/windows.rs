//! Windows 路径发现。纯文本解析可在非 Windows 主机上测试。

use std::path::{Path, PathBuf};

pub const CLI_FILE_NAMES: [&str; 2] = ["codex.exe", "codex"];
pub const GUI_FILE_NAMES: [&str; 2] = ["ChatGPT.exe", "Codex.exe"];

/// 把任意分隔符收成 Windows 风格，便于在 Mac 上测商店路径。
pub fn normalized_windows_path_text(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_string()
}

pub fn is_windowsapps_launch_path(path: &Path) -> bool {
    normalized_windows_path_text(path)
        .to_ascii_lowercase()
        .contains(r"\windowsapps\")
}

/// 启动路径形如 InstallLocation/app/ChatGPT.exe，去掉两级即安装根。
/// 用文本切割，避免 Path::parent 在非 Windows 主机上不认反斜杠。
pub fn windowsapps_install_location_from_launch_path(launch_path: &Path) -> Option<String> {
    if !is_windowsapps_launch_path(launch_path) {
        return None;
    }
    let normalized = normalized_windows_path_text(launch_path);
    let install_location = normalized.rsplitn(3, '\\').nth(2)?;
    let text = install_location.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

pub fn is_store_gui_file_name(path: &Path) -> bool {
    let name = normalized_windows_path_text(path)
        .rsplit('\\')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    name == "chatgpt.exe" || name == "codex.exe"
}

pub fn program_cli_candidates(local_app_data: &str) -> Vec<String> {
    let root = normalized_windows_path_text(Path::new(local_app_data));
    let mut candidates = Vec::new();
    for directory in ["Codex", "ChatGPT", "OpenAI Codex"] {
        for name in CLI_FILE_NAMES {
            candidates.push(format!(r"{root}\Programs\{directory}\{name}"));
        }
    }
    candidates
}

pub fn gui_candidates_from_cli(cli: &Path) -> Vec<String> {
    let normalized = normalized_windows_path_text(cli);
    let original_file = normalized.rsplit('\\').next().unwrap_or_default();
    let file = original_file.to_ascii_lowercase();
    let mut candidates = Vec::new();
    // ChatGPT.exe is always the GUI. Codex.exe (desktop branding) is GUI.
    // lowercase codex.exe is the CLI and must derive a sibling GUI first.
    if file == "chatgpt.exe" || original_file == "Codex.exe" {
        candidates.push(normalized.clone());
    }
    if let Some((parent, _)) = normalized.rsplit_once('\\') {
        for name in GUI_FILE_NAMES {
            let candidate = format!(r"{parent}\{name}");
            if !candidates
                .iter()
                .any(|current| current.eq_ignore_ascii_case(&candidate))
            {
                candidates.push(candidate);
            }
        }
    }
    // Store CLI lives under app\resources. The desktop exe is one directory above.
    if let Some((app_dir, _)) = normalized.rsplit_once(r"\resources\") {
        for name in GUI_FILE_NAMES {
            let candidate = format!(r"{app_dir}\{name}");
            if !candidates
                .iter()
                .any(|current| current.eq_ignore_ascii_case(&candidate))
            {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

pub fn store_cli_candidates(install_location: &str) -> Vec<String> {
    let root = normalized_windows_path_text(Path::new(install_location));
    [
        r"app\resources\codex.exe",
        r"app\resources\codex-cli\codex.exe",
        r"app\resources\codex-cli\bin\codex.exe",
        r"app\codex.exe",
    ]
    .into_iter()
    .map(|relative| format!(r"{root}\{relative}"))
    .collect()
}

/// 从 CLI 或 GUI 路径推出应对接隔离环境的 Desktop exe。不访问磁盘。
pub fn derive_gui_path_from_cli(cli: &Path) -> Option<String> {
    gui_candidates_from_cli(cli).into_iter().next()
}

pub fn path_cli_candidates(directory: &Path) -> Vec<PathBuf> {
    CLI_FILE_NAMES
        .iter()
        .map(|name| directory.join(name))
        .collect()
}

pub fn resolve_gui_executable(cli: &Path) -> Result<PathBuf, String> {
    for candidate in gui_candidates_from_cli(cli) {
        let path = PathBuf::from(&candidate);
        if super::is_executable_file(&path) {
            return Ok(path);
        }
    }
    Err("未找到与 Codex CLI 匹配的桌面 App 可执行文件。".to_string())
}

#[cfg(target_os = "windows")]
pub fn query_store_install_location() -> Option<String> {
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$pkg = Get-AppxPackage -Name OpenAI.Codex; if (-not $pkg) { $pkg = Get-AppxPackage | Where-Object { $_.Name -like 'OpenAI.ChatGPT*' } | Select-Object -First 1 }; if ($pkg) { $pkg.InstallLocation }",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    if line
        .chars()
        .any(|character| character.is_control() || character == ';' || character == '&')
    {
        return None;
    }
    Some(line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_install_location_strips_app_and_exe() {
        let path = Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe\app\ChatGPT.exe",
        );
        assert!(is_windowsapps_launch_path(path));
        assert_eq!(
            windowsapps_install_location_from_launch_path(path).as_deref(),
            Some(
                r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe"
            )
        );
        assert!(is_store_gui_file_name(path));
    }

    #[test]
    fn unpackaged_cli_derives_sibling_gui() {
        let cli = Path::new(r"C:\Users\a\AppData\Local\Programs\Codex\codex.exe");
        assert!(!is_windowsapps_launch_path(cli));
        assert_eq!(
            derive_gui_path_from_cli(cli).as_deref(),
            Some(r"C:\Users\a\AppData\Local\Programs\Codex\ChatGPT.exe")
        );
        assert_eq!(
            gui_candidates_from_cli(cli),
            vec![
                r"C:\Users\a\AppData\Local\Programs\Codex\ChatGPT.exe".to_string(),
                r"C:\Users\a\AppData\Local\Programs\Codex\Codex.exe".to_string(),
            ]
        );
    }

    #[test]
    fn gui_path_is_returned_as_is() {
        let gui = Path::new("D:/Apps/Codex/Codex.exe");
        assert_eq!(
            derive_gui_path_from_cli(gui).as_deref(),
            Some(r"D:\Apps\Codex\Codex.exe")
        );
    }

    #[test]
    fn store_resources_cli_also_checks_app_directory() {
        let cli = Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe\app\resources\codex.exe",
        );
        let candidates = gui_candidates_from_cli(cli);
        assert!(candidates.iter().any(|path| path.eq_ignore_ascii_case(
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe\app\ChatGPT.exe"
        )));
    }

    #[test]
    fn store_install_location_lists_cli_candidates() {
        let candidates = store_cli_candidates(
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe",
        );
        assert!(candidates.contains(
            &r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe\app\resources\codex.exe".to_string()
        ));
    }
    #[test]
    fn local_programs_search_covers_codex_and_chatgpt() {
        let candidates = program_cli_candidates(r"C:\Users\a\AppData\Local");
        assert!(
            candidates.contains(&r"C:\Users\a\AppData\Local\Programs\Codex\codex.exe".to_string())
        );
        assert!(candidates
            .contains(&r"C:\Users\a\AppData\Local\Programs\ChatGPT\codex.exe".to_string()));
    }
}
