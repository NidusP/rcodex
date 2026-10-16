//! 隔离配置的原子写、符号链接拒绝和 URL/名称校验。
//!
//! 不解释模型目录、不改偏好。isolated_auth 和各物化路径共用这些写入原语。

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
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
