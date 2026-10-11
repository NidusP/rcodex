//! macOS 上的 Codex Desktop / CLI 发现。

use std::path::{Path, PathBuf};

const CODEX_APP_BUNDLE_NAMES: [&str; 2] = ["Codex.app", "ChatGPT.app"];
const BUNDLED_CODEX_CLI_RELATIVE_PATHS: [&str; 2] = [
    "Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
    "Contents/Resources/codex",
];

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

fn app_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = crate::support::user_home() {
        let user_applications = home.join("Applications");
        if !roots.contains(&user_applications) {
            roots.push(user_applications);
        }
    }
    roots
}

pub fn bundled_codex_cli_candidates(bundle: &Path) -> Vec<PathBuf> {
    BUNDLED_CODEX_CLI_RELATIVE_PATHS
        .iter()
        .map(|relative| bundle.join(relative))
        .collect()
}

pub fn resolve_gui_executable(codex: &Path) -> Result<PathBuf, String> {
    let bundle = super::codex_app_bundle_from_cli(codex)?;
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
        if super::is_executable_file(&executable) {
            return Ok(executable);
        }
    }
    Err("未找到与 Codex CLI 匹配的桌面 App 可执行文件。".to_string())
}
