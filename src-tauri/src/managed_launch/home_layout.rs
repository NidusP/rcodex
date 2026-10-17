//! macOS 短路径隔离 home 与兼容链接。
//!
//! 不管 Electron 数据、不管活动对话库迁移。Unix socket 路径长度限制只在这里处理。
//! 由 prepare / 删除实例 / 揭示目录调用。

use std::path::{Path, PathBuf};

#[cfg(any(test, target_os = "macos"))]
use serde::{Deserialize, Serialize};

#[cfg(any(test, target_os = "macos"))]
use crate::managed_codex_config::write_private_atomically_overwrite;

use super::models::ManagedLaunchStore;
#[cfg(target_os = "macos")]
use super::storage::rewrite_rollout_paths_in_home;
use super::validation::{ensure_owned_path, set_private_directory};

pub(super) fn remove_owned_directory(
    root: &Path,
    path: &Path,
    error_code: &str,
) -> Result<(), String> {
    ensure_owned_path(root, path)?;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::remove_dir_all(path).map_err(|error| format!("{error_code}:{error}"))?;
        }
        Ok(_) => return Err(format!("{error_code}:not_directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{error_code}:{error}")),
    }
    Ok(())
}

#[cfg(any(test, target_os = "macos"))]
pub(super) const MANAGED_HOME_LAYOUT_FILE: &str = "home-layout.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg(any(test, target_os = "macos"))]
pub(super) enum ManagedHomeLayoutPhase {
    Legacy,
    Renaming,
    Linking,
    RewritingPaths,
    UpdatingDb,
    Complete,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg(any(test, target_os = "macos"))]
pub(super) struct ManagedHomeLayoutRecord {
    pub(super) instance_id: String,
    pub(super) long_home: String,
    pub(super) short_home: String,
    pub(super) phase: ManagedHomeLayoutPhase,
}

#[cfg(any(test, target_os = "macos"))]
pub(super) fn read_managed_home_layout(
    path: &Path,
) -> Result<Option<ManagedHomeLayoutRecord>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("managed_home_layout_read_failed:{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("managed_home_layout_invalid".to_string());
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("managed_home_layout_read_failed:{error}"))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "managed_home_layout_invalid".to_string())
}

#[cfg(any(test, target_os = "macos"))]
pub(super) fn write_managed_home_layout(
    path: &Path,
    record: &ManagedHomeLayoutRecord,
) -> Result<(), String> {
    let encoded = serde_json::to_vec(record)
        .map_err(|error| format!("managed_home_layout_encode_failed:{error}"))?;
    write_private_atomically_overwrite(path, &encoded, "managed_home_layout")
}

