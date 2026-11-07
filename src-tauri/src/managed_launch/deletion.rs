//! Permanent workspace deletion. The caller holds the launch/import mutex and
//! purges Gateway/archive state between prepare and storage/database commit.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use crate::database::resource_guard::{
    acquire_operation_leases_in_transaction, ResourceKind, ResourceRef,
};
use crate::support::{process_exists, unix_now_seconds};

use super::models::ManagedLaunchStore;
use super::validation::ensure_owned_path;

pub(super) const EXPLICITLY_EMPTY_WORKSPACE_KEY: &str = "managed_workspace_explicitly_empty";
const DELETION_OPERATION: &str = "delete_managed_workspace";

pub struct WorkspaceDeletionPlan {
    pub launch_preset_id: String,
    pub expected_version: i64,
    pub instance_ids: Vec<String>,
    operation_id: String,
    instances: Vec<InstanceStorage>,
    paths: Vec<OwnedPath>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InstanceStorage {
    id: String,
    home: PathBuf,
    lifecycle: PathBuf,
    target: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OwnedPath {
    path: PathBuf,
    /// Only exact, product-owned aliases may be unlinked.
    link_target: Option<PathBuf>,
}

impl ManagedLaunchStore {
    pub fn prepare_workspace_deletion(
        &self,
        launch_preset_id: &str,
        expected_version: i64,
    ) -> Result<WorkspaceDeletionPlan, String> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed")?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("workspace_delete_transaction_failed:{error}"))?;
        verify_workspace(&transaction, launch_preset_id, expected_version)?;
        let instances = read_instances(&transaction, launch_preset_id)?;
        let paths = self.workspace_deletion_paths(&transaction, launch_preset_id, &instances)?;
        // A crashed control process cannot leave the workspace undeletable.
        // A live owner is never displaced, even if its PID is this process.
        let prior: Option<(String, Option<u32>)> = transaction
            .query_row(
                "SELECT operation_kind, owner_pid FROM resource_operation_leases
             WHERE resource_kind='launch_preset' AND resource_id=?1",
                [launch_preset_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| format!("workspace_delete_lease_read_failed:{error}"))?;
        if let Some((kind, owner)) = prior {
            if kind == DELETION_OPERATION && owner.is_some_and(|pid| !process_exists(pid)) {
                transaction.execute("DELETE FROM resource_operation_leases WHERE resource_kind='launch_preset' AND resource_id=?1", [launch_preset_id])
                    .map_err(|error| format!("workspace_delete_lease_recovery_failed:{error}"))?;
            }
        }
        let operation_id = Uuid::new_v4().to_string();
        acquire_operation_leases_in_transaction(
            &transaction,
            &operation_id,
            DELETION_OPERATION,
            Some(std::process::id() as i64),
            None,
            &[ResourceRef::logical(
                ResourceKind::LaunchPreset,
                launch_preset_id,
            )],
            unix_now_seconds(),
        )
        .map_err(|error| format!("{}:{}", error.code, error.message))?;
        transaction.execute(
            "INSERT INTO provider_migration_state(key,value) VALUES (?1,'true') ON CONFLICT(key) DO UPDATE SET value='true'",
            [format!("managed_workspace_deleting:{launch_preset_id}")],
        ).map_err(|error| format!("workspace_delete_intent_failed:{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("workspace_delete_prepare_failed:{error}"))?;
        Ok(WorkspaceDeletionPlan {
            launch_preset_id: launch_preset_id.to_string(),
            expected_version,
            instance_ids: instances
                .iter()
                .map(|instance| instance.id.clone())
                .collect(),
            operation_id,
            instances,
            paths,
        })
    }

    pub fn release_workspace_deletion(&self, plan: &WorkspaceDeletionPlan) -> Result<(), String> {
        self.connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed")?
            .execute(
                "DELETE FROM resource_operation_leases WHERE operation_id=?1 AND operation_kind=?2",
                params![plan.operation_id, DELETION_OPERATION],
            )
            .map_err(|error| format!("workspace_delete_lease_release_failed:{error}"))?;
        Ok(())
    }

    pub fn delete_workspace_storage(&self, plan: &WorkspaceDeletionPlan) -> Result<(), String> {
        {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "managed_launch_database_lock_failed")?;
            verify_plan(&connection, plan)?;
            let current_paths = self.workspace_deletion_paths(
                &connection,
                &plan.launch_preset_id,
                &plan.instances,
            )?;
            // Missing paths are expected on retry, but a newly discovered path
            // must get another prepare pass before anything else is deleted.
            if current_paths.iter().any(|path| !plan.paths.contains(path)) {
                return Err("workspace_delete_storage_changed".to_string());
            }
        }
        let owned_root = self
            .instances_root
            .parent()
            .ok_or("managed_codex_home_root_invalid")?;
        for path in &plan.paths {
            validate_owned_path(owned_root, path)?;
        }
        for path in &plan.paths {
            validate_owned_path(owned_root, path)?;
            let result = if path.link_target.is_some() {
                std::fs::remove_file(&path.path)
            } else {
                std::fs::remove_dir_all(&path.path)
            };
            match result {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("workspace_delete_storage_failed:部分空间数据可能已删除，请保留空间记录后重试：{error}")),
            }
        }
        Ok(())
    }

    pub fn finish_workspace_deletion(
        &self,
        plan: &WorkspaceDeletionPlan,
    ) -> Result<Option<String>, String> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed")?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("workspace_delete_transaction_failed:{error}"))?;
        verify_plan(&transaction, plan)?;
        for path in &plan.paths {
            match std::fs::symlink_metadata(&path.path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err("workspace_delete_storage_remaining".to_string()),
            }
        }
        let gateway_ids = strings(&transaction,
            "SELECT DISTINCT gateway_service_revision_id FROM route_set_revisions WHERE launch_preset_id=?1", &plan.launch_preset_id)?;
        // Role deletion must precede selections because the selection trigger
        // deliberately forbids deleting a currently selected review role.
        for sql in [
            "DELETE FROM launch_preset_role_models WHERE launch_preset_id=?1",
            "DELETE FROM launch_preset_model_selections WHERE launch_preset_id=?1",
            "DELETE FROM launch_preset_runtime_bindings WHERE launch_preset_id=?1",
            "DELETE FROM instance_gateway_bindings WHERE instance_id IN (SELECT instance.id FROM codex_instances instance JOIN launch_configuration_revisions config ON config.id=instance.launch_configuration_revision_id WHERE config.launch_preset_id=?1)",
            "DELETE FROM runtime_configuration_locks WHERE instance_id IN (SELECT instance.id FROM codex_instances instance JOIN launch_configuration_revisions config ON config.id=instance.launch_configuration_revision_id WHERE config.launch_preset_id=?1)",
            "DELETE FROM codex_instances WHERE launch_configuration_revision_id IN (SELECT id FROM launch_configuration_revisions WHERE launch_preset_id=?1)",
            "DELETE FROM launch_configuration_route_sets WHERE launch_configuration_revision_id IN (SELECT id FROM launch_configuration_revisions WHERE launch_preset_id=?1)",
            "DELETE FROM launch_configuration_revisions WHERE launch_preset_id=?1",
            "DELETE FROM compatibility_gateway_bindings WHERE launch_preset_id=?1",
            "DELETE FROM route_set_revisions WHERE launch_preset_id=?1",
            "DELETE FROM launch_presets WHERE id=?1",
        ] {
            transaction.execute(sql, [&plan.launch_preset_id])
                .map_err(|error| format!("workspace_delete_database_failed:文件或档案可能已删除，空间记录已保留，可重试：{error}"))?;
        }
        for gateway_id in gateway_ids {
            transaction.execute(
                "DELETE FROM gateway_service_revisions WHERE id=?1
                 AND NOT EXISTS(SELECT 1 FROM route_set_revisions WHERE gateway_service_revision_id=?1)
                 AND NOT EXISTS(SELECT 1 FROM instance_gateway_bindings WHERE gateway_service_revision_id=?1)
                 AND NOT EXISTS(SELECT 1 FROM compatibility_gateway_bindings WHERE gateway_service_revision_id=?1)
                 AND NOT EXISTS(SELECT 1 FROM launch_configuration_dependencies WHERE resource_kind='gateway_service_revision' AND resource_id=?1)",
                [gateway_id],
            ).map_err(|error| format!("workspace_delete_gateway_revision_failed:{error}"))?;
        }
        // The deleted preset is already removed at this point, so any surviving
        // preferred row belongs to another workspace. Deleting a workspace that
        // is not the current selection keeps that selection; deleting the
        // selected workspace falls back to the first remaining slot by name
        // order.
        let surviving_preferred: Option<String> = transaction.query_row(
            "SELECT id FROM launch_presets WHERE entry_kind='managed_app' AND deleted_at IS NULL AND is_preferred=1",
            [], |row| row.get(0),
        ).optional().map_err(|error| format!("workspace_delete_selection_failed:{error}"))?;
        let selected: Option<String> = if surviving_preferred.is_some() {
            surviving_preferred
        } else {
            let first_remaining: Option<String> = transaction.query_row(
                "SELECT id FROM launch_presets WHERE entry_kind='managed_app' AND deleted_at IS NULL ORDER BY name ASC, created_at ASC, id ASC LIMIT 1",
                [], |row| row.get(0),
            ).optional().map_err(|error| format!("workspace_delete_selection_failed:{error}"))?;
            transaction
                .execute(
                    "UPDATE launch_presets SET is_preferred=0 WHERE is_preferred=1",
                    [],
                )
                .map_err(|error| format!("workspace_delete_selection_failed:{error}"))?;
            if let Some(id) = &first_remaining {
                transaction
                    .execute("UPDATE launch_presets SET is_preferred=1 WHERE id=?1", [id])
                    .map_err(|error| format!("workspace_delete_selection_failed:{error}"))?;
            } else {
                transaction.execute("INSERT INTO provider_migration_state(key,value) VALUES (?1,'true') ON CONFLICT(key) DO UPDATE SET value='true'", [EXPLICITLY_EMPTY_WORKSPACE_KEY])
                    .map_err(|error| format!("workspace_delete_empty_state_failed:{error}"))?;
            }
            first_remaining
        };
        transaction
            .execute(
                "DELETE FROM resource_operation_leases WHERE operation_id=?1",
                [&plan.operation_id],
            )
            .map_err(|error| format!("workspace_delete_lease_release_failed:{error}"))?;
        transaction
            .execute(
                "DELETE FROM provider_migration_state WHERE key=?1",
                [format!(
                    "managed_workspace_deleting:{}",
                    plan.launch_preset_id
                )],
            )
            .map_err(|error| format!("workspace_delete_intent_clear_failed:{error}"))?;
        transaction.commit().map_err(|error| {
            format!("workspace_delete_commit_failed:空间记录已保留，可重试：{error}")
        })?;
        Ok(selected)
    }

    fn workspace_deletion_paths(
        &self,
        connection: &Connection,
        preset_id: &str,
        instances: &[InstanceStorage],
    ) -> Result<Vec<OwnedPath>, String> {
        let root = self
            .instances_root
            .parent()
            .ok_or("managed_codex_home_root_invalid")?;
        ensure_owned_path(root, &self.instances_root)?;
        let short_root = root.join("r");
        ensure_owned_path(root, &short_root)?;
        let mut paths = Vec::new();
        for instance in instances {
            if instance.lifecycle != self.instances_root.join(&instance.id) {
                return Err("workspace_delete_lifecycle_path_invalid".to_string());
            }
            let long_home = instance.lifecycle.join("codex-home");
            let mut short_home = if instance.home.parent() == Some(short_root.as_path())
                && short_name_matches(&instance.id, &instance.home)
            {
                Some(instance.home.clone())
            } else if instance.home == long_home {
                None
            } else {
                return Err("workspace_delete_home_path_invalid".to_string());
            };
            let marker = instance.lifecycle.join("home-layout.json");
            if let Ok(metadata) = std::fs::symlink_metadata(&marker) {
                if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 8192
                {
                    return Err("workspace_delete_home_layout_invalid".to_string());
                }
                let value: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(&marker)
                        .map_err(|_| "workspace_delete_home_layout_unreadable")?,
                )
                .map_err(|_| "workspace_delete_home_layout_invalid")?;
                let candidate = value
                    .get("short_home")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .ok_or("workspace_delete_home_layout_invalid")?;
                if value.get("instance_id").and_then(|v| v.as_str()) != Some(instance.id.as_str())
                    || value
                        .get("long_home")
                        .and_then(|v| v.as_str())
                        .map(Path::new)
                        != Some(long_home.as_path())
                    || candidate.parent() != Some(short_root.as_path())
                    || !short_name_matches(&instance.id, &candidate)
                {
                    return Err("workspace_delete_home_layout_invalid".to_string());
                }
                short_home = Some(candidate);
            }
            if let Some(short_home) = short_home {
                let long_link = std::fs::symlink_metadata(&long_home)
                    .ok()
                    .is_some_and(|m| m.file_type().is_symlink());
                paths.push(OwnedPath {
                    path: long_home.clone(),
                    link_target: if long_link {
                        Some(short_home.clone())
                    } else {
                        None
                    },
                });
                paths.push(OwnedPath {
                    path: short_home,
                    link_target: None,
                });
            } else {
                paths.push(OwnedPath {
                    path: long_home.clone(),
                    link_target: None,
                });
            }
            // CLI and pre-migration App homes may leave aliases after a failed
            // deletion. Only aliases whose stored target is exactly ours count.
            let compact = instance.id.replace('-', "");
            for length in [12, 16, 24, 32] {
                if let Some(name) = compact.get(..length) {
                    let alias = short_root.join(name);
                    if std::fs::read_link(&alias).ok().as_deref() == Some(long_home.as_path()) {
                        paths.push(OwnedPath {
                            path: alias,
                            link_target: Some(long_home.clone()),
                        });
                    }
                }
            }
            paths.push(OwnedPath {
                path: instance.lifecycle.join("electron-data"),
                link_target: None,
            });
            paths.push(OwnedPath {
                path: instance.lifecycle.clone(),
                link_target: None,
            });
        }
        paths.push(OwnedPath {
            path: self.managed_app_sqlite_path(preset_id)?,
            link_target: None,
        });
        paths.sort_by(|a, b| {
            b.link_target
                .is_some()
                .cmp(&a.link_target.is_some())
                .then_with(|| {
                    b.path
                        .components()
                        .count()
                        .cmp(&a.path.components().count())
                })
                .then_with(|| a.path.cmp(&b.path))
        });
        paths.dedup();
        for path in &paths {
            validate_owned_path(root, path)?;
        }
        // Refuse corrupted/shared paths even when both locations are inside the
        // application-owned root. Another workspace must remain untouched.
        let other = read_other_instance_paths(connection, preset_id)?;
        for path in &paths {
            for other_path in &other {
                if path.path.starts_with(other_path) || other_path.starts_with(&path.path) {
                    return Err("workspace_delete_shared_path".to_string());
                }
            }
        }
        Ok(paths)
    }
}

