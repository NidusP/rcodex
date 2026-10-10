use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
const CODEX_APP_BUNDLE_NAMES: [&str; 2] = ["Codex.app", "ChatGPT.app"];

#[cfg(target_os = "macos")]
const BUNDLED_CODEX_CLI_RELATIVE_PATHS: [&str; 2] = [
    "Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
    "Contents/Resources/codex",
];

/// Resolve the CLI runtime needed for the managed configuration and launch
/// contract. A desktop installation may embed that runtime instead of putting
/// `codex` on the user's shell PATH.
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
            let path = directory.join("codex");
            if is_executable_file(&path) {
                return path.canonicalize().map_err(|error| error.to_string());
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

    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for path in [
            home.join(".local/bin/codex"),
            home.join(".npm-global/bin/codex"),
        ] {
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

/// Standard macOS install locations for the desktop application. Both current
/// Codex branding and the legacy ChatGPT bundle name remain supported.
#[cfg(target_os = "macos")]
pub fn codex_app_bundle_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for root in app_roots() {
        for name in CODEX_APP_BUNDLE_NAMES {
            let candidate = root.join(name);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

#[cfg(not(target_os = "macos"))]
pub fn codex_app_bundle_candidates() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(target_os = "macos")]
fn app_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let user_applications = home.join("Applications");
        if !roots.contains(&user_applications) {
            roots.push(user_applications);
        }
    }
    roots
}

#[cfg(target_os = "macos")]
pub fn bundled_codex_cli_candidates(bundle: &Path) -> Vec<PathBuf> {
    BUNDLED_CODEX_CLI_RELATIVE_PATHS
        .iter()
        .map(|relative| bundle.join(relative))
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub fn bundled_codex_cli_candidates(_bundle: &Path) -> Vec<PathBuf> {
    Vec::new()
}

/// The desktop bundle is the outermost `.app` ancestor. New desktop builds
/// place the CLI in an inner `CodexCLI.app`, which is not the application that
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
    let bundle = codex_app_bundle_from_cli(codex)?;
    let bundle_name = bundle
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "无法识别 Codex Desktop 应用名。".to_string())?;
    let mut names = vec![
        bundle_name.to_string(),
        "Codex".to_string(),
        "ChatGPT".to_string(),
    ];
    names.dedup();
    for name in names {
        let executable = bundle.join("Contents").join("MacOS").join(name);
        if is_executable_file(&executable) {
            return Ok(executable);
        }
    }
    Err("未找到与 Codex CLI 匹配的桌面 App 可执行文件。".to_string())
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
}