impl ManagedLaunchStore {
    #[cfg(any(test, target_os = "macos"))]
    pub(super) fn managed_app_short_root(&self) -> Result<PathBuf, String> {
        let root = self
            .instances_root
            .parent()
            .ok_or_else(|| "managed_codex_home_root_invalid".to_string())?
            .join("r");
        ensure_owned_path(self.instances_root.parent().unwrap(), &root)?;
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("managed_codex_short_root_create_failed:{error}"))?;
        set_private_directory(&root)?;
        Ok(root)
    }

    #[cfg(any(test, target_os = "macos"))]
    pub(super) fn short_home_name_matches_instance(instance_id: &str, name: &str) -> bool {
        let compact = instance_id.replace('-', "");
        [12, 16, 24, 32].iter().any(|length| {
            compact
                .get(..*length)
                .is_some_and(|expected| expected == name)
        })
    }

    #[cfg(any(test, target_os = "macos"))]
    pub(super) fn ensure_managed_app_short_home(
        &self,
        instance_id: &str,
        short_home: &Path,
    ) -> Result<(), String> {
        let root = self.managed_app_short_root()?;
        if short_home.parent() != Some(root.as_path())
            || !short_home
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| Self::short_home_name_matches_instance(instance_id, name))
        {
            return Err("managed_home_layout_conflict".to_string());
        }
        ensure_owned_path(self.instances_root.parent().unwrap(), short_home)?;
        match std::fs::symlink_metadata(short_home) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
            Ok(_) => Err("managed_home_layout_conflict".to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err("managed_home_layout_conflict".to_string())
            }
            Err(error) => Err(format!("managed_home_layout_check_failed:{error}")),
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    pub(super) fn managed_app_short_home_candidate(
        &self,
        instance_id: &str,
        legacy_long_home: Option<&Path>,
    ) -> Result<PathBuf, String> {
        let root = self.managed_app_short_root()?;
        let compact = instance_id.replace('-', "");
        let legacy_canonical = legacy_long_home.and_then(|path| path.canonicalize().ok());
        for length in [12, 16, 24, 32] {
            let name = compact
                .get(..length)
                .ok_or_else(|| "managed_codex_instance_id_invalid".to_string())?;
            let candidate = root.join(name);
            match std::fs::symlink_metadata(&candidate) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    if let (Some(expected), Ok(resolved)) =
                        (legacy_canonical.as_ref(), candidate.canonicalize())
                    {
                        if &resolved == expected {
                            return Ok(candidate);
                        }
                    }
                }
                Ok(_) | Err(_) => {}
            }
        }
        Err("managed_codex_short_link_collision".to_string())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn link_resolves_to(link: &Path, target: &Path) -> bool {
        match (link.canonicalize(), target.canonicalize()) {
            (Ok(link), Ok(target)) => link == target,
            _ => false,
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn ensure_managed_app_compat_link(
        &self,
        long_home: &Path,
        short_home: &Path,
    ) -> Result<(), String> {
        match std::fs::symlink_metadata(long_home) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::os::unix::fs::symlink(short_home, long_home)
                    .map_err(|error| format!("managed_home_layout_link_failed:{error}"))?;
                Ok(())
            }
            Ok(metadata)
                if metadata.file_type().is_symlink()
                    && Self::link_resolves_to(long_home, short_home) =>
            {
                Ok(())
            }
            Ok(_) => Err("managed_home_layout_conflict".to_string()),
            Err(error) => Err(format!("managed_home_layout_link_failed:{error}")),
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn prepare_macos_managed_app_home_layout(
        &self,
        instance_id: &str,
        lifecycle_dir: &Path,
        stored_home: Option<&Path>,
    ) -> Result<(PathBuf, PathBuf, Option<PathBuf>), String> {
        let long_home = lifecycle_dir.join("codex-home");
        let marker = lifecycle_dir.join(MANAGED_HOME_LAYOUT_FILE);
        let existing_marker = read_managed_home_layout(&marker)?;

        if let Some(record) = existing_marker {
            if record.instance_id != instance_id || Path::new(&record.long_home) != long_home {
                return Err("managed_home_layout_conflict".to_string());
            }
            let short_home = PathBuf::from(&record.short_home);
            if stored_home == Some(short_home.as_path()) {
                if !matches!(
                    record.phase,
                    ManagedHomeLayoutPhase::UpdatingDb | ManagedHomeLayoutPhase::Complete
                ) {
                    return Err("managed_home_layout_conflict".to_string());
                }
                self.ensure_managed_app_short_home(instance_id, &short_home)?;
                self.ensure_managed_app_compat_link(&long_home, &short_home)?;
                let _ = self.complete_managed_app_home_layout(&marker);
                return Ok((short_home.clone(), short_home, None));
            }
            if stored_home != Some(long_home.as_path()) {
                return Err("managed_home_layout_conflict".to_string());
            }
            return self.resume_macos_managed_app_home_layout(&marker, record);
        }

        match stored_home {
            None => {
                let short_home = self.managed_app_short_home_candidate(instance_id, None)?;
                if std::fs::symlink_metadata(&long_home).is_ok() {
                    return Err("managed_home_layout_conflict".to_string());
                }
                std::fs::create_dir_all(&short_home)
                    .map_err(|error| format!("managed_codex_home_create_failed:{error}"))?;
                set_private_directory(&short_home)?;
                self.ensure_managed_app_short_home(instance_id, &short_home)?;
                self.ensure_managed_app_compat_link(&long_home, &short_home)?;
                Ok((short_home.clone(), short_home, None))
            }
            Some(home) if home == long_home => {
                ensure_owned_path(&self.instances_root, &long_home)?;
                let short_home =
                    self.managed_app_short_home_candidate(instance_id, Some(&long_home))?;
                let record = ManagedHomeLayoutRecord {
                    instance_id: instance_id.to_string(),
                    long_home: long_home.display().to_string(),
                    short_home: short_home.display().to_string(),
                    phase: ManagedHomeLayoutPhase::Legacy,
                };
                write_managed_home_layout(&marker, &record)?;
                self.resume_macos_managed_app_home_layout(&marker, record)
            }
            Some(home) if self.is_managed_app_short_home_path(instance_id, home)? => {
                self.ensure_managed_app_short_home(instance_id, home)?;
                self.ensure_managed_app_compat_link(&long_home, home)?;
                Ok((home.to_path_buf(), home.to_path_buf(), None))
            }
            Some(_) => Err("managed_home_layout_conflict".to_string()),
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn is_managed_app_short_home_path(
        &self,
        instance_id: &str,
        path: &Path,
    ) -> Result<bool, String> {
        let root = self.managed_app_short_root()?;
        Ok(path.parent() == Some(root.as_path())
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| Self::short_home_name_matches_instance(instance_id, name)))
    }

    #[cfg(target_os = "macos")]
    pub(super) fn resume_macos_managed_app_home_layout(
        &self,
        marker: &Path,
        mut record: ManagedHomeLayoutRecord,
    ) -> Result<(PathBuf, PathBuf, Option<PathBuf>), String> {
        let long_home = PathBuf::from(&record.long_home);
        let short_home = PathBuf::from(&record.short_home);
        loop {
            match record.phase {
                ManagedHomeLayoutPhase::Legacy => {
                    record.phase = ManagedHomeLayoutPhase::Renaming;
                    write_managed_home_layout(marker, &record)?;
                }
                ManagedHomeLayoutPhase::Renaming => {
                    let long = std::fs::symlink_metadata(&long_home).ok();
                    let short = std::fs::symlink_metadata(&short_home).ok();
                    match (long, short) {
                        (Some(long), None) if long.is_dir() && !long.file_type().is_symlink() => {
                            std::fs::rename(&long_home, &short_home).map_err(|error| {
                                format!("managed_home_layout_rename_failed:{error}")
                            })?;
                        }
                        (Some(long), Some(short))
                            if long.is_dir()
                                && !long.file_type().is_symlink()
                                && short.file_type().is_symlink()
                                && Self::link_resolves_to(&short_home, &long_home) =>
                        {
                            std::fs::remove_file(&short_home).map_err(|error| {
                                format!("managed_home_layout_unlink_failed:{error}")
                            })?;
                            std::fs::rename(&long_home, &short_home).map_err(|error| {
                                format!("managed_home_layout_rename_failed:{error}")
                            })?;
                        }
                        (None, Some(short))
                            if short.is_dir() && !short.file_type().is_symlink() => {}
                        _ => return Err("managed_home_layout_conflict".to_string()),
                    }
                    record.phase = ManagedHomeLayoutPhase::Linking;
                    write_managed_home_layout(marker, &record)?;
                }
                ManagedHomeLayoutPhase::Linking => {
                    self.ensure_managed_app_short_home(&record.instance_id, &short_home)?;
                    self.ensure_managed_app_compat_link(&long_home, &short_home)?;
                    record.phase = ManagedHomeLayoutPhase::RewritingPaths;
                    write_managed_home_layout(marker, &record)?;
                }
                ManagedHomeLayoutPhase::RewritingPaths => {
                    self.ensure_managed_app_short_home(&record.instance_id, &short_home)?;
                    self.ensure_managed_app_compat_link(&long_home, &short_home)?;
                    rewrite_rollout_paths_in_home(&long_home, &short_home)?;
                    record.phase = ManagedHomeLayoutPhase::UpdatingDb;
                    write_managed_home_layout(marker, &record)?;
                    return Ok((short_home.clone(), short_home, Some(marker.to_path_buf())));
                }
                ManagedHomeLayoutPhase::UpdatingDb => {
                    self.ensure_managed_app_short_home(&record.instance_id, &short_home)?;
                    self.ensure_managed_app_compat_link(&long_home, &short_home)?;
                    return Ok((short_home.clone(), short_home, Some(marker.to_path_buf())));
                }
                ManagedHomeLayoutPhase::Complete => {
                    self.ensure_managed_app_short_home(&record.instance_id, &short_home)?;
                    self.ensure_managed_app_compat_link(&long_home, &short_home)?;
                    let _ = std::fs::remove_file(marker);
                    return Ok((short_home.clone(), short_home, None));
                }
            }
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    pub(super) fn prepare_managed_app_home_layout(
        &self,
        instance_id: &str,
        lifecycle_dir: &Path,
        stored_home: Option<&Path>,
    ) -> Result<(PathBuf, PathBuf, Option<PathBuf>), String> {
        #[cfg(target_os = "macos")]
        {
            return self.prepare_macos_managed_app_home_layout(
                instance_id,
                lifecycle_dir,
                stored_home,
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            let managed_codex_home = stored_home
                .map(Path::to_path_buf)
                .unwrap_or_else(|| lifecycle_dir.join("codex-home"));
            ensure_owned_path(&self.instances_root, &managed_codex_home)?;
            std::fs::create_dir_all(&managed_codex_home)
                .map_err(|error| format!("managed_codex_home_create_failed:{error}"))?;
            set_private_directory(&managed_codex_home)?;
            Ok((managed_codex_home.clone(), managed_codex_home, None))
        }
    }

    pub(super) fn complete_managed_app_home_layout(&self, marker: &Path) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let Some(mut record) = read_managed_home_layout(marker)? else {
                return Ok(());
            };
            record.phase = ManagedHomeLayoutPhase::Complete;
            write_managed_home_layout(marker, &record)?;
            std::fs::remove_file(marker)
                .map_err(|error| format!("managed_home_layout_cleanup_failed:{error}"))?;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = marker;
        Ok(())
    }

    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    pub(super) fn remove_managed_app_home_for_delete(
        &self,
        instance_id: &str,
        lifecycle_dir: &Path,
        managed_codex_home: &Path,
    ) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        if self.is_managed_app_short_home_path(instance_id, managed_codex_home)? {
            self.ensure_managed_app_short_home(instance_id, managed_codex_home)?;
            let long_home = lifecycle_dir.join("codex-home");
            match std::fs::symlink_metadata(&long_home) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(metadata)
                    if metadata.file_type().is_symlink()
                        && Self::link_resolves_to(&long_home, managed_codex_home) =>
                {
                    std::fs::remove_file(&long_home)
                        .map_err(|error| format!("managed_home_layout_cleanup_failed:{error}"))?;
                }
                Ok(_) => return Err("managed_home_layout_conflict".to_string()),
                Err(error) => return Err(format!("managed_home_layout_cleanup_failed:{error}")),
            }
            remove_owned_directory(
                self.instances_root.parent().unwrap(),
                managed_codex_home,
                "managed_instance_storage_cleanup_failed",
            )?;
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        self.remove_legacy_managed_app_short_link_for_delete(instance_id, managed_codex_home)?;
        ensure_owned_path(&self.instances_root, managed_codex_home)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn remove_legacy_managed_app_short_link_for_delete(
        &self,
        instance_id: &str,
        long_home: &Path,
    ) -> Result<(), String> {
        let root = self.managed_app_short_root()?;
        let compact = instance_id.replace('-', "");
        if compact.len() < 12 {
            return Ok(());
        }
        for length in [12, 16, 24, 32] {
            let Some(name) = compact.get(..length) else {
                continue;
            };
            let candidate = root.join(name);
            let metadata = match std::fs::symlink_metadata(&candidate) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("managed_home_layout_cleanup_failed:{error}")),
            };
            if metadata.file_type().is_symlink() && Self::link_resolves_to(&candidate, long_home) {
                std::fs::remove_file(&candidate)
                    .map_err(|error| format!("managed_home_layout_cleanup_failed:{error}"))?;
            }
        }
        Ok(())
    }

    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    pub(super) fn ensure_isolated_instance_home(
        &self,
        instance_id: &str,
        home: &Path,
    ) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        if self.is_managed_app_short_home_path(instance_id, home)? {
            return self.ensure_managed_app_short_home(instance_id, home);
        }
        ensure_owned_path(&self.instances_root, home)
    }

    /// CLI 是否用短 symlink 指向 lifecycle/codex-home。
    ///
    /// macOS 需要短路径，因为 Unix socket 有长度限制。Windows 创建目录
    /// symlink 需要管理员或开发人员模式；隔离 App 已经直接用长路径，CLI
    /// 同样不再要求 symlink。
    pub(super) const fn cli_uses_short_home_link() -> bool {
        !cfg!(target_os = "windows")
    }

    pub(super) fn resolve_cli_launch_codex_home(
        &self,
        instance_id: &str,
        managed_codex_home: &Path,
    ) -> Result<PathBuf, String> {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = Self::cli_uses_short_home_link();
            self.short_codex_home_link(instance_id, managed_codex_home)
        }
        #[cfg(target_os = "windows")]
        {
            let _ = (instance_id, Self::cli_uses_short_home_link());
            Ok(managed_codex_home.to_path_buf())
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub(super) fn short_codex_home_link(
        &self,
        instance_id: &str,
        target: &Path,
    ) -> Result<PathBuf, String> {
        let root = self
            .instances_root
            .parent()
            .ok_or_else(|| "managed_codex_home_root_invalid".to_string())?
            .join("r");
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("managed_codex_short_root_create_failed:{error}"))?;
        set_private_directory(&root)?;
        let compact = instance_id.replace('-', "");
        for length in [12, 16, 24, 32] {
            let name = compact
                .get(..length)
                .ok_or_else(|| "managed_codex_instance_id_invalid".to_string())?;
            let link = root.join(name);
            if link.exists() {
                continue;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &link)
                .map_err(|error| format!("managed_codex_short_link_failed:{error}"))?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(target, &link)
                .map_err(|error| format!("managed_codex_short_link_failed:{error}"))?;
            return Ok(link);
        }
        Err("managed_codex_short_link_collision".to_string())
    }
}
