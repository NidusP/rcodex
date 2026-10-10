//! 官方启动准备与失败落账。
//!
//! 不 spawn 进程、不投影隔离配置。启动编排在拿到 PreparedOfficialLaunch 后由 command 继续。

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use rusqlite::{params, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use crate::support::unix_now_seconds;

use super::helpers::{managed_official_home_matches, resolve_workspace, set_private_directory};
use super::lifecycle::{locked_instances, release_finished_instance_locks};
use super::models::{LaunchCodexInput, PreparedOfficialLaunch};
use super::CodexControlStore;
use super::ACTIVE_INSTANCE_STATES;

impl CodexControlStore {
    pub fn preset_credential_kind(&self, input: &LaunchCodexInput) -> Result<String, String> {
        let preset = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?
            .query_row(
                "SELECT credential_kind, version FROM launch_presets
                 WHERE id=?1 AND deleted_at IS NULL",
                [&input.launch_preset_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(|error| format!("无法读取启动预设：{error}"))?
            .ok_or_else(|| "launch_preset_not_found".to_string())?;
        if preset.1 != input.expected_version {
            return Err("version_conflict".to_string());
        }
        Ok(preset.0)
    }

    pub fn assert_resource_mutable(&self, kind: &str, id: &str) -> Result<(), String> {
        self.reconcile_all()?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        release_finished_instance_locks(&connection)?;
        let ids = locked_instances(&connection, kind, id)?;
        if !ids.is_empty() {
            return Err(format!("configuration_in_use:{}", ids.join(",")));
        }
        let leased: bool = connection
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM resource_operation_leases
                   WHERE resource_kind=?1 AND resource_id=?2
                 )",
                params![kind, id],
                |row| row.get(0),
            )
            .map_err(|error| format!("无法检查资源操作租约：{error}"))?;
        if leased {
            return Err("resource_operation_in_progress".to_string());
        }
        Ok(())
    }

    pub fn prepare_official_launch(
        &self,
        input: &LaunchCodexInput,
        codex_executable: &Path,
        codex_version: &str,
    ) -> Result<PreparedOfficialLaunch, String> {
        let workspace_path = resolve_workspace(input.workspace_path.as_deref())?;
        let instance_id = Uuid::new_v4().to_string();
        let launch_nonce = Uuid::new_v4().to_string();
        let lifecycle_dir = self.instances_root.join(&instance_id);
        let status_file = lifecycle_dir.join("status.json");
        let now = unix_now_seconds();
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("无法开始启动事务：{error}"))?;
        let preset = transaction
            .query_row(
                "SELECT p.version, p.credential_kind, p.codex_account_id,
                        a.current_credential_revision_id,
                        r.managed_codex_home, r.auth_state
                 FROM launch_presets p
                 LEFT JOIN codex_accounts a ON a.id = p.codex_account_id
                 LEFT JOIN official_credential_revisions r
                   ON r.id = a.current_credential_revision_id
                 WHERE p.id = ?1 AND p.deleted_at IS NULL",
                [&input.launch_preset_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("无法解析启动预设：{error}"))?
            .ok_or_else(|| "launch_preset_not_found".to_string())?;
        if preset.0 != input.expected_version {
            return Err("version_conflict".to_string());
        }
        if preset.1 != "official" {
            return Err("invalid_launch_preset".to_string());
        }
        let account_id = preset.2.ok_or_else(|| "credential_missing".to_string())?;
        let credential_revision_id = preset
            .3
            .ok_or_else(|| "official_account_needs_reauth".to_string())?;
        let managed_codex_home = PathBuf::from(
            preset
                .4
                .ok_or_else(|| "official_account_needs_reauth".to_string())?,
        );
        if preset.5.as_deref() != Some("authorized") {
            return Err("official_account_needs_reauth".to_string());
        }
        if !managed_official_home_matches(
            &self.official_homes_root,
            &account_id,
            &credential_revision_id,
            &managed_codex_home,
        ) {
            return Err("managed_home_invalid".to_string());
        }
        ensure_no_operation_lease(
            &transaction,
            &[
                ("launch_preset", input.launch_preset_id.as_str()),
                ("official_account", account_id.as_str()),
                (
                    "official_credential_revision",
                    credential_revision_id.as_str(),
                ),
            ],
        )?;
        let already_active: bool = transaction
            .query_row(
                &format!(
                    "SELECT EXISTS(
                        SELECT 1 FROM codex_instances
                        WHERE official_credential_revision_id=?1
                          AND state IN ({ACTIVE_INSTANCE_STATES})
                    )"
                ),
                [&credential_revision_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("无法检查官方凭据实例：{error}"))?;
        if already_active {
            return Err("official_credential_in_use".to_string());
        }

        let launch_configuration_id = Uuid::new_v4().to_string();
        let snapshot = serde_json::json!({
            "launchPresetId": input.launch_preset_id,
            "launchPresetVersion": input.expected_version,
            "credentialKind": "official",
            "accountId": account_id,
            "credentialRevisionId": credential_revision_id,
            "workspacePath": workspace_path,
        })
        .to_string();
        let mut hasher = DefaultHasher::new();
        snapshot.hash(&mut hasher);
        let profile_snapshot_hash = format!("std-hash:{:016x}", hasher.finish());
        transaction
            .execute(
                "INSERT INTO launch_configuration_revisions (
                    id, launch_preset_id, launch_preset_version, credential_kind,
                    profile_snapshot_hash, config_snapshot, codex_executable,
                    codex_version, created_at
                 ) VALUES (?1, ?2, ?3, 'official', ?4, ?5, ?6, ?7, ?8)",
                params![
                    launch_configuration_id,
                    input.launch_preset_id,
                    input.expected_version,
                    profile_snapshot_hash,
                    snapshot,
                    codex_executable.display().to_string(),
                    codex_version,
                    now,
                ],
            )
            .map_err(|error| format!("无法创建启动配置修订：{error}"))?;
        insert_dependency(
            &transaction,
            &launch_configuration_id,
            "launch_preset",
            &input.launch_preset_id,
            None,
        )?;
        insert_dependency(
            &transaction,
            &launch_configuration_id,
            "official_account",
            &account_id,
            Some(&credential_revision_id),
        )?;
        insert_dependency(
            &transaction,
            &launch_configuration_id,
            "official_credential_revision",
            &credential_revision_id,
            Some(&credential_revision_id),
        )?;
        transaction
            .execute(
                "INSERT INTO codex_instances (
                    id, launch_configuration_revision_id, credential_kind,
                    official_credential_revision_id, workspace_path,
                    managed_codex_home, lifecycle_dir, launch_nonce,
                    state, launched_at
                 ) VALUES (?1, ?2, 'official', ?3, ?4, ?5, ?6, ?7, 'launching', ?8)",
                params![
                    instance_id,
                    launch_configuration_id,
                    credential_revision_id,
                    workspace_path,
                    managed_codex_home.display().to_string(),
                    lifecycle_dir.display().to_string(),
                    launch_nonce,
                    now,
                ],
            )
            .map_err(|error| format!("无法创建 Codex 实例：{error}"))?;
        transaction
            .execute(
                "INSERT INTO runtime_configuration_locks (
                    instance_id, launch_configuration_revision_id, acquired_at
                 ) VALUES (?1, ?2, ?3)",
                params![instance_id, launch_configuration_id, now],
            )
            .map_err(|error| format!("无法获取运行配置锁：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("无法提交启动事务：{error}"))?;
        drop(connection);

        if !managed_codex_home.is_dir() {
            self.mark_launch_failed(&instance_id, true)?;
            return Err("official_account_needs_reauth".to_string());
        }
        if let Err(error) = std::fs::create_dir_all(&lifecycle_dir)
            .map_err(|error| format!("无法创建实例生命周期目录：{error}"))
            .and_then(|_| set_private_directory(&lifecycle_dir))
        {
            self.mark_launch_failed(&instance_id, true)?;
            return Err(error);
        }
        Ok(PreparedOfficialLaunch {
            instance_id,
            launch_nonce,
            managed_codex_home,
            status_file,
            workspace_path,
        })
    }

    pub fn mark_launch_failed(
        &self,
        instance_id: &str,
        confirmed_no_process: bool,
    ) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        if confirmed_no_process {
            connection
                .execute(
                    "UPDATE codex_instances SET state='failed', exited_at=?2
                     WHERE id=?1 AND state IN ('launching','orphaned')",
                    params![instance_id, unix_now_seconds()],
                )
                .map_err(|error| format!("无法记录启动失败：{error}"))?;
            connection
                .execute(
                    "UPDATE runtime_configuration_locks SET released_at=?2
                     WHERE instance_id=?1 AND released_at IS NULL",
                    params![instance_id, unix_now_seconds()],
                )
                .map_err(|error| format!("无法释放失败实例配置锁：{error}"))?;
        } else {
            connection
                .execute(
                    "UPDATE codex_instances SET state='orphaned'
                     WHERE id=?1 AND state='launching'",
                    [instance_id],
                )
                .map_err(|error| format!("无法记录未核验实例：{error}"))?;
        }
        Ok(())
    }
}