fn validate_owned_path(root: &Path, owned: &OwnedPath) -> Result<(), String> {
    if owned.path == root {
        return Err("workspace_delete_owned_root_invalid".to_string());
    }
    if let Some(expected) = &owned.link_target {
        let parent = owned.path.parent().ok_or("workspace_delete_path_invalid")?;
        ensure_owned_path(root, parent)?;
        match std::fs::symlink_metadata(&owned.path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if std::fs::read_link(&owned.path)
                    .map_err(|_| "workspace_delete_link_unreadable")?
                    != *expected
                {
                    return Err("workspace_delete_link_conflict".to_string());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("workspace_delete_link_conflict".to_string()),
        }
    } else {
        ensure_owned_path(root, &owned.path)?;
        match std::fs::symlink_metadata(&owned.path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("workspace_delete_directory_invalid".to_string()),
        }
    }
    Ok(())
}

fn short_name_matches(id: &str, path: &Path) -> bool {
    let compact = id.replace('-', "");
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            [12, 16, 24, 32]
                .iter()
                .any(|length| compact.get(..*length) == Some(name))
        })
}

fn verify_workspace(connection: &Connection, id: &str, version: i64) -> Result<String, String> {
    let value: Option<(String,i64)> = connection.query_row(
        "SELECT name,version FROM launch_presets WHERE id=?1 AND entry_kind='managed_app' AND deleted_at IS NULL", [id], |row| Ok((row.get(0)?,row.get(1)?))
    ).optional().map_err(|error| format!("workspace_delete_lookup_failed:{error}"))?;
    let (name, current_version) = value.ok_or("launch_preset_not_found")?;
    if current_version != version {
        return Err("version_conflict".to_string());
    }
    Ok(name)
}

