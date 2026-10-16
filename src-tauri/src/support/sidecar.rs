//! 安装包里伴随进程的文件名。
//!
//! Tauri 会把 externalBin 收成带目标三元组的文件。Windows 还有 .exe。
//! 这里只拼候选路径，不访问磁盘，不启动进程。

use std::path::{Path, PathBuf};

const WINDOWS_TRIPLES: [&str; 2] = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"];
const MACOS_TRIPLES: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

pub fn bundled_sidecar_file_names(stem: &str) -> Vec<String> {
    let mut names = Vec::new();
    names.push(format!("{stem}.exe"));
    for triple in WINDOWS_TRIPLES {
        names.push(format!("{stem}-{triple}.exe"));
    }
    names.push(stem.to_string());
    for triple in MACOS_TRIPLES {
        names.push(format!("{stem}-{triple}"));
    }
    names
}

pub fn bundled_sidecar_candidates(directory: &Path, stem: &str) -> Vec<PathBuf> {
    bundled_sidecar_file_names(stem)
        .into_iter()
        .map(|name| directory.join(name))
        .collect()
}

pub fn first_existing_sidecar(directory: &Path, stem: &str) -> Option<PathBuf> {
    bundled_sidecar_candidates(directory, stem)
        .into_iter()
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_package_names_include_exe_and_target_triple() {
        let names = bundled_sidecar_file_names("codex-gateway");
        assert!(names.iter().any(|name| name == "codex-gateway.exe"));
        assert!(names
            .iter()
            .any(|name| name == "codex-gateway-x86_64-pc-windows-msvc.exe"));
        assert!(names.iter().any(|name| name == "codex-gateway"));
    }
}