fn insert_dependency(
    transaction: &rusqlite::Transaction<'_>,
    launch_configuration_id: &str,
    kind: &str,
    id: &str,
    revision_id: Option<&str>,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO launch_configuration_dependencies (
                launch_configuration_revision_id, resource_kind,
                resource_id, resource_revision_id
             ) VALUES (?1, ?2, ?3, ?4)",
            params![launch_configuration_id, kind, id, revision_id],
        )
        .map_err(|error| format!("无法记录启动配置依赖：{error}"))?;
    Ok(())
}

pub(super) fn ensure_no_operation_lease(
    transaction: &rusqlite::Transaction<'_>,
    resources: &[(&str, &str)],
) -> Result<(), String> {
    for (kind, id) in resources {
        let operation: Option<String> = transaction
            .query_row(
                "SELECT operation_id FROM resource_operation_leases
                 WHERE resource_kind=?1 AND resource_id=?2",
                params![kind, id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("无法检查资源操作租约：{error}"))?;
        if operation.is_some() {
            return Err("resource_operation_in_progress".to_string());
        }
    }
    Ok(())
}

pub(super) fn reject_locked_resource(
    transaction: &rusqlite::Transaction<'_>,
    kind: &str,
    id: &str,
) -> Result<(), String> {
    let ids = locked_instances(transaction, kind, id)?;
    if ids.is_empty() {
        Ok(())
    } else {
        Err(format!("configuration_in_use:{}", ids.join(",")))
    }
}
