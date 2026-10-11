//! Windows 商店包路径解析。不查询注册表，不启动进程。

use std::path::Path;

use super::windows::{is_windowsapps_launch_path, normalized_windows_path_text};

/// 从 WindowsApps 路径取出 PackageFamilyName。
/// OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe -> OpenAI.ChatGPT-Desktop_8wekyb3d8bbwe
pub fn package_family_from_path(path: &Path) -> Option<String> {
    if !is_windowsapps_launch_path(path) {
        return None;
    }
    let normalized = normalized_windows_path_text(path);
    let lower = normalized.to_ascii_lowercase();
    let marker = r"\windowsapps\";
    let index = lower.find(&marker)?;
    let after = &normalized[index + marker.len()..];
    let folder = after.split('\\').next()?;
    let (name_version, publisher) = folder.rsplit_once("__")?;
    let name = name_version.split('_').next()?;
    if name.is_empty() || publisher.is_empty() {
        return None;
    }
    Some(format!("{name}_{publisher}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn store_path_yields_family_name_with_publisher() {
        let path = Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe\app\ChatGPT.exe",
        );
        assert_eq!(
            package_family_from_path(path).as_deref(),
            Some("OpenAI.ChatGPT-Desktop_8wekyb3d8bbwe")
        );
    }

    #[test]
    fn unpackaged_path_has_no_family() {
        let path = Path::new(r"C:\Users\a\AppData\Local\Programs\Codex\ChatGPT.exe");
        assert_eq!(package_family_from_path(path), None);
    }
}
