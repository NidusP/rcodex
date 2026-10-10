//! 工作目录解析和私有目录权限。
//!
//! 只处理字面相同的路径约束，不创建业务目录布局，不读写 SQLite。
//! 由受管理启动和实例控制面共用。

use std::path::{Path, PathBuf};

/// 把 `~` 或相对主目录路径展开成绝对工作目录。
/// 空值表示未指定；文件或无法规范化的路径返回现有错误文案。
pub fn resolve_workspace(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let path = if value == "~" {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "无法定位用户主目录。".to_string())?
    } else if let Some(relative) = value.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "无法定位用户主目录。".to_string())?
            .join(relative)
    } else {
        PathBuf::from(value)
    };
    let path = path
        .canonicalize()
        .map_err(|error| format!("Codex 工作目录不可用：{}（{error}）", path.display()))?;
    if !path.is_dir() {
        return Err(format!("Codex 工作目录不是文件夹：{}", path.display()));
    }
    Ok(Some(path.display().to_string()))
}

/// 把目录权限设为 `0o700`。非 Unix 平台是空操作。
pub fn set_private_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("无法保护受管理目录：{error}"))?;
    }
    Ok(())
}
