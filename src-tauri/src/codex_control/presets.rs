//! 启动预设的创建、更名、更新和墓碑删除。
//!
//! 不启动实例。版本冲突和占用锁仍走现有错误字符串。

use rusqlite::{params, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use crate::support::unix_now_seconds;

use super::launch::{ensure_no_operation_lease, reject_locked_resource};
use super::models::{
    CreateLaunchPresetInput, CredentialKind, LaunchPresetSummary, UpdateLaunchPresetInput,
};
use super::CodexControlStore;

impl CodexControlStore {
    pub fn create_preset(
        &self,
        input: CreateLaunchPresetInput,
    ) -> Result<LaunchPresetSummary, String> {
        validate_preset_input(
            &input.credential_kind,
            input.api_key_provider_id.as_deref(),
            input.codex_account_id.as_deref(),
            input.gateway_policy.as_deref(),
        )?;
        let name = validate_preset_name(&input.name)?;
        let id = Uuid::new_v4().to_string();
        let now = unix_now_seconds();
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("无法开始预设事务：{error}"))?;
        let credential_available: bool = if CredentialKind::wire_is_api_key(&input.credential_kind)
        {
            transaction
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM api_key_providers provider
                        JOIN api_key_provider_revisions revision
                          ON revision.id=provider.current_revision_id
                        WHERE provider.id=?1 AND revision.state='active'
                    )",
                    [input.api_key_provider_id.as_deref().unwrap_or_default()],
                    |row| row.get(0),
                )
                .map_err(|error| format!("无法检查 Provider：{error}"))?
        } else {
            transaction
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM codex_accounts account
                        WHERE account.id=?1 AND NOT EXISTS (
                            SELECT 1 FROM official_credential_revisions deleted
                            WHERE deleted.account_id=account.id
                              AND deleted.auth_error_code='official_account_deleted'
                        )
                    )",
                    [input.codex_account_id.as_deref().unwrap_or_default()],
                    |row| row.get(0),
                )
                .map_err(|error| format!("无法检查官方账户：{error}"))?
        };
        if !credential_available {
            return Err("credential_missing".to_string());
        }
        if input.is_preferred {
            transaction
                .execute("UPDATE launch_presets SET is_preferred=0", [])
                .map_err(|error| format!("无法更新常用预设：{error}"))?;
        }
        transaction
            .execute(
                "INSERT INTO launch_presets (
                    id, entry_kind, name, credential_kind, api_key_provider_id,
                    codex_account_id, gateway_policy, profile_options,
                    default_workspace_path, version, is_preferred,
                    created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '{}', ?8, 1, ?9, ?10, ?10)",
                params![
                    id,
                    input.entry_kind,
                    name,
                    input.credential_kind,
                    input.api_key_provider_id,
                    input.codex_account_id,
                    input.gateway_policy,
                    input.default_workspace_path,
                    input.is_preferred,
                    now,
                ],
            )
            .map_err(|error| format!("无法创建启动预设：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("无法提交启动预设：{error}"))?;
        self.preset(&id)
    }

    pub fn update_preset(
        &self,
        id: &str,
        input: UpdateLaunchPresetInput,
    ) -> Result<LaunchPresetSummary, String> {
        let name = validate_preset_name(&input.name)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("无法开始预设事务：{error}"))?;
        reject_locked_resource(&transaction, "launch_preset", id)?;
        ensure_no_operation_lease(&transaction, &[("launch_preset", id)])?;
        if input.is_preferred {
            transaction
                .execute("UPDATE launch_presets SET is_preferred=0", [])
                .map_err(|error| format!("无法更新常用预设：{error}"))?;
        }
        let changed = transaction
            .execute(
                "UPDATE launch_presets
                 SET name=?1, default_workspace_path=?2, version=version+1,
                     is_preferred=?3, updated_at=?4
                 WHERE id=?5 AND version=?6",
                params![
                    name,
                    input.default_workspace_path,
                    input.is_preferred,
                    unix_now_seconds(),
                    id,
                    input.expected_version,
                ],
            )
            .map_err(|error| format!("无法更新启动预设：{error}"))?;
        if changed != 1 {
            return Err("version_conflict".to_string());
        }
        transaction
            .commit()
            .map_err(|error| format!("无法提交启动预设：{error}"))?;
        self.preset(id)
    }

    pub fn rename_preset(
        &self,
        id: &str,
        name: &str,
        expected_version: i64,
    ) -> Result<LaunchPresetSummary, String> {
        let name = validate_preset_name(name)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("无法开始预设事务：{error}"))?;
        ensure_no_operation_lease(&transaction, &[("launch_preset", id)])?;
        let current: Option<(String, i64, Option<i64>)> = transaction
            .query_row(
                "SELECT name, version, deleted_at FROM launch_presets WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| format!("无法读取启动预设：{error}"))?;
        let Some((current_name, version, deleted_at)) = current else {
            return Err("launch_preset_not_found".to_string());
        };
        if deleted_at.is_some() {
            return Err("launch_preset_not_found".to_string());
        }
        if version != expected_version {
            return Err("version_conflict".to_string());
        }
        if current_name != name {
            let changed = transaction
                .execute(
                    "UPDATE launch_presets
                     SET name=?1, version=version+1, updated_at=?2
                     WHERE id=?3 AND version=?4 AND deleted_at IS NULL",
                    params![name, unix_now_seconds(), id, expected_version],
                )
                .map_err(|error| format!("无法更新启动预设：{error}"))?;
            if changed != 1 {
                return Err("version_conflict".to_string());
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("无法提交启动预设：{error}"))?;
        drop(connection);
        self.preset(id)
    }

    pub fn delete_preset(&self, id: &str) -> Result<(), String> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "Codex 控制数据库锁已损坏。".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("无法开始预设事务：{error}"))?;
        reject_locked_resource(&transaction, "launch_preset", id)?;
        ensure_no_operation_lease(&transaction, &[("launch_preset", id)])?;
        let deleted = transaction
            .execute(
                "UPDATE launch_presets
                 SET deleted_at=?1, is_preferred=0, version=version+1, updated_at=?1
                 WHERE id=?2 AND deleted_at IS NULL",
                params![unix_now_seconds(), id],
            )
            .map_err(|error| format!("无法删除启动预设：{error}"))?;
        if deleted != 1 {
            return Err("launch_preset_not_found".to_string());
        }
        transaction
            .commit()
            .map_err(|error| format!("无法提交启动预设删除：{error}"))
    }
}

pub(crate) fn validate_preset_input(
    credential_kind: &str,
    provider_id: Option<&str>,
    account_id: Option<&str>,
    gateway_policy: Option<&str>,
) -> Result<(), String> {
    match credential_kind {
        "api_key" if provider_id.is_some() && account_id.is_none() => {
            if matches!(
                gateway_policy,
                Some("direct" | "gateway_passthrough" | "gateway_chat_completions")
            ) {
                Ok(())
            } else {
                Err("invalid_gateway_policy".to_string())
            }
        }
        "official" if provider_id.is_none() && account_id.is_some() && gateway_policy.is_none() => {
            Ok(())
        }
        _ => Err("invalid_launch_preset".to_string()),
    }
}

pub(super) fn validate_preset_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 80 || value.chars().any(char::is_control) {
        return Err("invalid_launch_preset_name".to_string());
    }
    Ok(value.to_string())
}
