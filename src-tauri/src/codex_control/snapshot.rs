//! 控制面快照：账户、渠道、预设和实例行。
//!
//! 不创建或停止实例。首页和抽屉通过 snapshot 读当前状态。

use std::fs;

use rusqlite::OptionalExtension;

use super::helpers::{
    managed_app_compatibility_enabled, sanitize_startup_log, MAX_EXPOSED_STARTUP_LOG_BYTES,
};
use super::lifecycle::{locked_instances, release_finished_instance_locks};
use super::models::{
    CodexAccountSummary, CodexControlCompatibility, CodexControlSnapshot, CodexInstanceSummary,
    CodexProviderSummary, CodexQuotaSummary, LaunchPresetSummary,
};
use super::CodexControlStore;

impl CodexControlStore {
    pub fn snapshot(&self) -> Result<CodexControlSnapshot, String> {
        self.reconcile_all()?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        release_finished_instance_locks(&connection)?;

        let mut presets_statement = connection
            .prepare(
                "SELECT id, entry_kind, name, credential_kind, api_key_provider_id, codex_account_id,
                        gateway_policy, default_workspace_path, version, is_preferred,
                        COALESCE(profile_options, '{}')
                 FROM launch_presets p
                 WHERE p.deleted_at IS NULL
                   AND COALESCE(p.entry_kind, 'managed_app') = 'managed_app'
                   AND (p.credential_kind = 'none' OR (p.credential_kind = 'api_key' AND EXISTS (
                        SELECT 1 FROM api_key_providers provider
                        JOIN api_key_provider_revisions revision
                          ON revision.id = provider.current_revision_id
                        WHERE provider.id = p.api_key_provider_id
                          AND revision.state = 'active'
                    )) OR (p.credential_kind = 'official' AND EXISTS (
                        SELECT 1 FROM codex_accounts account
                        WHERE account.id = p.codex_account_id
                          AND NOT EXISTS (
                            SELECT 1 FROM official_credential_revisions deleted
                            WHERE deleted.account_id = account.id
                              AND deleted.auth_error_code = 'official_account_deleted'
                          )
                 )))
                 ORDER BY is_preferred DESC, created_at ASC",
            )
            .map_err(|error| format!("无法读取启动预设：{error}"))?;
        let presets = presets_statement
            .query_map([], launch_preset_summary_from_row)
            .map_err(|error| format!("无法读取启动预设：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法解析启动预设：{error}"))?;

        let mut provider_statement = connection
            .prepare(
                "SELECT provider.id, provider.name, provider.base_url,
                        provider.is_default, provider.configured,
                        provider.transport_mode, provider.current_revision_id
                 FROM api_key_providers provider
                 JOIN api_key_provider_revisions revision
                   ON revision.id = provider.current_revision_id
                 WHERE revision.state = 'active'
                 ORDER BY provider.is_default DESC, provider.created_at ASC",
            )
            .map_err(|error| format!("无法读取 API Key Provider：{error}"))?;
        let providers = provider_statement
            .query_map([], |row| {
                Ok(CodexProviderSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    base_url: row.get(2)?,
                    is_default: row.get(3)?,
                    configured: row.get(4)?,
                    transport_mode: row.get(5)?,
                    current_revision_id: row.get(6)?,
                    locked_by_instances: Vec::new(),
                })
            })
            .map_err(|error| format!("无法读取 API Key Provider：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法解析 API Key Provider：{error}"))?;

