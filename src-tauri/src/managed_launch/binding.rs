//! Runtime binding：启动记录、准入密钥引用、延后撤销。
//!
//! 不物化 Codex 配置，不删除隔离目录。SQLite 只存 hash/引用，明文密钥不进库。
//! 由 prepare 在同一事务里写入，停止/删除实例时撤销。

use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use crate::launcher_protocol::LaunchTarget;
use crate::routing::RouteSetRevision;
use crate::support::unix_now_seconds;

use super::models::ManagedLaunchStore;
use super::persistence::{insert_dependency, query_strings};
use super::preset_validation::ensure_resources_available;

impl ManagedLaunchStore {
    /// Revoke the database binding immediately. The instance admission secret
    /// is not persisted in plaintext, so there is no external secret cleanup.
    pub fn revoke_binding_deferred(&self, instance_id: &str) -> Result<(), String> {
        let _ = self.revoke_binding_record(instance_id)?;
        Ok(())
    }

    pub(super) fn revoke_binding_record(
        &self,
        instance_id: &str,
    ) -> Result<Option<String>, String> {
        let (secret_ref, should_clear) = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
            let managed_codex_home = connection
                .query_row(
                    "SELECT managed_codex_home FROM codex_instances WHERE id=?1",
                    [instance_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| format!("managed_codex_home_read_failed:{error}"))?;
            let Some(_managed_codex_home) = managed_codex_home else {
                return Ok(None);
            };
            let secret_ref = connection
                .query_row(
                    "SELECT instance_key_ref FROM instance_gateway_bindings
                     WHERE instance_id=?1",
                    [instance_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| format!("instance_binding_read_failed:{error}"))?;
            let should_clear = if secret_ref.is_some() {
                let changed = connection
                    .execute(
                        "UPDATE instance_gateway_bindings
                         SET state='revoked', revoked_at=?2
                         WHERE instance_id=?1 AND state='active'",
                        params![instance_id, unix_now_seconds()],
                    )
                    .map_err(|error| format!("instance_binding_revoke_failed:{error}"))?;
                changed > 0
            } else {
                false
            };
            (secret_ref, should_clear)
        };
        Ok(should_clear.then_some(secret_ref).flatten())
    }

    pub fn revoke_binding_for_delete(&self, instance_id: &str) -> Result<(), String> {
        let secret_ref = if let Some(secret_ref) = self.revoke_binding_record(instance_id)? {
            Some(secret_ref)
        } else {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
            connection
                .query_row(
                    "SELECT instance_key_ref FROM instance_gateway_bindings
                     WHERE instance_id=?1",
                    [instance_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| format!("instance_binding_read_failed:{error}"))?
        };
        let _ = secret_ref;
        Ok(())
    }

    pub(super) fn managed_app_runtime_binding(
        &self,
        launch_preset_id: &str,
        launch_target: &str,
    ) -> Result<Option<(String, PathBuf, PathBuf, String)>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
        let bound = connection
            .query_row(
                "SELECT instance.id, instance.managed_codex_home, instance.lifecycle_dir,
                        instance.state
                 FROM launch_preset_runtime_bindings binding
                 JOIN codex_instances instance ON instance.id=binding.instance_id
                 WHERE binding.launch_preset_id=?1",
                [launch_preset_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        PathBuf::from(row.get::<_, String>(1)?),
                        PathBuf::from(row.get::<_, String>(2)?),
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("app_runtime_binding_read_failed:{error}"))?;
        if let Some(bound) = bound {
            let target_matches = connection
                .query_row(
                    "SELECT COALESCE(
                            json_extract(configuration.config_snapshot, '$.launchTarget'),
                            preset.entry_kind
                        )
                     FROM launch_configuration_revisions configuration
                     LEFT JOIN launch_presets preset
                       ON preset.id=configuration.launch_preset_id
                     WHERE configuration.id=(SELECT launch_configuration_revision_id
                                              FROM codex_instances WHERE id=?1)",
                    [&bound.0],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()
                .map_err(|error| format!("app_runtime_binding_read_failed:{error}"))?
                .flatten()
                .is_some_and(|target| target == launch_target);
            if target_matches {
                return Ok(Some(bound));
            }
        }
        // Older databases may contain a complete App launch row but no
        // stable binding (the binding was introduced after compatibility
        // mode). Reuse the newest row for this exact target and let the final
        // transaction attach the missing binding instead of creating a
        // duplicate runtime record.
        connection
            .query_row(
                "SELECT instance.id, instance.managed_codex_home, instance.lifecycle_dir,
                        instance.state
                 FROM codex_instances instance
                 JOIN launch_configuration_revisions configuration
                   ON configuration.id=instance.launch_configuration_revision_id
                 LEFT JOIN launch_presets preset
                   ON preset.id=configuration.launch_preset_id
                 WHERE configuration.launch_preset_id=?1
                   AND COALESCE(
                         json_extract(configuration.config_snapshot, '$.launchTarget'),
                         preset.entry_kind
                       )=?2
                 ORDER BY COALESCE(instance.launched_at, configuration.created_at) DESC
                 LIMIT 1",
                params![launch_preset_id, launch_target],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        PathBuf::from(row.get::<_, String>(1)?),
                        PathBuf::from(row.get::<_, String>(2)?),
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("app_runtime_binding_read_failed:{error}"))
    }

    pub fn terminal_active_bindings(&self) -> Result<Vec<String>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
        query_strings(
            &connection,
            "SELECT binding.instance_id
             FROM instance_gateway_bindings binding
             JOIN codex_instances instance ON instance.id=binding.instance_id
             WHERE binding.state='active' AND instance.state IN ('exited','failed')
             ORDER BY binding.created_at",
            "",
        )
    }

    #[cfg(test)]
    pub(super) fn revoke_binding(&self, instance_id: &str) -> Result<(), String> {
        let _ = self.revoke_binding_record(instance_id)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn insert_launch_records(
        &self,
        instance_id: &str,
        launch_nonce: &str,
        lifecycle_dir: &Path,
        managed_codex_home: &Path,
        workspace_path: Option<&str>,
        codex_executable: &Path,
        codex_version: &str,
        profile_sha256: &str,
        route_set: &RouteSetRevision,
        instance_key_ref: Option<&str>,
        instance_key_hash: Option<&str>,
        launch_target: &str,
    ) -> Result<(), String> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("managed_launch_transaction_failed:{error}"))?;
        let version: i64 = transaction
            .query_row(
                "SELECT version FROM launch_presets
                 WHERE id=?1 AND deleted_at IS NULL",
                [&route_set.launch_preset_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("managed_preset_lookup_failed:{error}"))?;
        if version as u64 != route_set.launch_preset_version {
            return Err("version_conflict".to_string());
        }
        ensure_resources_available(&transaction, route_set)?;
        let existing_bound_instance: Option<String> =
            if LaunchTarget::wire_is_managed_app(&launch_target) {
                transaction
                    .query_row(
                        "SELECT binding.instance_id
                         FROM launch_preset_runtime_bindings binding
                         JOIN codex_instances instance ON instance.id=binding.instance_id
                         JOIN launch_configuration_revisions configuration
                           ON configuration.id=instance.launch_configuration_revision_id
                         LEFT JOIN launch_presets preset
                           ON preset.id=configuration.launch_preset_id
                         WHERE binding.launch_preset_id=?1
                           AND COALESCE(
                                 json_extract(configuration.config_snapshot, '$.launchTarget'),
                                 preset.entry_kind
                               )=?2",
                        params![&route_set.launch_preset_id, launch_target],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|error| format!("app_runtime_binding_read_failed:{error}"))?
            } else {
                None
            };
        let legacy_runtime_instance: Option<String> =
            if LaunchTarget::wire_is_managed_app(&launch_target) {
                transaction
                    .query_row(
                        "SELECT instance.id
                         FROM codex_instances instance
                         JOIN launch_configuration_revisions configuration
                           ON configuration.id=instance.launch_configuration_revision_id
                         LEFT JOIN launch_presets preset
                           ON preset.id=configuration.launch_preset_id
                         WHERE configuration.launch_preset_id=?1
                           AND COALESCE(
                                 json_extract(configuration.config_snapshot, '$.launchTarget'),
                                 preset.entry_kind
                               )=?2
                         ORDER BY COALESCE(instance.launched_at, configuration.created_at) DESC
                         LIMIT 1",
                        params![&route_set.launch_preset_id, launch_target],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|error| format!("app_runtime_binding_read_failed:{error}"))?
            } else {
                None
            };
        let existing_runtime_instance = existing_bound_instance.clone().or(legacy_runtime_instance);
        if existing_runtime_instance.as_deref() != Some(instance_id)
            && existing_runtime_instance.is_some()
        {
            return Err("managed_app_runtime_binding_mismatch".to_string());
        }
        let reuse_runtime_instance = existing_runtime_instance.is_some();
        let has_runtime_binding = existing_bound_instance.is_some();
        let first_provider = route_set
            .providers
            .first()
            .ok_or_else(|| "credential_missing".to_string())?;
        let launch_configuration_id = Uuid::new_v4().to_string();
        let snapshot = serde_json::json!({
            "launchPresetId": route_set.launch_preset_id,
            "launchPresetVersion": route_set.launch_preset_version,
            "credentialKind": "managed_route_set",
            "routeSetRevisionId": route_set.id,
            "providerRevisionIds": route_set.providers.iter().map(|item| &item.provider_revision_id).collect::<Vec<_>>(),
            "publicModelIds": route_set.routes.iter().map(|item| &item.public_model_id).collect::<Vec<_>>(),
            "launchTarget": launch_target,
            "workspacePath": workspace_path,
        })
        .to_string();
        let now = unix_now_seconds();
        transaction
            .execute(
                "INSERT INTO launch_configuration_revisions (
                    id, launch_preset_id, launch_preset_version, credential_kind,
                    profile_snapshot_hash, config_snapshot, codex_executable,
                    codex_version, created_at
                 ) VALUES (?1, ?2, ?3, 'api_key', ?4, ?5, ?6, ?7, ?8)",
                params![
                    launch_configuration_id,
                    route_set.launch_preset_id,
                    route_set.launch_preset_version,
                    profile_sha256,
                    snapshot,
                    codex_executable.display().to_string(),
                    codex_version,
                    now,
                ],
            )
            .map_err(|error| format!("managed_launch_configuration_insert_failed:{error}"))?;
        insert_dependency(
            &transaction,
            &launch_configuration_id,
            "launch_preset",
            &route_set.launch_preset_id,
            None,
        )?;
        insert_dependency(
            &transaction,
            &launch_configuration_id,
            "route_set_revision",
            &route_set.id,
            Some(&route_set.id),
        )?;
        if instance_key_ref.is_some() {
            insert_dependency(
                &transaction,
                &launch_configuration_id,
                "gateway_service_revision",
                &route_set.gateway_service.id,
                Some(&route_set.gateway_service.id),
            )?;
        }
        insert_dependency(
            &transaction,
            &launch_configuration_id,
            "profile_revision",
            instance_id,
            Some(profile_sha256),
        )?;
        if let Some(instance_key_hash) = instance_key_hash {
            insert_dependency(
                &transaction,
                &launch_configuration_id,
                "instance_gateway_binding",
                instance_id,
                Some(instance_key_hash),
            )?;
        }
        for provider in &route_set.providers {
            insert_dependency(
                &transaction,
                &launch_configuration_id,
                "api_key_provider",
                &provider.provider_id,
                Some(&provider.provider_revision_id),
            )?;
            insert_dependency(
                &transaction,
                &launch_configuration_id,
                "api_key_provider_revision",
                &provider.provider_revision_id,
                Some(&provider.provider_revision_id),
            )?;
        }
        for route in &route_set.routes {
            insert_dependency(
                &transaction,
                &launch_configuration_id,
                "model_route",
                &route.id,
                Some(&route.id),
            )?;
            insert_dependency(
                &transaction,
                &launch_configuration_id,
                "model_route_identity",
                &route.route_identity_id,
                Some(&route.route_identity_id),
            )?;
        }
        transaction
            .execute(
                "INSERT INTO launch_configuration_route_sets (
                    launch_configuration_revision_id, route_set_revision_id
                 ) VALUES (?1, ?2)",
                params![launch_configuration_id, route_set.id],
            )
            .map_err(|error| format!("managed_route_set_link_insert_failed:{error}"))?;
        if reuse_runtime_instance {
            transaction
                .execute(
                    "UPDATE codex_instances
                     SET launch_configuration_revision_id=?2,
                         credential_kind='api_key',
                         api_key_provider_revision_id=?3,
                         official_credential_revision_id=NULL,
                         workspace_path=?4,
                         managed_codex_home=?5,
                         lifecycle_dir=?6,
                         launch_nonce=?7,
                         state='launching', supervisor_pid=NULL, pid=NULL,
                         process_started_at=NULL, launched_at=?8,
                         exited_at=NULL, exit_code=NULL
                     WHERE id=?1 AND state IN ('exited','failed','degraded','orphaned')",
                    params![
                        instance_id,
                        launch_configuration_id,
                        first_provider.provider_revision_id,
                        workspace_path,
                        managed_codex_home.display().to_string(),
                        lifecycle_dir.display().to_string(),
                        launch_nonce,
                        now,
                    ],
                )
                .map_err(|error| format!("managed_instance_reopen_failed:{error}"))?;
        } else {
            transaction
                .execute(
                    "INSERT INTO codex_instances (
                        id, launch_configuration_revision_id, credential_kind,
                        api_key_provider_revision_id, workspace_path,
                        managed_codex_home, lifecycle_dir, launch_nonce,
                        state, launched_at
                     ) VALUES (?1, ?2, 'api_key', ?3, ?4, ?5, ?6, ?7, 'launching', ?8)",
                    params![
                        instance_id,
                        launch_configuration_id,
                        first_provider.provider_revision_id,
                        workspace_path,
                        managed_codex_home.display().to_string(),
                        lifecycle_dir.display().to_string(),
                        launch_nonce,
                        now,
                    ],
                )
                .map_err(|error| format!("managed_instance_insert_failed:{error}"))?;
        }
        if reuse_runtime_instance {
            transaction
                .execute(
                    "UPDATE runtime_configuration_locks
                     SET launch_configuration_revision_id=?2,
                         acquired_at=?3, released_at=NULL
                     WHERE instance_id=?1",
                    params![instance_id, launch_configuration_id, now],
                )
                .map_err(|error| format!("managed_runtime_lock_reopen_failed:{error}"))?;
        } else {
            transaction
                .execute(
                    "INSERT INTO runtime_configuration_locks (
                        instance_id, launch_configuration_revision_id, acquired_at
                     ) VALUES (?1, ?2, ?3)",
                    params![instance_id, launch_configuration_id, now],
                )
                .map_err(|error| format!("managed_runtime_lock_insert_failed:{error}"))?;
        }
        if let (Some(instance_key_ref), Some(instance_key_hash)) =
            (instance_key_ref, instance_key_hash)
        {
            if reuse_runtime_instance {
                transaction
                    .execute(
                        "UPDATE instance_gateway_bindings
                         SET route_set_revision_id=?2,
                             gateway_service_revision_id=?3,
                             instance_key_ref=?4, instance_key_hash=?5,
                             state='active', created_at=?6, revoked_at=NULL
                         WHERE instance_id=?1",
                        params![
                            instance_id,
                            route_set.id,
                            route_set.gateway_service.id,
                            instance_key_ref,
                            instance_key_hash,
                            now,
                        ],
                    )
                    .map_err(|error| format!("managed_instance_binding_reopen_failed:{error}"))?;
            } else {
                transaction
                    .execute(
                        "INSERT INTO instance_gateway_bindings (
                            instance_id, route_set_revision_id, gateway_service_revision_id,
                            instance_key_ref, instance_key_hash, state, created_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6)",
                        params![
                            instance_id,
                            route_set.id,
                            route_set.gateway_service.id,
                            instance_key_ref,
                            instance_key_hash,
                            now,
                        ],
                    )
                    .map_err(|error| format!("managed_instance_binding_insert_failed:{error}"))?;
            }
            if LaunchTarget::wire_is_managed_app(&launch_target) && !has_runtime_binding {
                transaction
                    .execute(
                        "INSERT INTO launch_preset_runtime_bindings (
                            launch_preset_id, instance_id, created_at, updated_at
                         ) VALUES (?1, ?2, ?3, ?3)",
                        params![route_set.launch_preset_id, instance_id, now],
                    )
                    .map_err(|error| format!("app_runtime_binding_insert_failed:{error}"))?;
            } else if LaunchTarget::wire_is_managed_app(&launch_target) {
                transaction
                    .execute(
                        "UPDATE launch_preset_runtime_bindings
                         SET updated_at=?2 WHERE launch_preset_id=?1 AND instance_id=?3",
                        params![route_set.launch_preset_id, now, instance_id],
                    )
                    .map_err(|error| format!("app_runtime_binding_update_failed:{error}"))?;
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("managed_launch_commit_failed:{error}"))
    }
}
