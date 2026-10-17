//! 隔离配置的原子写、符号链接拒绝和 URL/名称校验。
//!
//! 不解释模型目录、不改偏好。isolated_auth 和各物化路径共用这些写入原语。

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use url::Url;
use uuid::Uuid;

pub(crate) use crate::support::sha256_label;

pub(super) fn read_regular_toml(path: &Path) -> Result<Option<toml::Table>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("managed_preference_seed_stat_failed:{error}"))?;
    if metadata.file_type().is_symlink() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)
        .map_err(|error| format!("managed_preference_seed_read_failed:{error}"))?;
    text.parse::<toml::Table>()
        .map(Some)
        .map_err(|_| "managed_preference_seed_invalid".to_string())
}

pub(super) fn validate_gateway_url(url: &Url) -> Result<(), String> {
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || url.path().trim_end_matches('/') != "/v1"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("managed_gateway_url_invalid".to_string());
    }
    Ok(())
}

pub(super) fn validate_profile_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("managed_profile_name_invalid".to_string());
    }
    Ok(())
}

pub(super) fn write_immutable_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    reject_link(path)?;
    if path.exists() {
        let existing =
            fs::read(path).map_err(|error| format!("managed_file_read_failed:{error}"))?;
        return if existing == bytes {
            Ok(())
        } else {
            Err("managed_file_immutable".to_string())
        };
    }
    write_atomically(path, bytes, "managed_file", false)
}

pub(crate) fn write_private_atomically_overwrite(
    path: &Path,
    bytes: &[u8],
    label: &str,
) -> Result<(), String> {
    write_atomically(path, bytes, label, true)
}

pub(super) fn write_atomically(
    path: &Path,
    bytes: &[u8],
    label: &str,
    overwrite: bool,
) -> Result<(), String> {
    reject_link(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("{label}_parent_missing"))?;
    let temporary = parent.join(format!(".rcodex-{label}-{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("{label}_create_failed:{error}"))?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| format!("{label}_write_failed:{error}"))?;
        file.sync_all()
            .map_err(|error| format!("{label}_sync_failed:{error}"))?;
        if overwrite {
            #[cfg(windows)]
            if path.exists() {
                fs::remove_file(path).map_err(|error| format!("{label}_replace_failed:{error}"))?;
            }
            fs::rename(&temporary, path)
                .map_err(|error| format!("{label}_publish_failed:{error}"))?;
        } else {
            // Publishing through a hard link is exclusive, including races with
            // another writer after the initial existence check.
            fs::hard_link(&temporary, path)
                .map_err(|error| format!("{label}_publish_failed:{error}"))?;
            fs::remove_file(&temporary)
                .map_err(|error| format!("{label}_cleanup_failed:{error}"))?;
        }
        sync_parent_directory(path, label)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn reject_link(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("managed_file_symlink_rejected".to_string())
        }
        Ok(metadata) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() > 1 {
                    return Err("managed_file_hardlink_rejected".to_string());
                }
            }
            #[cfg(not(unix))]
            {
                let _ = &metadata;
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("managed_file_stat_failed".to_string()),
    }
}

#[cfg_attr(not(unix), allow(unused_variables))]
pub(super) fn sync_parent_directory(path: &Path, label: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .ok_or_else(|| format!("{label}_parent_missing"))?;
        OpenOptions::new()
            .read(true)
            .open(parent)
            .map_err(|error| format!("{label}_parent_open_failed:{error}"))?
            .sync_all()
            .map_err(|error| format!("{label}_parent_sync_failed:{error}"))?;
    }
    Ok(())
}

/// Paths written into isolated config.toml and MCP env.
///
/// Assembly and stringify share this contract so Windows and macOS do not
/// leak mixed separators or canonicalize()'s verbatim prefix into Codex.
pub(crate) fn join_config_path(
    base: impl AsRef<Path>,
    segments: impl IntoIterator<Item = impl AsRef<str>>,
) -> PathBuf {
    let mut assembled = normalize_config_path_text(&base.as_ref().to_string_lossy());
    for segment in segments {
        let segment = segment.as_ref();
        let normalized_segment = normalize_config_path_text(segment);
        if is_absolute_config_path(&normalized_segment) {
            assembled = normalized_segment;
            continue;
        }
        let separator = config_path_separator(&assembled);
        for part in config_path_parts(segment) {
            if assembled.ends_with(separator) || assembled.is_empty() {
                assembled.push_str(part);
            } else {
                assembled.push(separator);
                assembled.push_str(part);
            }
        }
        assembled = normalize_config_path_text(&assembled);
    }
    PathBuf::from(normalize_config_path_text(&assembled))
}

/// Write form of an assembled or canonical path.
pub(crate) fn config_path_text(path: &Path) -> String {
    normalize_config_path_text(&path.to_string_lossy())
}