        let mut account_statement = connection
            .prepare(
                "SELECT a.id, a.masked_identity, a.workspace_label, a.plan_label,
                        a.current_credential_revision_id, r.auth_state,
                        r.auth_checked_at, r.auth_error_code, r.codex_version,
                        r.schema_fingerprint, q.primary_used_percent,
                        q.primary_window_minutes, q.primary_resets_at,
                        q.secondary_used_percent, q.secondary_window_minutes,
                        q.secondary_resets_at, q.refreshed_at, q.expires_at,
                        q.error_code
                 FROM codex_accounts a
                 LEFT JOIN official_credential_revisions r
                   ON r.id = a.current_credential_revision_id
                 LEFT JOIN codex_quota_snapshots q
                   ON q.id = (
                      SELECT latest.id FROM codex_quota_snapshots latest
                      WHERE latest.account_id = a.id
                        AND latest.official_credential_revision_id = r.id
                      ORDER BY latest.refreshed_at DESC, latest.id DESC
                      LIMIT 1
                   )
                 WHERE NOT EXISTS (
                    SELECT 1 FROM official_credential_revisions deleted
                    WHERE deleted.account_id = a.id
                      AND deleted.auth_error_code = 'official_account_deleted'
                 )
                 ORDER BY a.created_at ASC",
            )
            .map_err(|error| format!("无法读取官方账户：{error}"))?;
        let accounts = account_statement
            .query_map([], |row| {
                let quota_refreshed_at: Option<i64> = row.get(16)?;
                let quota = if let Some(refreshed_at) = quota_refreshed_at {
                    Some(CodexQuotaSummary {
                        primary_used_percent: row.get(10)?,
                        primary_window_minutes: row.get(11)?,
                        primary_resets_at: row.get(12)?,
                        secondary_used_percent: row.get(13)?,
                        secondary_window_minutes: row.get(14)?,
                        secondary_resets_at: row.get(15)?,
                        refreshed_at,
                        expires_at: row.get::<_, Option<i64>>(17)?.unwrap_or(refreshed_at),
                        error_code: row.get(18)?,
                    })
                } else {
                    None
                };
                Ok(CodexAccountSummary {
                    id: row.get(0)?,
                    masked_identity: row.get(1)?,
                    workspace_label: row.get(2)?,
                    plan_label: row.get(3)?,
                    current_credential_revision_id: row.get(4)?,
                    auth_state: row.get(5)?,
                    auth_checked_at: row.get(6)?,
                    auth_error_code: row.get(7)?,
                    codex_version: row.get(8)?,
                    schema_fingerprint: row.get(9)?,
                    quota,
                    locked_by_instances: Vec::new(),
                })
            })
            .map_err(|error| format!("无法读取官方账户：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法解析官方账户：{error}"))?;

        let mut instance_statement = connection
            .prepare(
                "SELECT i.id, i.launch_configuration_revision_id,
                        c.launch_preset_id,
                        COALESCE(
                            json_extract(c.config_snapshot, '$.launchTarget'),
                            p.entry_kind
                        ),
                        i.credential_kind,
                        COALESCE(i.api_key_provider_revision_id,
                                 i.official_credential_revision_id),
                        i.workspace_path, i.state, i.supervisor_pid, i.pid,
                        i.launched_at, i.exited_at, i.exit_code
                 FROM codex_instances i
                 JOIN launch_configuration_revisions c
                   ON c.id = i.launch_configuration_revision_id
                 LEFT JOIN launch_presets p
                   ON p.id = c.launch_preset_id
                 WHERE COALESCE(
                           json_extract(c.config_snapshot, '$.launchTarget'),
                           p.entry_kind
                       ) IS NULL
                    OR COALESCE(
                           json_extract(c.config_snapshot, '$.launchTarget'),
                           p.entry_kind
                       ) NOT IN ('compat_app','compat_cli')
                 ORDER BY COALESCE(i.launched_at, c.created_at) DESC",
            )
            .map_err(|error| format!("无法读取 Codex 实例：{error}"))?;
        let mut instances = instance_statement
            .query_map([], |row| {
                Ok(CodexInstanceSummary {
                    id: row.get(0)?,
                    launch_configuration_revision_id: row.get(1)?,
                    launch_preset_id: row.get(2)?,
                    launch_target: row.get(3)?,
                    credential_kind: row.get(4)?,
                    credential_revision_id: row.get(5)?,
                    workspace_path: row.get(6)?,
                    state: row.get(7)?,
                    supervisor_pid: row.get(8)?,
                    pid: row.get(9)?,
                    launched_at: row.get(10)?,
                    exited_at: row.get(11)?,
                    exit_code: row.get(12)?,
                    startup_log: None,
                })
            })
            .map_err(|error| format!("无法读取 Codex 实例：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法解析 Codex 实例：{error}"))?;
        drop(instance_statement);
        for instance in &mut instances {
            instance.startup_log = self.read_startup_log(&instance.id);
        }
        drop(account_statement);
        drop(provider_statement);
        drop(presets_statement);

