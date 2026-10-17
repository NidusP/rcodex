//! 实例对账、停止、删除和运行锁。
//!
//! 不创建启动配置修订、不改预设内容。启动失败走 launch，快照组装走 snapshot。

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::launcher_protocol::{
    process_start_identity, read_receipt, LauncherReceipt, LauncherState,
};
use crate::support::unix_now_seconds;

use super::helpers::{canonical_or_original, process_exists};
use super::models::{CodexInstanceSummary, InstanceState};
use super::CodexControlStore;
use super::ACTIVE_INSTANCE_STATES;

impl CodexControlStore {
    /// Whether an R Codex managed instance is still active enough to rely on
    /// the loopback Gateway. This is used by destructive Gateway recovery
    /// actions; the default Codex environment never has rows in this table.
    pub fn has_active_managed_instances(&self) -> Result<bool, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM codex_instances instance
                    JOIN launch_configuration_revisions configuration
                      ON configuration.id = instance.launch_configuration_revision_id
                    LEFT JOIN launch_presets preset
                      ON preset.id = configuration.launch_preset_id
                    WHERE instance.state IN ('launching','running','stopping','orphaned','degraded')
                      AND COALESCE(
                            json_extract(configuration.config_snapshot, '$.launchTarget'),
                            preset.entry_kind,
                            'cli'
                          ) IN ('cli','managed_app')
                )",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|error| format!("无法读取活动 Codex 实例：{error}"))
    }

    pub fn reconcile_instance(&self, instance_id: &str) -> Result<CodexInstanceSummary, String> {
        self.ensure_owned_instance(instance_id)?;
        let receipt = self.instance_receipt(instance_id)?;
        if let Some(receipt) = receipt {
            self.apply_receipt(&receipt)?;
        }
        self.instance(instance_id)
    }

    pub fn instance_ids(&self) -> Result<Vec<String>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        let mut statement = connection
            .prepare("SELECT instance.id FROM codex_instances instance
                      JOIN launch_configuration_revisions configuration ON configuration.id=instance.launch_configuration_revision_id
                      LEFT JOIN launch_presets preset ON preset.id=configuration.launch_preset_id
                      WHERE COALESCE(json_extract(configuration.config_snapshot, '$.launchTarget'), preset.entry_kind, 'cli')
                        IN ('cli','managed_app') ORDER BY COALESCE(instance.launched_at, 0) DESC")
            .map_err(|error| format!("无法读取 Codex 实例列表：{error}"))?;
        let result = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("无法读取 Codex 实例列表：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法解析 Codex 实例列表：{error}"));
        result
    }

    pub fn stop_instance(&self, instance_id: &str) -> Result<CodexInstanceSummary, String> {
        let current = self.reconcile_instance(instance_id)?;
        if InstanceState::is_finished(current.state.as_str()) {
            return Ok(current);
        }
        let (pid, expected_identity) = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
            connection
                .query_row(
                    "SELECT pid, process_started_at FROM codex_instances WHERE id=?1",
                    [instance_id],
                    |row| {
                        Ok((
                            row.get::<_, Option<u32>>(0)?,
                            row.get::<_, Option<String>>(1)?,
                        ))
                    },
                )
                .map_err(|error| format!("无法读取实例进程身份：{error}"))?
        };
        let (Some(pid), Some(expected_identity)) = (pid, expected_identity) else {
            // A degraded instance can be left without a PID when the Gateway
            // died before the launcher published its running receipt. An
            // explicit stop is then conclusive for this managed instance and
            // must move it to a terminal state; merely releasing the lock
            // would leave the UI stuck in degraded and still undeletable.
            match current.state.as_str() {
                "orphaned" => self.mark_launch_failed(instance_id, true)?,
                "degraded" | "stopping" => self.finish_instance_after_explicit_stop(instance_id)?,
                "launching" => self.mark_launch_failed(instance_id, true)?,
                _ => {}
            }
            return self.instance(instance_id);
        };
        match process_start_identity(pid) {
            Ok(identity) if identity == expected_identity => {}
            Ok(_) => {
                self.finish_instance_without_receipt(instance_id)?;
                return self.instance(instance_id);
            }
            Err(_) if !process_exists(pid) => {
                self.finish_instance_without_receipt(instance_id)?;
                return self.instance(instance_id);
            }
            Err(_) => {
                self.mark_launch_failed(instance_id, false)?;
                return self.instance(instance_id);
            }
        }
        self.connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?
            .execute(
                "UPDATE codex_instances SET state='stopping' WHERE id=?1",
                [instance_id],
            )
            .map_err(|error| format!("无法记录实例停止状态：{error}"))?;
        #[cfg(unix)]
        {
            // SAFETY: the PID and OS start identity were verified immediately
            // before this signal, so a reused PID is not intentionally killed.
            let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
            if result != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ESRCH) {
                    self.finish_instance_without_receipt(instance_id)?;
                } else {
                    return Err(format!("无法停止 Codex 实例：{error}"));
                }
            }
        }
        #[cfg(target_os = "windows")]
        {
            if let Err(error) = crate::support::terminate_pid(pid) {
                if crate::support::process_exists(pid) {
                    return Err(error);
                }
                self.finish_instance_without_receipt(instance_id)?;
            }
        }
        self.instance(instance_id)
    }

    pub fn delete_instance(&self, instance_id: &str) -> Result<(), String> {
        let lifecycle_dir = self.ensure_instance_deletable(instance_id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("无法开始实例删除事务：{error}"))?;
        transaction
            .execute(
                "DELETE FROM instance_gateway_bindings WHERE instance_id=?1",
                [instance_id],
            )
            .map_err(|error| format!("无法删除实例 Gateway 绑定：{error}"))?;
        transaction
            .execute(
                "DELETE FROM runtime_configuration_locks WHERE instance_id=?1",
                [instance_id],
            )
            .map_err(|error| format!("无法删除实例配置锁记录：{error}"))?;
        transaction
            .execute(
                "DELETE FROM launch_preset_runtime_bindings WHERE instance_id=?1",
                [instance_id],
            )
            .map_err(|error| format!("无法删除启动方案运行实例绑定：{error}"))?;
        let deleted = transaction
            .execute("DELETE FROM codex_instances WHERE id=?1", [instance_id])
            .map_err(|error| format!("无法删除 Codex 实例：{error}"))?;
        if deleted != 1 {
            return Err("codex_instance_not_found".to_string());
        }
        transaction
            .commit()
            .map_err(|error| format!("无法提交实例删除：{error}"))?;
        if lifecycle_dir.exists() {
            fs::remove_dir_all(lifecycle_dir)
                .map_err(|error| format!("无法删除实例私有目录：{error}"))?;
        }
        Ok(())
    }

    pub fn ensure_instance_deletable(&self, instance_id: &str) -> Result<PathBuf, String> {
        self.ensure_owned_instance(instance_id)?;
        let (state, lifecycle_dir, lock_active) = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
            connection
                .query_row(
                    "SELECT i.state, i.lifecycle_dir,
                            EXISTS(
                                SELECT 1 FROM runtime_configuration_locks l
                                WHERE l.instance_id=i.id AND l.released_at IS NULL
                            )
                     FROM codex_instances i WHERE i.id=?1",
                    [instance_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, bool>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| format!("无法读取 Codex 实例：{error}"))?
                .ok_or_else(|| "codex_instance_not_found".to_string())?
        };
        if !InstanceState::is_finished(state.as_str()) {
            return Err("instance_not_terminal".to_string());
        }
        if lock_active {
            return Err("instance_lock_active".to_string());
        }
        let lifecycle_dir = PathBuf::from(lifecycle_dir);
        if lifecycle_dir.parent() != Some(self.instances_root.as_path()) {
            return Err("instance_lifecycle_path_invalid".to_string());
        }
        Ok(lifecycle_dir)
    }
    pub(super) fn reconcile_all(&self) -> Result<(), String> {
        let ids = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
            let mut statement = connection
                .prepare(&format!(
                    "SELECT instance.id FROM codex_instances instance
                     JOIN launch_configuration_revisions configuration ON configuration.id=instance.launch_configuration_revision_id
                     LEFT JOIN launch_presets preset ON preset.id=configuration.launch_preset_id
                     WHERE instance.state IN ({ACTIVE_INSTANCE_STATES})
                       AND COALESCE(json_extract(configuration.config_snapshot, '$.launchTarget'), preset.entry_kind, 'cli') IN ('cli','managed_app')"
                ))
                .map_err(|error| format!("无法读取活动实例：{error}"))?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| format!("无法读取活动实例：{error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("无法解析活动实例：{error}"))?;
            ids
        };
        for id in ids {
            if let Some(receipt) = self.instance_receipt(&id)? {
                self.apply_receipt(&receipt)?;
            }
        }
        Ok(())
    }

    pub(super) fn instance_receipt(
        &self,
        instance_id: &str,
    ) -> Result<Option<LauncherReceipt>, String> {
        self.ensure_owned_instance(instance_id)?;
        let (lifecycle_dir, launch_nonce, codex_home) = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
            connection
                .query_row(
                    "SELECT lifecycle_dir, launch_nonce, managed_codex_home
                     FROM codex_instances WHERE id=?1",
                    [instance_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| format!("无法读取实例回执路径：{error}"))?
                .ok_or_else(|| "codex_instance_not_found".to_string())?
        };
        let path = Path::new(&lifecycle_dir).join("status.json");
        if !path.is_file() {
            return Ok(None);
        }
        let receipt = read_receipt(&path)?;
        if receipt.instance_id != instance_id
            || receipt.launch_nonce != launch_nonce
            || canonical_or_original(Path::new(&receipt.codex_home))
                != canonical_or_original(Path::new(&codex_home))
        {
            self.mark_launch_failed(instance_id, false)?;
            return Err("launch_receipt_mismatch".to_string());
        }
        Ok(Some(receipt))
    }

    pub(super) fn ensure_owned_instance(&self, instance_id: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "codex_control_database_lock_failed".to_string())?;
        let target: String = connection.query_row(
            "SELECT COALESCE(json_extract(configuration.config_snapshot, '$.launchTarget'), preset.entry_kind, 'cli')
             FROM codex_instances instance
             JOIN launch_configuration_revisions configuration ON configuration.id=instance.launch_configuration_revision_id
             LEFT JOIN launch_presets preset ON preset.id=configuration.launch_preset_id
             WHERE instance.id=?1",
            [instance_id], |row| row.get(0),
        ).map_err(|_| "codex_instance_not_found".to_string())?;
        if !matches!(target.as_str(), "cli" | "managed_app") {
            return Err("compatibility_mode_removed".to_string());
        }
        Ok(())
    }

    pub(super) fn apply_receipt(&self, receipt: &LauncherReceipt) -> Result<(), String> {
        match receipt.state {
            LauncherState::Running => {
                match process_start_identity(receipt.codex_pid) {
                    Ok(identity) if identity == receipt.process_start_identity => {}
                    Ok(_) => {
                        self.finish_instance_without_receipt(&receipt.instance_id)?;
                        return Ok(());
                    }
                    Err(_) if !process_exists(receipt.codex_pid) => {
                        self.finish_instance_without_receipt(&receipt.instance_id)?;
                        return Ok(());
                    }
                    Err(_) => return Ok(()),
                }
                let connection = self
                    .connection
                    .lock()
                    .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
                connection
                    .execute(
                        "UPDATE codex_instances
                         SET state='running', supervisor_pid=?2, pid=?3,
                             process_started_at=?4, launched_at=COALESCE(launched_at, ?5)
                         WHERE id=?1 AND state IN ('launching','orphaned','running')",
                        params![
                            receipt.instance_id,
                            receipt.launcher_pid,
                            receipt.codex_pid,
                            receipt.process_start_identity,
                            unix_now_seconds(),
                        ],
                    )
                    .map_err(|error| format!("无法提交启动回执：{error}"))?;
                connection
                    .execute("UPDATE launch_presets SET is_preferred=0", [])
                    .map_err(|error| format!("无法更新常用启动预设：{error}"))?;
                connection
                    .execute(
                        "UPDATE launch_presets SET is_preferred=1
                         WHERE id=(
                           SELECT launch_preset_id FROM launch_configuration_revisions
                           WHERE id=(SELECT launch_configuration_revision_id
                                     FROM codex_instances WHERE id=?1)
                         )",
                        [&receipt.instance_id],
                    )
                    .map_err(|error| format!("无法记录最近启动预设：{error}"))?;
            }
            LauncherState::Exited => {
                let connection = self
                    .connection
                    .lock()
                    .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
                connection
                    .execute(
                        "UPDATE codex_instances
                         SET state='exited', supervisor_pid=?2, pid=?3,
                             process_started_at=?4, exited_at=?5, exit_code=?6
                         WHERE id=?1 AND state NOT IN ('exited','failed')",
                        params![
                            receipt.instance_id,
                            receipt.launcher_pid,
                            receipt.codex_pid,
                            receipt.process_start_identity,
                            unix_now_seconds(),
                            receipt.exit_code,
                        ],
                    )
                    .map_err(|error| format!("无法提交退出回执：{error}"))?;
                release_instance_lock(&connection, &receipt.instance_id)?;
            }
        }
        Ok(())
    }

    pub(super) fn finish_instance_without_receipt(&self, instance_id: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "codex_control_database_lock_failed".to_string())?;
        connection
            .execute(
                "UPDATE codex_instances SET state='exited', exited_at=?2
             WHERE id=?1 AND state NOT IN ('exited','failed')",
                params![instance_id, unix_now_seconds()],
            )
            .map_err(|error| format!("无法确认 Codex 实例退出：{error}"))?;
        release_instance_lock(&connection, instance_id)
    }

    pub(super) fn finish_instance_after_explicit_stop(
        &self,
        instance_id: &str,
    ) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        connection
            .execute(
                "UPDATE codex_instances SET state='exited', exited_at=?2
                 WHERE id=?1 AND state NOT IN ('exited','failed')",
                params![instance_id, unix_now_seconds()],
            )
            .map_err(|error| format!("无法确认实例已明确停止：{error}"))?;
        release_instance_lock(&connection, instance_id)
    }

    pub(super) fn mark_interrupted_gateway_instances_degraded(&self) -> Result<(), String> {
        self.connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?
            .execute(
                &format!(
                    "UPDATE codex_instances SET state='degraded'
                     WHERE state IN ({ACTIVE_INSTANCE_STATES}) AND EXISTS (
                       SELECT 1 FROM launch_configuration_dependencies d
                       WHERE d.launch_configuration_revision_id=codex_instances.launch_configuration_revision_id
                         AND d.resource_kind IN ('gateway', 'gateway_service_revision')
                     )"
                ),
                [],
            )
            .map_err(|error| format!("无法恢复 Gateway 实例状态：{error}"))?;
        Ok(())
    }
}