fn read_instances(connection: &Connection, id: &str) -> Result<Vec<InstanceStorage>, String> {
    let mut statement = connection.prepare(
        "SELECT instance.id,instance.managed_codex_home,instance.lifecycle_dir,
          COALESCE(json_extract(config.config_snapshot,'$.launchTarget'),preset.entry_kind,'cli'),instance.state,
          EXISTS(SELECT 1 FROM runtime_configuration_locks lock WHERE lock.instance_id=instance.id AND lock.released_at IS NULL)
         FROM codex_instances instance JOIN launch_configuration_revisions config ON config.id=instance.launch_configuration_revision_id
         JOIN launch_presets preset ON preset.id=config.launch_preset_id
         WHERE config.launch_preset_id=?1 ORDER BY instance.id"
    ).map_err(|error| format!("workspace_delete_instances_read_failed:{error}"))?;
    let rows = statement
        .query_map([id], |row| {
            Ok((
                InstanceStorage {
                    id: row.get(0)?,
                    home: PathBuf::from(row.get::<_, String>(1)?),
                    lifecycle: PathBuf::from(row.get::<_, String>(2)?),
                    target: row.get(3)?,
                },
                row.get::<_, String>(4)?,
                row.get::<_, bool>(5)?,
            ))
        })
        .map_err(|error| format!("workspace_delete_instances_read_failed:{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("workspace_delete_instances_read_failed:{error}"))?;
    let mut instances = Vec::new();
    for (instance, state, locked) in rows {
        if !matches!(instance.target.as_str(), "managed_app" | "cli") {
            return Err("workspace_delete_instance_target_invalid".to_string());
        }
        if !matches!(state.as_str(), "exited" | "failed") {
            return Err("workspace_has_active_instances".to_string());
        }
        if locked {
            return Err("instance_lock_active".to_string());
        }
        instances.push(instance);
    }
    let bindings = strings(
        connection,
        "SELECT instance_id FROM launch_preset_runtime_bindings WHERE launch_preset_id=?1",
        id,
    )?;
    if bindings
        .iter()
        .any(|binding| !instances.iter().any(|instance| &instance.id == binding))
    {
        return Err("workspace_delete_binding_conflict".to_string());
    }
    let active_compat:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM compatibility_gateway_bindings WHERE launch_preset_id=?1 AND state='active')",[id],|row|row.get(0))
        .map_err(|error|format!("workspace_delete_binding_read_failed:{error}"))?;
    if active_compat {
        return Err("workspace_delete_legacy_binding_active".to_string());
    }
    Ok(instances)
}

fn read_other_instance_paths(connection: &Connection, id: &str) -> Result<Vec<PathBuf>, String> {
    let mut statement=connection.prepare("SELECT instance.lifecycle_dir,instance.managed_codex_home FROM codex_instances instance JOIN launch_configuration_revisions config ON config.id=instance.launch_configuration_revision_id WHERE config.launch_preset_id IS NULL OR config.launch_preset_id<>?1")
        .map_err(|error|format!("workspace_delete_paths_read_failed:{error}"))?;
    let rows = statement
        .query_map([id], |row| {
            Ok([
                PathBuf::from(row.get::<_, String>(0)?),
                PathBuf::from(row.get::<_, String>(1)?),
            ])
        })
        .map_err(|error| format!("workspace_delete_paths_read_failed:{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("workspace_delete_paths_read_failed:{error}"))?;
    Ok(rows.into_iter().flatten().collect())
}

fn verify_plan(connection: &Connection, plan: &WorkspaceDeletionPlan) -> Result<(), String> {
    verify_workspace(connection, &plan.launch_preset_id, plan.expected_version)?;
    let lease:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM resource_operation_leases WHERE resource_kind='launch_preset' AND resource_id=?1 AND operation_id=?2 AND operation_kind=?3)",params![plan.launch_preset_id,plan.operation_id,DELETION_OPERATION],|row|row.get(0))
        .map_err(|error|format!("workspace_delete_lease_read_failed:{error}"))?;
    if !lease {
        return Err("workspace_delete_lease_lost".to_string());
    }
    if read_instances(connection, &plan.launch_preset_id)? != plan.instances {
        return Err("workspace_delete_instances_changed".to_string());
    }
    Ok(())
}

fn strings(connection: &Connection, sql: &str, id: &str) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("workspace_delete_query_failed:{error}"))?;
    let values = statement
        .query_map([id], |row| row.get(0))
        .map_err(|error| format!("workspace_delete_query_failed:{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("workspace_delete_query_failed:{error}"))?;
    Ok(values)
}

pub(crate) fn assert_workspace_not_deleting(
    connection: &Connection,
    id: &str,
) -> Result<(), String> {
    let deleting: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_migration_state WHERE key=?1)",
            [format!("managed_workspace_deleting:{id}")],
            |row| row.get(0),
        )
        .map_err(|error| format!("workspace_delete_intent_read_failed:{error}"))?;
    if deleting {
        return Err("workspace_delete_in_progress".to_string());
    }
    Ok(())
}
