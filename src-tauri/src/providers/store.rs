//! ProviderStore：渠道创建、更新、改名、默认选择和删除。
//!
//! 不解析上游凭据；中断恢复和租约收尾在 operations。
//! `from_database` 给 App 装配用，测试走 `in_memory`。

use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::database::{
    resource_guard::{ResourceGuard, ResourceKind, ResourceRef},
    Database,
};
use crate::secrets::{validate_api_key, ApiKeyVault};
use crate::support::unix_now_seconds;

use super::models::{
    CreateProviderInput, ProviderSummary, UpdateProviderInput, PROVIDER_RECORD_COLUMNS,
};
use super::operations::{
    command_error_string, provider_operation_resources, revision_secret_ref,
    save_gateway_timeout_in_transaction, validate_gateway_timeout, validate_name, wire_api,
};
use super::validate_base_url;

#[derive(Clone)]
pub struct ProviderStore {
    pub(super) connection: Arc<Mutex<Connection>>,
    pub(super) vault: Arc<dyn ApiKeyVault>,
    pub(super) resource_guard: ResourceGuard,
}

impl ProviderStore {
    pub fn from_database(database: Database, vault: Arc<dyn ApiKeyVault>) -> Result<Self, String> {
        let store = Self {
            connection: database.connection(),
            vault,
            resource_guard: ResourceGuard::new(database),
        };
        store.recover_interrupted_provider_operations()?;
        Ok(store)
    }
    #[cfg(test)]
    pub(super) fn in_memory(vault: Arc<dyn ApiKeyVault>) -> Self {
        let database = Database::in_memory();
        Self::from_database(database, vault).unwrap()
    }
    pub fn list(&self) -> Result<Vec<ProviderSummary>, String> {
        let records = self.records(
            &format!(
                "SELECT {PROVIDER_RECORD_COLUMNS} FROM api_key_providers
             WHERE current_revision_id IS NOT NULL
             ORDER BY is_default DESC, created_at ASC"
            ),
            [],
        )?;
        records
            .into_iter()
            .map(|record| {
                Ok(ProviderSummary {
                    configured: record.configured,
                    id: record.id,
                    name: record.name,
                    base_url: record.base_url,
                    is_default: record.is_default,
                    transport_mode: record.transport_mode,
                })
            })
            .collect()
    }