pub(super) fn locked_instances(
    connection: &Connection,
    kind: &str,
    id: &str,
) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(
            &format!(
                "SELECT DISTINCT l.instance_id
             FROM runtime_configuration_locks l
             JOIN launch_configuration_dependencies d
               ON d.launch_configuration_revision_id=l.launch_configuration_revision_id
             JOIN codex_instances instance ON instance.id=l.instance_id
             LEFT JOIN launch_configuration_revisions configuration
               ON configuration.id=l.launch_configuration_revision_id
             LEFT JOIN launch_presets preset ON preset.id=configuration.launch_preset_id
             WHERE l.released_at IS NULL AND d.resource_kind=?1 AND d.resource_id=?2
               AND instance.state IN ({ACTIVE_INSTANCE_STATES})
               AND (COALESCE(json_extract(configuration.config_snapshot, '$.launchTarget'), preset.entry_kind) IS NULL
                    OR COALESCE(json_extract(configuration.config_snapshot, '$.launchTarget'), preset.entry_kind) NOT IN ('compat_app','compat_cli'))
             ORDER BY l.instance_id"
            ),
        )
        .map_err(|error| format!("无法检查资源配置锁：{error}"))?;
    let ids = statement
        .query_map(params![kind, id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("无法检查资源配置锁：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法解析资源配置锁：{error}"))?;
    Ok(ids)
}

pub(super) fn release_finished_instance_locks(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            &format!(
                "UPDATE runtime_configuration_locks SET released_at=?1
                 WHERE released_at IS NULL AND instance_id IN (
                   SELECT id FROM codex_instances WHERE state NOT IN ({ACTIVE_INSTANCE_STATES})
                 )"
            ),
            params![unix_now_seconds()],
        )
        .map_err(|error| format!("无法释放已结束实例的配置锁：{error}"))?;
    Ok(())
}

pub(super) fn release_instance_lock(
    connection: &Connection,
    instance_id: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE runtime_configuration_locks SET released_at=?2
             WHERE instance_id=?1 AND released_at IS NULL",
            params![instance_id, unix_now_seconds()],
        )
        .map_err(|error| format!("无法释放实例配置锁：{error}"))?;
    Ok(())
}