        let mut presets = presets;
        for preset in &mut presets {
            preset.locked_by_instances =
                locked_instances(&connection, "launch_preset", &preset.id)?;
        }
        let mut accounts = accounts;
        for account in &mut accounts {
            account.locked_by_instances =
                locked_instances(&connection, "official_account", &account.id)?;
        }
        let mut providers = providers;
        for provider in &mut providers {
            provider.locked_by_instances =
                locked_instances(&connection, "api_key_provider", &provider.id)?;
        }
        Ok(CodexControlSnapshot {
            accounts,
            providers,
            presets,
            instances,
            compatibility: CodexControlCompatibility {
                control_plane_available: true,
                // Official OAuth is retained in storage but intentionally
                // disabled in the current product release.
                official_accounts_available: false,
                managed_app_available: managed_app_compatibility_enabled(),
                official_hybrid_available: false,
            },
        })
    }

    pub(super) fn instance(&self, id: &str) -> Result<CodexInstanceSummary, String> {
        let mut instance = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?
            .query_row(
                "SELECT i.id, i.launch_configuration_revision_id,
                        c.launch_preset_id,
                        COALESCE(
                            json_extract(c.config_snapshot, '$.launchTarget'),
                            p.entry_kind
                        ),
                        i.credential_kind,
                        COALESCE(i.api_key_provider_revision_id,
                                 i.official_credential_revision_id),
                        i.workspace_path, i.state, i.supervisor_pid, i.pid,
                        i.launched_at, i.exited_at, i.exit_code
                 FROM codex_instances i
                 JOIN launch_configuration_revisions c
                   ON c.id=i.launch_configuration_revision_id
                 LEFT JOIN launch_presets p
                   ON p.id=c.launch_preset_id
                 WHERE i.id=?1",
                [id],
                |row| {
                    Ok(CodexInstanceSummary {
                        id: row.get(0)?,
                        launch_configuration_revision_id: row.get(1)?,
                        launch_preset_id: row.get(2)?,
                        launch_target: row.get(3)?,
                        credential_kind: row.get(4)?,
                        credential_revision_id: row.get(5)?,
                        workspace_path: row.get(6)?,
                        state: row.get(7)?,
                        supervisor_pid: row.get(8)?,
                        pid: row.get(9)?,
                        launched_at: row.get(10)?,
                        exited_at: row.get(11)?,
                        exit_code: row.get(12)?,
                        startup_log: None,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("无法读取 Codex 实例：{error}"))?
            .ok_or_else(|| "codex_instance_not_found".to_string())?;
        instance.startup_log = self.read_startup_log(id);
        Ok(instance)
    }

    pub(super) fn read_startup_log(&self, instance_id: &str) -> Option<String> {
        if instance_id.is_empty()
            || instance_id == "."
            || instance_id == ".."
            || instance_id.contains('/')
            || instance_id.contains('\\')
        {
            return None;
        }
        let path = self.instances_root.join(instance_id).join("startup.log");
        let bytes = fs::read(path).ok()?;
        let start = bytes.len().saturating_sub(MAX_EXPOSED_STARTUP_LOG_BYTES);
        Some(sanitize_startup_log(&String::from_utf8_lossy(
            &bytes[start..],
        )))
    }
    pub(super) fn preset(&self, id: &str) -> Result<LaunchPresetSummary, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        let mut preset = connection
            .query_row(
                "SELECT id, entry_kind, name, credential_kind, api_key_provider_id,
                        codex_account_id, gateway_policy, default_workspace_path,
                        version, is_preferred, COALESCE(profile_options, '{}')
                 FROM launch_presets WHERE id=?1 AND deleted_at IS NULL",
                [id],
                launch_preset_summary_from_row,
            )
            .optional()
            .map_err(|error| format!("无法读取启动预设：{error}"))?
            .ok_or_else(|| "launch_preset_not_found".to_string())?;
        preset.locked_by_instances = locked_instances(&connection, "launch_preset", id)?;
        Ok(preset)
    }
}

fn launch_preset_summary_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<LaunchPresetSummary> {
    let profile_options: String = row.get(10)?;
    let login_provider_id = serde_json::from_str::<serde_json::Value>(&profile_options)
        .ok()
        .and_then(|value| {
            value
                .get("loginProviderId")
                .and_then(|item| item.as_str())
                .map(|item| item.trim().to_string())
        })
        .filter(|item| !item.is_empty());
    Ok(LaunchPresetSummary {
        id: row.get(0)?,
        entry_kind: row.get(1)?,
        name: row.get(2)?,
        credential_kind: row.get(3)?,
        api_key_provider_id: row.get(4)?,
        codex_account_id: row.get(5)?,
        gateway_policy: row.get(6)?,
        default_workspace_path: row.get(7)?,
        version: row.get(8)?,
        is_preferred: row.get(9)?,
        locked_by_instances: Vec::new(),
        login_provider_id,
    })
}
