//! 启动方案名称、工作区路径和私有目录约束。
//!
//! 不读渠道凭据，不管模型合同。各写入路径共用。

use std::path::Path;

pub(super) use crate::support::{resolve_workspace, set_private_directory, sha256_label};

pub(super) fn validate_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 80 || value.chars().any(char::is_control) {
        return Err("invalid_launch_preset_name".to_string());
    }
    Ok(value.to_string())
}

pub(super) fn normalize_optional_path(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn ensure_owned_path(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "managed_path_outside_owned_root".to_string())?;
    let mut current = root.to_path_buf();
    for component in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(component) = component {
            if !matches!(component, std::path::Component::Normal(_)) {
                return Err("managed_path_outside_owned_root".to_string());
            }
            current.push(component);
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("managed_path_link_rejected".to_string());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("managed_path_unreadable".to_string()),
        }
    }
    Ok(())
}
