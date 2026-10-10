//! Electron 数据目录、活动对话库路径和实例目录删除。
//!
//! 不改工作区 composition，不处理 macOS 短路径布局。遗留 managed-app-sqlite 只迁进当前 home。
//! 由 prepare、删除实例和测试夹具调用。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::codex_control::InstanceState;
use crate::launcher_protocol::LaunchTarget;

use super::home_layout::remove_owned_directory;
use super::models::ManagedLaunchStore;
use super::validation::{ensure_owned_path, set_private_directory};

pub(super) const LEGACY_SQLITE_FILE_PREFIXES: [&str; 6] = [
    "state_",
    "logs_",
    "memories_",
    "goals_",
    "queue_",
    "thread_history_",
];

pub(super) fn quote_sqlite_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Rewrites persisted rollout paths only when the old home is the complete
/// path component prefix. Repeating the update after an interrupted migration
/// is safe because rewritten rows no longer match the old prefix.
pub(super) fn rewrite_rollout_paths_in_home(
    old_home: &Path,
    new_home: &Path,
) -> Result<(), String> {
    let old_prefix = old_home.display().to_string();
    let new_prefix = new_home.display().to_string();
    let entries = std::fs::read_dir(new_home)
        .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?;
    let mut state_databases = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?;
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?;
        if metadata.file_type().is_file() && is_state_sqlite_file(&entry.path()) {
            state_databases.push(entry.path());
        }
    }
    state_databases.sort();

    for database_path in state_databases {
        let mut connection = Connection::open(&database_path)
            .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?;
        let table_names = {
            let mut statement = transaction
                .prepare("SELECT name FROM sqlite_master WHERE type='table'")
                .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?;
            let names = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?;
            names
        };
        for table_name in table_names {
            let table = quote_sqlite_identifier(&table_name);
            let has_rollout_path = {
                let mut statement = transaction
                    .prepare(&format!("PRAGMA table_info({table})"))
                    .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?;
                let columns = statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?
                    .iter()
                    .any(|column| column == "rollout_path");
                columns
            };
            if !has_rollout_path {
                continue;
            }
            transaction
                .execute(
                    &format!(
                        "UPDATE {table}
                         SET \"rollout_path\"=?2 || substr(\"rollout_path\", length(?1) + 1)
                         WHERE substr(\"rollout_path\", 1, length(?1))=?1
                           AND (
                             length(\"rollout_path\")=length(?1)
                             OR substr(\"rollout_path\", length(?1) + 1, 1)='/'
                           )"
                    ),
                    params![old_prefix, new_prefix],
                )
                .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("managed_home_rollout_rewrite_failed:{error}"))?;
    }
    Ok(())
}

pub(super) fn legacy_sqlite_base_name(name: &str) -> Option<&str> {
    let base = name
        .strip_suffix("-wal")
        .or_else(|| name.strip_suffix("-shm"))
        .unwrap_or(name);
    if !base.ends_with(".sqlite")
        || !LEGACY_SQLITE_FILE_PREFIXES
            .iter()
            .any(|prefix| base.starts_with(prefix))
    {
        return None;
    }
    Some(base)
}

pub(super) fn legacy_sqlite_group_name(path: &Path) -> Option<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(legacy_sqlite_base_name)
}

pub(super) fn legacy_sqlite_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("managed_sqlite_migrate_failed:{error}")),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("managed_sqlite_migrate_failed:{error}"))?;
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|error| format!("managed_sqlite_migrate_failed:{error}"))?;
        if legacy_sqlite_base_name(&entry.file_name().to_string_lossy()).is_some() {
            if !metadata.file_type().is_file() {
                return Err("managed_sqlite_migrate_failed:invalid_file".to_string());
            }
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

pub(super) fn is_state_sqlite_file(path: &Path) -> bool {
    let Some(base) = legacy_sqlite_group_name(path) else {
        return false;
    };
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    base.starts_with("state_") && name == base
}

/// Move the legacy per-preset SQLite files into the instance Codex home.
/// Both roots are owned by R Codex, so renaming the database and its WAL/SHM
/// companions keeps the migration single-source and avoids copy-on-write
/// divergence. The caller must ensure the managed App is not active.
pub(super) fn migrate_managed_app_sqlite_into_home(
    old_sqlite_home: &Path,
    dest_home: &Path,
) -> Result<(), String> {
    let old_files = legacy_sqlite_files(old_sqlite_home)?;
    if old_files.is_empty() {
        return Ok(());
    }
    let dest_files = legacy_sqlite_files(dest_home)?;
    let old_has_state = old_files.iter().any(|path| is_state_sqlite_file(path));
    let dest_has_state = dest_files.iter().any(|path| is_state_sqlite_file(path));
    if old_has_state && dest_has_state {
        return Err("managed_sqlite_layout_conflict".to_string());
    }

    std::fs::create_dir_all(dest_home)
        .map_err(|error| format!("managed_sqlite_migrate_failed:{error}"))?;
    let mut destinations = Vec::with_capacity(old_files.len());
    for source in &old_files {
        if dest_files.iter().any(|destination| {
            legacy_sqlite_group_name(destination) == legacy_sqlite_group_name(source)
        }) {
            return Err("managed_sqlite_layout_conflict".to_string());
        }
        let file_name = source
            .file_name()
            .ok_or_else(|| "managed_sqlite_migrate_failed:invalid_file_name".to_string())?;
        let destination = dest_home.join(file_name);
        if destination.exists() {
            return Err("managed_sqlite_layout_conflict".to_string());
        }
        destinations.push(destination);
    }

    let mut moved = Vec::with_capacity(old_files.len());
    for (source, destination) in old_files.iter().zip(destinations.iter()) {
        if let Err(error) = std::fs::rename(source, destination) {
            for (moved_source, moved_destination) in moved.into_iter().rev() {
                let _ = std::fs::rename(moved_destination, moved_source);
            }
            return Err(format!("managed_sqlite_migrate_failed:{error}"));
        }
        moved.push((source, destination));
    }

    let _ = std::fs::remove_dir(old_sqlite_home);
    Ok(())
}

impl ManagedLaunchStore {
    pub(super) fn managed_app_electron_data_dir(
        &self,
        lifecycle_dir: &Path,
    ) -> Result<PathBuf, String> {
        ensure_owned_path(&self.instances_root, lifecycle_dir)?;
        std::fs::create_dir_all(lifecycle_dir)
            .map_err(|error| format!("managed_lifecycle_dir_create_failed:{error}"))?;
        let dir = lifecycle_dir.join("electron-data");
        ensure_owned_path(&self.instances_root, &dir)?;
        if !dir.exists() {
            if let Some(parent) = self.instances_root.parent() {
                let legacy = parent.join("managed-app-data");
                if legacy.is_dir() {
                    // The historical shared root is adopted by the first App
                    // that launches after this change. Later Apps start empty.
                    let _ = std::fs::rename(&legacy, &dir);
                }
            }
        }
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("managed_electron_data_dir_failed:{error}"))?;
        set_private_directory(&dir)?;
        Ok(dir)
    }

    pub(super) fn managed_app_sqlite_path(
        &self,
        launch_preset_id: &str,
    ) -> Result<PathBuf, String> {
        if launch_preset_id.is_empty()
            || !launch_preset_id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err("managed_sqlite_home_id_invalid".to_string());
        }
        let root = self
            .instances_root
            .parent()
            .ok_or_else(|| "managed_sqlite_root_invalid".to_string())?
            .join("managed-app-sqlite")
            .join(launch_preset_id);
        ensure_owned_path(self.instances_root.parent().unwrap(), &root)?;
        Ok(root)
    }

    /// Isolated Codex home for a managed-app preset. None until the user has
    /// launched once from R Codex, because the home is allocated at first launch.
    pub fn isolated_codex_home_for_preset(
        &self,
        launch_preset_id: &str,
    ) -> Result<PathBuf, String> {
        let (instance_id, home, _, state) = self
            .managed_app_runtime_binding(launch_preset_id, "managed_app")?
            .ok_or_else(|| "isolated_home_not_ready".to_string())?;
        if !InstanceState::is_finished(state.as_str()) {
            return Err("managed_app_in_use".to_string());
        }
        self.ensure_isolated_instance_home(&instance_id, &home)?;
        if !home.is_dir() {
            return Err("isolated_home_not_ready".to_string());
        }
        Ok(home)
    }

    /// Finder reveal of the reused managed-app home. Running is allowed;
    /// import still uses the stricter exited-only lookup above.
    pub fn isolated_codex_home_for_reveal(
        &self,
        launch_preset_id: &str,
    ) -> Result<PathBuf, String> {
        let (instance_id, home, _, _) = self
            .managed_app_runtime_binding(launch_preset_id, "managed_app")?
            .ok_or_else(|| "isolated_home_not_ready".to_string())?;
        self.ensure_isolated_instance_home(&instance_id, &home)?;
        if !home.is_dir() {
            return Err("isolated_home_not_ready".to_string());
        }
        Ok(home)
    }

    pub fn isolated_codex_storage_for_preset(
        &self,
        launch_preset_id: &str,
    ) -> Result<(PathBuf, PathBuf), String> {
        let home = self.isolated_codex_home_for_preset(launch_preset_id)?;
        Ok((home.clone(), home))
    }

    pub fn delete_instance_storage(&self, instance_id: &str) -> Result<(), String> {
        let (
            stored_instance_id,
            managed_codex_home,
            lifecycle_dir,
            launch_target,
            launch_preset_id,
        ) = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
            connection
                .query_row(
                    "SELECT instance.id, instance.managed_codex_home, instance.lifecycle_dir,
                            COALESCE(json_extract(configuration.config_snapshot, '$.launchTarget'), preset.entry_kind),
                            configuration.launch_preset_id
                     FROM codex_instances instance
                     JOIN launch_configuration_revisions configuration
                       ON configuration.id=instance.launch_configuration_revision_id
                     LEFT JOIN launch_presets preset
                       ON preset.id=configuration.launch_preset_id
                     WHERE instance.id=?1",
                    [instance_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            PathBuf::from(row.get::<_, String>(1)?),
                            PathBuf::from(row.get::<_, String>(2)?),
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| format!("managed_instance_storage_read_failed:{error}"))?
                .ok_or_else(|| "codex_instance_not_found".to_string())?
        };
        if lifecycle_dir.parent() != Some(self.instances_root.as_path()) {
            return Err("instance_lifecycle_path_invalid".to_string());
        }
        ensure_owned_path(&self.instances_root, &lifecycle_dir)?;

        if LaunchTarget::wire_is_managed_app(&launch_target) {
            let launch_preset_id = launch_preset_id
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "managed_app_preset_missing".to_string())?;
            let legacy_sqlite = self.managed_app_sqlite_path(&launch_preset_id)?;
            remove_owned_directory(
                self.instances_root.parent().unwrap(),
                &legacy_sqlite,
                "managed_sqlite_home_cleanup_failed",
            )?;
        }
        if LaunchTarget::wire_is_managed_app(&launch_target) {
            self.remove_managed_app_home_for_delete(
                &stored_instance_id,
                &lifecycle_dir,
                &managed_codex_home,
            )?;
        } else {
            ensure_owned_path(&self.instances_root, &managed_codex_home)?;
        }
        remove_owned_directory(
            &self.instances_root,
            &lifecycle_dir,
            "managed_instance_storage_cleanup_failed",
        )?;
        Ok(())
    }
}