    pub fn create(&self, input: CreateProviderInput) -> Result<ProviderSummary, String> {
        let name = validate_name(input.name)?;
        let base_url = validate_base_url(input.base_url)?;
        let api_key = validate_api_key(input.api_key)?;
        let id = Uuid::new_v4().to_string();
        let revision_id = Uuid::new_v4().to_string();
        let secret_ref = revision_secret_ref(&id, &revision_id);
        let created_at = unix_now_seconds();
        let transport_mode = input.transport_mode;
        validate_gateway_timeout(input.gateway_timeout_settings.as_ref())?;
        let operation_id = Uuid::new_v4().to_string();
        let resources = vec![
            ResourceRef::revision(ResourceKind::ApiKeyProvider, &id, &revision_id),
            ResourceRef::logical(ResourceKind::ApiKeyProviderRevision, &revision_id),
            ResourceRef::logical(ResourceKind::Gateway, &id),
        ];
        self.resource_guard
            .acquire_operation_leases(
                &operation_id,
                "create_api_key_provider",
                Some(std::process::id() as i64),
                Some(&secret_ref),
                &resources,
                created_at,
            )
            .map_err(command_error_string)?;
        if let Err(error) = self.vault.save(&secret_ref, &api_key) {
            return Err(self.finish_operation_error(&operation_id, error, false));
        }
        let write = (|| -> Result<bool, String> {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "本地数据库锁已损坏。".to_string())?;
            let transaction = connection
                .unchecked_transaction()
                .map_err(|error| error.to_string())?;
            let is_default: bool = transaction
                .query_row(
                    "SELECT NOT EXISTS(
                        SELECT 1 FROM api_key_providers
                        WHERE current_revision_id IS NOT NULL
                    )",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "INSERT INTO api_key_providers (
                        id, name, base_url, secret_ref, is_default,
                        transport_mode, configured, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
                    params![
                        id,
                        name,
                        base_url,
                        secret_ref,
                        is_default,
                        transport_mode.as_db(),
                        created_at,
                    ],
                )
                .map_err(|error| format!("无法保存 Provider 元数据：{error}"))?;
            transaction
                .execute(
                    "INSERT INTO api_key_provider_revisions (
                        id, provider_id, secret_ref, base_url, wire_api,
                        transport_mode, state, created_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7)",
                    params![
                        revision_id,
                        id,
                        secret_ref,
                        base_url,
                        wire_api(transport_mode),
                        transport_mode.as_db(),
                        created_at,
                    ],
                )
                .map_err(|error| format!("无法保存 Provider 修订：{error}"))?;
            transaction
                .execute(
                    "UPDATE api_key_providers SET current_revision_id = ?1 WHERE id = ?2",
                    params![revision_id, id],
                )
                .map_err(|error| format!("无法激活 Provider 修订：{error}"))?;
            save_gateway_timeout_in_transaction(
                &transaction,
                &id,
                input.gateway_timeout_settings.as_ref(),
            )?;
            transaction
                .commit()
                .map_err(|error| format!("无法完成 Provider 创建：{error}"))?;
            Ok(is_default)
        })();
        let is_default = match write {
            Ok(is_default) => is_default,
            Err(error) => {
                return match self.vault.clear(&secret_ref) {
                    Ok(()) => Err(self.finish_operation_error(
                        &operation_id,
                        format!("{error}；新 API Key 已回收。"),
                        false,
                    )),
                    Err(clear_error) => Err(self.finish_operation_error(
                        &operation_id,
                        format!("{error}；同时无法回收新 API Key：{clear_error}"),
                        true,
                    )),
                }
            }
        };
        self.finish_operation_success(&operation_id)?;
        Ok(ProviderSummary {
            id,
            name,
            base_url,
            is_default,
            configured: true,
            transport_mode,
        })
    }

    pub fn update(&self, id: &str, input: UpdateProviderInput) -> Result<ProviderSummary, String> {
        let record = self
            .record_by_id(id)?
            .ok_or_else(|| "找不到指定 Provider。".to_string())?;
        let name = validate_name(input.name)?;
        let base_url = validate_base_url(input.base_url)?;
        let replacement_key = match input.api_key {
            Some(value) if !value.is_empty() => Some(validate_api_key(value)?),
            _ => None,
        };
        let transport_mode = input.transport_mode;
        validate_gateway_timeout(input.gateway_timeout_settings.as_ref())?;
        let creates_revision = replacement_key.is_some()
            || base_url != record.base_url
            || transport_mode != record.transport_mode;
        let revision_id = creates_revision.then(|| Uuid::new_v4().to_string());
        let secret_ref = revision_id
            .as_deref()
            .map(|revision_id| revision_secret_ref(id, revision_id));
        let operation_id = Uuid::new_v4().to_string();
        let resources = provider_operation_resources(&record);
        self.resource_guard
            .acquire_operation_leases(
                &operation_id,
                "update_api_key_provider",
                Some(std::process::id() as i64),
                secret_ref.as_deref(),
                &resources,
                unix_now_seconds(),
            )
            .map_err(command_error_string)?;

        let new_key = if let Some(api_key) = replacement_key {
            Some(api_key)
        } else if creates_revision && record.configured {
            match self.vault.load(&record.secret_ref) {
                Ok(Some(api_key)) => Some(api_key),
                Ok(None) => {
                    return Err(self.finish_operation_error(
                        &operation_id,
                        "当前 API Key 已不存在，请提供新 API Key 后重试。".to_string(),
                        false,
                    ))
                }
                Err(error) => return Err(self.finish_operation_error(&operation_id, error, false)),
            }
        } else {
            None
        };
        let configured = if creates_revision {
            new_key.is_some()
        } else {
            record.configured
        };
        let mut wrote_new_secret = false;
        if let (Some(secret_ref), Some(api_key)) = (secret_ref.as_deref(), new_key.as_ref()) {
            if let Err(error) = self.vault.save(secret_ref, api_key) {
                return Err(self.finish_operation_error(&operation_id, error, false));
            }
            wrote_new_secret = true;
        }

        let updated_at = unix_now_seconds();
        let update_result = (|| -> Result<(), String> {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "本地数据库锁已损坏。".to_string())?;
            let transaction = connection
                .unchecked_transaction()
                .map_err(|error| error.to_string())?;
            if let (Some(revision_id), Some(secret_ref)) =
                (revision_id.as_deref(), secret_ref.as_deref())
            {
                transaction
                    .execute(
                        "INSERT INTO api_key_provider_revisions (
                            id, provider_id, secret_ref, base_url, wire_api,
                            transport_mode, state, created_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            revision_id,
                            id,
                            secret_ref,
                            base_url,
                            wire_api(transport_mode),
                            transport_mode.as_db(),
                            if configured {
                                "active"
                            } else {
                                "credential_missing"
                            },
                            updated_at,
                        ],
                    )
                    .map_err(|error| format!("无法创建 Provider 修订：{error}"))?;
                transaction
                    .execute(
                        "UPDATE api_key_provider_revisions
                         SET state = 'retired', retired_at = ?1
                         WHERE id = ?2 AND state != 'retired'",
                        params![updated_at, record.current_revision_id],
                    )
                    .map_err(|error| format!("无法退役旧 Provider 修订：{error}"))?;
                let updated = transaction
                    .execute(
                        "UPDATE api_key_providers
                         SET name = ?1, base_url = ?2, secret_ref = ?3,
                             transport_mode = ?4, configured = ?5,
                             current_revision_id = ?6, updated_at = ?7
                         WHERE id = ?8 AND current_revision_id = ?9",
                        params![
                            name,
                            base_url,
                            secret_ref,
                            transport_mode.as_db(),
                            configured,
                            revision_id,
                            updated_at,
                            id,
                            record.current_revision_id,
                        ],
                    )
                    .map_err(|error| format!("无法更新 Provider：{error}"))?;
                if updated != 1 {
                    return Err("Provider 已被其他操作修改，请刷新后重试。".to_string());
                }
            } else {
                let updated = transaction
                    .execute(
                        "UPDATE api_key_providers SET name = ?1, updated_at = ?2
                         WHERE id = ?3 AND current_revision_id = ?4",
                        params![name, updated_at, id, record.current_revision_id],
                    )
                    .map_err(|error| format!("无法更新 Provider：{error}"))?;
                if updated != 1 {
                    return Err("Provider 已被其他操作修改，请刷新后重试。".to_string());
                }
            }
            save_gateway_timeout_in_transaction(
                &transaction,
                id,
                input.gateway_timeout_settings.as_ref(),
            )?;
            transaction
                .commit()
                .map_err(|error| format!("无法完成 Provider 更新：{error}"))
        })();
        if let Err(error) = update_result {
            if wrote_new_secret {
                let secret_ref = secret_ref
                    .as_deref()
                    .expect("saved revision has a secret ref");
                return match self.vault.clear(secret_ref) {
                    Ok(()) => Err(self.finish_operation_error(
                        &operation_id,
                        format!("{error}；新 API Key 已回收，原 API Key 保持不变。"),
                        false,
                    )),
                    Err(clear_error) => Err(self.finish_operation_error(
                        &operation_id,
                        format!("{error}；同时无法回收新 API Key：{clear_error}"),
                        true,
                    )),
                };
            }
            return Err(self.finish_operation_error(&operation_id, error, false));
        }
        if let Err(error) = self.clear_provider_secrets(id, true) {
            return Err(self.finish_operation_error(
                &operation_id,
                format!("Provider 已更新，但无法回收旧 API Key：{error}"),
                true,
            ));
        }
        self.finish_operation_success(&operation_id)?;
        Ok(ProviderSummary {
            id: id.to_string(),
            name,
            base_url,
            is_default: record.is_default,
            configured,
            transport_mode,
        })
    }

    pub fn rename(&self, id: &str, name: String) -> Result<ProviderSummary, String> {
        let name = validate_name(name)?;
        let record = self
            .record_by_id(id)?
            .ok_or_else(|| "找不到指定 Provider。".to_string())?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| "本地数据库锁已损坏。".to_string())?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let leased: bool = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM resource_operation_leases
                    WHERE resource_kind='api_key_provider' AND resource_id=?1
                 )",
                [id],
                |row| row.get(0),
            )
            .map_err(|error| format!("无法检查资源操作租约：{error}"))?;
        if leased {
            return Err("resource_operation_in_progress".to_string());
        }
        let updated_at = unix_now_seconds();
        let updated = transaction
            .execute(
                "UPDATE api_key_providers SET name=?1, updated_at=?2
                 WHERE id=?3 AND current_revision_id=?4",
                params![name, updated_at, id, record.current_revision_id],
            )
            .map_err(|error| format!("无法更新 Provider：{error}"))?;
        if updated != 1 {
            return Err("Provider 已被其他操作修改，请刷新后重试。".to_string());
        }
        transaction
            .commit()
            .map_err(|error| format!("无法完成 Provider 更新：{error}"))?;
        Ok(ProviderSummary {
            id: id.to_string(),
            name,
            base_url: record.base_url,
            is_default: record.is_default,
            configured: record.configured,
            transport_mode: record.transport_mode,
        })
    }

    pub fn select_default(&self, id: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "本地数据库锁已损坏。".to_string())?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        if transaction
            .execute(
                "UPDATE api_key_providers SET is_default = 0
                 WHERE current_revision_id IS NOT NULL",
                [],
            )
            .map_err(|error| error.to_string())?
            == 0
        {
            return Err("尚未创建 Provider。".to_string());
        }
        if transaction
            .execute(
                "UPDATE api_key_providers SET is_default = 1
                 WHERE id = ?1 AND current_revision_id IS NOT NULL",
                [id],
            )
            .map_err(|error| error.to_string())?
            != 1
        {
            return Err("找不到指定 Provider。".to_string());
        }
        transaction
            .commit()
            .map_err(|error| format!("无法选择默认 Provider：{error}"))
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let record = self
            .record_by_id(id)?
            .ok_or_else(|| "找不到指定 Provider。".to_string())?;
        let operation_id = Uuid::new_v4().to_string();
        let resources = provider_operation_resources(&record);
        self.resource_guard
            .acquire_operation_leases(
                &operation_id,
                "delete_api_key_provider",
                Some(std::process::id() as i64),
                Some(&record.secret_ref),
                &resources,
                unix_now_seconds(),
            )
            .map_err(command_error_string)?;
        let delete_result = (|| -> Result<(), String> {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "本地数据库锁已损坏。".to_string())?;
            let transaction = connection
                .unchecked_transaction()
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "DELETE FROM provider_usage_events WHERE provider_id = ?1",
                    [id],
                )
                .map_err(|error| format!("无法清理 Provider 用量记录：{error}"))?;
            transaction
                .execute(
                    "DELETE FROM gateway_diagnostic_events WHERE provider_id = ?1",
                    [id],
                )
                .map_err(|error| format!("无法清理 Provider 诊断记录：{error}"))?;
            transaction
                .execute(
                    "DELETE FROM provider_connectivity_reports WHERE provider_id = ?1",
                    [id],
                )
                .map_err(|error| format!("无法清理 Provider 检测记录：{error}"))?;
            transaction
                .execute(
                    "DELETE FROM gateway_timeout_settings WHERE provider_id = ?1",
                    [id],
                )
                .map_err(|error| format!("无法清理 Provider Gateway 设置：{error}"))?;
            transaction
                .execute(
                    "DELETE FROM provider_upstream_balances WHERE provider_id = ?1",
                    [id],
                )
                .map_err(|error| format!("无法清理 Provider 余额缓存：{error}"))?;
            transaction
                .execute(
                    "UPDATE api_key_provider_revisions
                     SET state = 'retired', retired_at = ?1
                     WHERE id = ?2 AND state != 'retired'",
                    params![unix_now_seconds(), record.current_revision_id],
                )
                .map_err(|error| format!("无法退役 Provider 修订：{error}"))?;
            let tombstoned = transaction
                .execute(
                    "UPDATE api_key_providers
                     SET configured = 0, is_default = 0,
                         current_revision_id = NULL, updated_at = ?1
                     WHERE id = ?2 AND current_revision_id = ?3",
                    params![unix_now_seconds(), id, record.current_revision_id],
                )
                .map_err(|error| format!("无法删除 Provider：{error}"))?;
            if tombstoned != 1 {
                return Err("Provider 已被其他操作修改，请刷新后重试。".to_string());
            }
            if record.is_default {
                if let Some(next) = transaction
                    .query_row(
                        "SELECT id FROM api_key_providers
                         WHERE current_revision_id IS NOT NULL
                         ORDER BY created_at ASC LIMIT 1",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(|error| error.to_string())?
                {
                    transaction
                        .execute(
                            "UPDATE api_key_providers SET is_default = 1 WHERE id = ?1",
                            [next],
                        )
                        .map_err(|error| error.to_string())?;
                }
            }
            transaction
                .commit()
                .map_err(|error| format!("无法完成 Provider 删除：{error}"))
        })();
        if let Err(error) = delete_result {
            return Err(self.finish_operation_error(&operation_id, error, false));
        }
        if let Err(error) = self.clear_provider_secrets(id, false) {
            return Err(self.finish_operation_error(
                &operation_id,
                format!("Provider 已隐藏，但无法回收 API Key 修订：{error}"),
                true,
            ));
        }
        self.finish_operation_success(&operation_id)
    }
}