/// Join isolated config paths for env lists such as NODE_REPL_TRUSTED_CODE_PATHS.
///
/// Windows-shaped paths use `;` so `C:\` drive letters stay intact. Unix uses `:`.
pub(crate) fn join_env_path_list(first: impl AsRef<str>, second: impl AsRef<str>) -> String {
    let first = normalize_config_path_text(first.as_ref());
    let second = normalize_config_path_text(second.as_ref());
    let separator = if is_windows_config_path(&first) || is_windows_config_path(&second) {
        ';'
    } else {
        ':'
    };
    format!("{first}{separator}{second}")
}

fn normalize_config_path_text(raw: &str) -> String {
    let stripped = strip_verbatim_prefix(raw);
    if is_windows_config_path(stripped) {
        stripped.replace('/', "\\")
    } else {
        stripped.to_string()
    }
}

fn strip_verbatim_prefix(raw: &str) -> &str {
    raw.strip_prefix(r"\\?\")
        .or_else(|| raw.strip_prefix("//?/"))
        .unwrap_or(raw)
}

fn is_windows_config_path(value: &str) -> bool {
    let stripped = strip_verbatim_prefix(value);
    stripped.contains('\\') || stripped.chars().nth(1) == Some(':')
}

fn is_absolute_config_path(value: &str) -> bool {
    let stripped = strip_verbatim_prefix(value);
    stripped.starts_with('\\') || stripped.starts_with('/') || stripped.chars().nth(1) == Some(':')
}

fn config_path_separator(value: &str) -> char {
    if is_windows_config_path(value) {
        '\\'
    } else {
        '/'
    }
}

fn config_path_parts(value: &str) -> impl Iterator<Item = &str> {
    strip_verbatim_prefix(value)
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .filter(|part| !is_windows_drive_component(part))
}

fn is_windows_drive_component(part: &str) -> bool {
    let bytes = part.as_bytes();
    bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

#[cfg(test)]
mod tests {
    use super::{join_config_path, join_env_path_list, normalize_config_path_text};
    use std::path::{Path, PathBuf};

    #[test]
    fn strips_verbatim_prefix_and_mixed_separators() {
        assert_eq!(
            normalize_config_path_text(r"\\?\C:\Users\a\codex-home\file.json"),
            r"C:\Users\a\codex-home\file.json",
        );
        assert_eq!(
            normalize_config_path_text(
                r"C:\Program Files\WindowsApps\OpenAI.Codex\app\resources/cua_node/bin/node.exe",
            ),
            r"C:\Program Files\WindowsApps\OpenAI.Codex\app\resources\cua_node\bin\node.exe",
        );
        assert_eq!(
            normalize_config_path_text("/tmp/isolated/config.toml"),
            "/tmp/isolated/config.toml",
        );
    }

    #[test]
    fn join_config_path_uses_the_path_platform_separator() {
        assert_eq!(
            join_config_path(
                Path::new(r"C:\Program Files\WindowsApps\OpenAI.Codex\app"),
                ["resources/cua_node/bin/node.exe"],
            ),
            PathBuf::from(
                r"C:\Program Files\WindowsApps\OpenAI.Codex\app\resources\cua_node\bin\node.exe",
            ),
        );
        assert_eq!(
            join_config_path(
                Path::new(r"\\?\C:\Users\a\codex-home"),
                ["managed_x.models.json"],
            ),
            PathBuf::from(r"C:\Users\a\codex-home\managed_x.models.json"),
        );
        assert_eq!(
            join_config_path(
                Path::new(r"C:\Users\a\codex-home"),
                ["computer-use/windows", "codex-computer-use-arm64.exe"],
            ),
            PathBuf::from(
                r"C:\Users\a\codex-home\computer-use\windows\codex-computer-use-arm64.exe",
            ),
        );
        assert_eq!(
            join_config_path(
                Path::new("/tmp/isolated"),
                ["computer-use/windows", "helper.exe"],
            ),
            PathBuf::from("/tmp/isolated/computer-use/windows/helper.exe"),
        );
        assert_eq!(
            join_config_path(
                Path::new(r"C:\Program Files\WindowsApps\OpenAI.Codex\app\resources/cua_node",),
                ["bin/node.exe"],
            ),
            PathBuf::from(
                r"C:\Program Files\WindowsApps\OpenAI.Codex\app\resources\cua_node\bin\node.exe",
            ),
        );
    }

    #[test]
    fn join_env_path_list_keeps_windows_drive_letters() {
        assert_eq!(
            join_env_path_list(
                r"C:\Users\a\codex-home",
                r"C:\Program Files\WindowsApps\OpenAI.Codex\app\resources\cua_node\bin",
            ),
            r"C:\Users\a\codex-home;C:\Program Files\WindowsApps\OpenAI.Codex\app\resources\cua_node\bin",
        );
        assert_eq!(
            join_env_path_list(
                "/Users/a/codex-home",
                "/Applications/ChatGPT.app/Contents/Resources/cua_node/bin",
            ),
            "/Users/a/codex-home:/Applications/ChatGPT.app/Contents/Resources/cua_node/bin",
        );
    }
}
