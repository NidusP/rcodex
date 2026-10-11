use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(test, target_os = "windows"))]
pub mod windows;
#[cfg(any(test, target_os = "windows"))]
pub mod windows_store;

/// Resolve the CLI runtime needed for the managed configuration and launch
/// contract. A desktop installation may embed that runtime instead of putting
///  on the user's shell PATH.
pub fn resolve_codex_executable() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CODEX_BIN").map(PathBuf::from) {
        if !is_executable_file(&path) {
            return Err(format!(
                "CODEX_BIN 指向的 Codex 可执行文件不存在或不可执行：{}",
                path.display()
            ));
        }
        return path
            .canonicalize()
            .map_err(|error| format!("CODEX_BIN 指向的 Codex 不可用：{error}"));
    }

    if let Some(path_value) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path_value) {
            for name in path_cli_file_names() {
                let path = directory.join(name);
                if is_executable_file(&path) {
                    return path.canonicalize().map_err(|error| error.to_string());
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        for bundle in codex_app_bundle_candidates() {
            for path in bundled_codex_cli_candidates(&bundle) {
                if is_executable_file(&path) {
                    return path.canonicalize().map_err(|error| error.to_string());
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            for candidate in windows::program_cli_candidates(&local_app_data.to_string_lossy()) {
                let path = PathBuf::from(&candidate);
                if is_executable_file(&path) {
                    return path.canonicalize().map_err(|error| error.to_string());
                }
            }
        }
    }

    if let Some(home) = crate::support::user_home() {
        for relative in [
            ".local/bin/codex",
            ".local/bin/codex.exe",
            ".npm-global/bin/codex",
            ".npm-global/bin/codex.exe",
        ] {
            let path = home.join(relative);
            if is_executable_file(&path) {
                return path.canonicalize().map_err(|error| error.to_string());
            }
        }
    }

    Err(
        "未找到 Codex CLI。请确认已安装 Codex Desktop，或设置 CODEX_BIN 指向 codex 可执行文件。"
            .to_string(),
    )
}

fn path_cli_file_names() -> &'static [&'static str] {
    &["codex", "codex.exe"]
}

/// Standard macOS install locations for the desktop application. Both current
/// Codex branding and the legacy ChatGPT bundle name remain supported.
#[cfg(target_os = "macos")]
pub fn codex_app_bundle_candidates() -> Vec<PathBuf> {
    macos::codex_app_bundle_candidates()
}

#[cfg(not(target_os = "macos"))]
pub fn codex_app_bundle_candidates() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(target_os = "macos")]
pub fn bundled_codex_cli_candidates(bundle: &Path) -> Vec<PathBuf> {
    macos::bundled_codex_cli_candidates(bundle)
}

#[cfg(not(target_os = "macos"))]
pub fn bundled_codex_cli_candidates(_bundle: &Path) -> Vec<PathBuf> {
    Vec::new()
}

/// The desktop bundle is the outermost  ancestor. New desktop builds
/// place the CLI in an inner , which is not the application that
/// must receive the isolated Electron environment.
pub fn codex_app_bundle_from_cli(codex: &Path) -> Result<PathBuf, String> {
    codex
        .ancestors()
        .filter(|ancestor| ancestor.extension().and_then(|value| value.to_str()) == Some("app"))
        .last()
        .map(Path::to_path_buf)
        .ok_or_else(|| "无法定位 Codex Desktop 应用包。".to_string())
}

pub fn resolve_codex_app_executable(codex: &Path) -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        return macos::resolve_gui_executable(codex);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::resolve_gui_executable(codex);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = codex;
        Err("当前平台尚不支持受管理 Codex Desktop。".to_string())
    }
}

pub fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_outer_desktop_bundle_for_legacy_and_nested_cli_layouts() {
        for cli in [
            "/Applications/ChatGPT.app/Contents/Resources/codex",
            "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
        ] {
            assert_eq!(
                codex_app_bundle_from_cli(Path::new(cli)).unwrap(),
                PathBuf::from("/Applications/ChatGPT.app")
            );
        }
        assert!(codex_app_bundle_from_cli(Path::new("/usr/local/bin/codex")).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn supports_current_and_legacy_cli_paths_inside_a_desktop_bundle() {
        let bundle = Path::new("/Applications/Codex.app");
        assert_eq!(
            bundled_codex_cli_candidates(bundle),
            vec![
                bundle.join("Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex"),
                bundle.join("Contents/Resources/codex"),
            ]
        );
    }

    #[test]
    fn path_search_tries_codex_exe_as_well_as_codex() {
        assert_eq!(path_cli_file_names(), &["codex", "codex.exe"]);
    }
}
