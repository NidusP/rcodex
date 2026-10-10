//! Provider 操作辅助：恢复、租约收尾、修订凭据和 URL 校验。
//!
//! 不对外提供 IPC；由 store / upstream 调用。
//! 渠道 CRUD 仍走 ProviderStore 的公开方法。

use std::collections::BTreeMap;

use rusqlite::params;
use url::Url;

use crate::database::resource_guard::{CommandError, ResourceKind, ResourceRef};
use crate::secrets::ApiKey;
use crate::support::unix_now_seconds;

use super::models::{
    ProviderGatewayTimeoutInput, ProviderRecord, TransportMode, PROVIDER_RECORD_COLUMNS,
};
use super::store::ProviderStore;

impl ProviderStore {
    pub(super) fn recover_interrupted_provider_operations(&self) -> Result<(), String> {
        let mut grouped = BTreeMap::new();
        for lease in self
            .resource_guard
            .recoverable_leases()
            .map_err(command_error_string)?
        {
            if matches!(
                lease.operation_kind.as_str(),
                "create_api_key_provider" | "update_api_key_provider" | "delete_api_key_provider"
            ) {
                grouped
                    .entry(lease.operation_id.clone())
                    .or_insert_with(Vec::new)
                    .push(lease);
            }
        }
        for (operation_id, leases) in grouped {
            let operation_kind = leases[0].operation_kind.as_str();
            let recovery_secret_ref = leases[0].recovery_detail.as_deref();
            let provider_id = leases.iter().find_map(|lease| {
                (lease.resource.kind == ResourceKind::ApiKeyProvider)
                    .then(|| lease.resource.id.clone())
            });
            let Some(provider_id) = provider_id else {
                let _ = self.resource_guard.mark_operation_recovering(&operation_id);
                continue;
            };
            let (provider_exists, published_recovery_secret): (bool, bool) = {
                let connection = self
                    .connection
                    .lock()
                    .map_err(|_| "本地数据库锁已损坏。".to_string())?;
                let provider_exists = connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM api_key_providers WHERE id=?1)",
                        [&provider_id],
                        |row| row.get(0),
                    )
                    .map_err(|error| format!("无法核验 Provider 恢复状态：{error}"))?;
                let published = match recovery_secret_ref {
                    Some(secret_ref) => connection
                        .query_row(
                            "SELECT EXISTS(
                                SELECT 1 FROM api_key_providers provider
                                JOIN api_key_provider_revisions revision
                                  ON revision.id=provider.current_revision_id
                                WHERE provider.id=?1 AND revision.secret_ref=?2
                            )",
                            params![provider_id, secret_ref],
                            |row| row.get(0),
                        )
                        .map_err(|error| format!("无法核验 Provider 恢复凭据状态：{error}"))?,
                    None => false,
                };
                (provider_exists, published)
            };

            let cleanup_result = match operation_kind {
                "create_api_key_provider" => match recovery_secret_ref {
                    Some(secret_ref) if !published_recovery_secret => self.vault.clear(secret_ref),
                    Some(_) => Ok(()),
                    None => Err("Provider 创建恢复缺少凭据引用。".to_string()),
                },
                "update_api_key_provider" => {
                    let unpublished_cleanup = match recovery_secret_ref {
                        Some(secret_ref) if !published_recovery_secret => {
                            self.vault.clear(secret_ref)
                        }
                        _ => Ok(()),
                    };
                    unpublished_cleanup.and_then(|_| {
                        if provider_exists {
                            self.clear_provider_secrets(&provider_id, true)
                        } else {
                            Ok(())
                        }
                    })
                }
                "delete_api_key_provider" if !published_recovery_secret && provider_exists => {
                    self.clear_provider_secrets(&provider_id, false)
                }
                "delete_api_key_provider" => Ok(()),
                _ => Ok(()),
            };
            if cleanup_result.is_err() {
                let _ = self.resource_guard.mark_operation_recovering(&operation_id);
                continue;
            }
            self.resource_guard
                .release_operation_leases(&operation_id)
                .map_err(command_error_string)?;
        }
        Ok(())
    }
    pub(super) fn load_active_revision_key(
        &self,
        _provider_id: &str,
        revision_secret_ref: &str,
    ) -> Result<ApiKey, String> {
        let api_key = self
            .vault
            .load(revision_secret_ref)?
            .ok_or_else(|| "provider_credential_missing".to_string())?;
        Ok(api_key)
    }

    pub(super) fn finish_operation_success(&self, operation_id: &str) -> Result<(), String> {
        match self.resource_guard.release_operation_leases(operation_id) {
            Ok(_) => Ok(()),
            Err(error) => {
                let release_error = command_error_string(error);
                let recovery_error = self
                    .resource_guard
                    .mark_operation_recovering(operation_id)
                    .err()
                    .map(command_error_string);
                Err(match recovery_error {
                    Some(recovery_error) => format!(
                        "操作已提交但资源租约释放失败：{release_error}；同时无法标记恢复：{recovery_error}"
                    ),
                    None => format!("操作已提交但资源租约释放失败：{release_error}"),
                })
            }
        }
    }

    pub(super) fn finish_operation_error(
        &self,
        operation_id: &str,
        error: String,
        needs_recovery: bool,
    ) -> String {
        let lease_result = if needs_recovery {
            self.resource_guard
                .mark_operation_recovering(operation_id)
                .map(|_| ())
        } else {
            self.resource_guard
                .release_operation_leases(operation_id)
                .map(|_| ())
        };
        match lease_result {
            Ok(()) => error,
            Err(lease_error) => format!(
                "{error}；资源操作状态更新失败：{}",
                command_error_string(lease_error)
            ),
        }
    }

    pub(super) fn api_key_for_record(&self, record: &ProviderRecord) -> Result<ApiKey, String> {
        if !record.configured {
            return Err(format!("Provider “{}” 未配置 API Key。", record.name));
        }
        match self.vault.load(&record.secret_ref)? {
            Some(api_key) => Ok(api_key),
            None => {
                self.set_configured(&record.id, false).map_err(|error| {
                    format!(
                        "Provider “{}” 的 API Key 已不存在；同时无法更新本地状态：{error}",
                        record.name
                    )
                })?;
                Err(format!(
                    "Provider “{}” 的 API Key 已不存在，请重新编辑并保存。",
                    record.name
                ))
            }
        }
    }

    fn set_configured(&self, id: &str, configured: bool) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "本地数据库锁已损坏。".to_string())?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let updated = transaction
            .execute(
                "UPDATE api_key_providers SET configured = ?1 WHERE id = ?2
                 AND current_revision_id IS NOT NULL",
                params![configured, id],
            )
            .map_err(|error| format!("无法更新 Provider 凭据状态：{error}"))?;
        if updated == 1 {
            transaction
                .execute(
                    "UPDATE api_key_provider_revisions
                     SET state = CASE WHEN ?1 THEN 'active' ELSE 'credential_missing' END,
                         retired_at = NULL
                     WHERE id = (
                         SELECT current_revision_id FROM api_key_providers WHERE id = ?2
                     )",
                    params![configured, id],
                )
                .map_err(|error| format!("无法更新 Provider 修订凭据状态：{error}"))?;
            transaction
                .commit()
                .map_err(|error| format!("无法完成 Provider 凭据状态更新：{error}"))?;
        }
        if updated == 1 {
            Ok(())
        } else {
            Err("找不到指定 Provider。".to_string())
        }
    }

    pub(super) fn record_by_id(&self, id: &str) -> Result<Option<ProviderRecord>, String> {
        self.records(
            &format!(
                "SELECT {PROVIDER_RECORD_COLUMNS} FROM api_key_providers
                 WHERE id = ?1 AND current_revision_id IS NOT NULL"
            ),
            [id],
        )
        .map(|mut records| records.pop())
    }
    pub(super) fn records<P: rusqlite::Params>(
        &self,
        query: &str,
        params: P,
    ) -> Result<Vec<ProviderRecord>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "本地数据库锁已损坏。".to_string())?;
        let mut statement = connection
            .prepare(query)
            .map_err(|error| error.to_string())?;
        let records = statement
            .query_map(params, |row| {
                Ok(ProviderRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    base_url: row.get(2)?,
                    secret_ref: row.get(3)?,
                    is_default: row.get(4)?,
                    transport_mode: TransportMode::from_db(row.get(5)?)?,
                    configured: row.get(6)?,
                    current_revision_id: row.get(7)?,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(records)
    }

    pub(super) fn clear_provider_secrets(
        &self,
        provider_id: &str,
        retired_only: bool,
    ) -> Result<(), String> {
        let secret_refs = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "本地数据库锁已损坏。".to_string())?;
            let query = if retired_only {
                "SELECT secret_ref FROM api_key_provider_revisions
                 WHERE provider_id=?1 AND state='retired'"
            } else {
                "SELECT secret_ref FROM api_key_provider_revisions WHERE provider_id=?1"
            };
            let mut statement = connection
                .prepare(query)
                .map_err(|error| error.to_string())?;
            let secret_refs = statement
                .query_map([provider_id], |row| row.get::<_, String>(0))
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            secret_refs
        };
        for secret_ref in secret_refs {
            self.vault.clear(&secret_ref)?;
        }
        Ok(())
    }
}

pub(super) fn provider_operation_resources(record: &ProviderRecord) -> Vec<ResourceRef> {
    vec![
        ResourceRef::revision(
            ResourceKind::ApiKeyProvider,
            &record.id,
            &record.current_revision_id,
        ),
        ResourceRef::logical(
            ResourceKind::ApiKeyProviderRevision,
            &record.current_revision_id,
        ),
        ResourceRef::logical(ResourceKind::Gateway, &record.id),
    ]
}

pub(super) fn revision_secret_ref(provider_id: &str, revision_id: &str) -> String {
    format!("provider:{provider_id}:revision:{revision_id}")
}

pub(super) fn validate_gateway_timeout(
    input: Option<&ProviderGatewayTimeoutInput>,
) -> Result<(), String> {
    let Some(input) = input else {
        return Ok(());
    };
    for (label, value, minimum, maximum) in [
        ("连接", input.connect_seconds, 1, 120),
        ("首字节", input.first_byte_seconds, 1, 600),
        ("流式空闲", input.stream_idle_seconds, 1, 1800),
        ("非流式总计", input.non_stream_total_seconds, 1, 7200),
    ] {
        if !(minimum..=maximum).contains(&value) {
            return Err(format!("{label}超时必须在 {minimum} 到 {maximum} 秒之间。"));
        }
    }
    if input.non_stream_total_seconds < input.first_byte_seconds {
        return Err("非流式总超时不能小于首字节超时。".to_string());
    }
    Ok(())
}

pub(super) fn save_gateway_timeout_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    provider_id: &str,
    input: Option<&ProviderGatewayTimeoutInput>,
) -> Result<(), String> {
    let Some(input) = input else {
        return Ok(());
    };
    transaction
        .execute(
            "INSERT INTO gateway_timeout_settings (
                provider_id, connect_seconds, first_byte_seconds,
                stream_idle_seconds, non_stream_total_seconds, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(provider_id) DO UPDATE SET
                connect_seconds=excluded.connect_seconds,
                first_byte_seconds=excluded.first_byte_seconds,
                stream_idle_seconds=excluded.stream_idle_seconds,
                non_stream_total_seconds=excluded.non_stream_total_seconds,
                updated_at=excluded.updated_at",
            params![
                provider_id,
                input.connect_seconds,
                input.first_byte_seconds,
                input.stream_idle_seconds,
                input.non_stream_total_seconds,
                unix_now_seconds(),
            ],
        )
        .map_err(|error| format!("无法保存 Provider Gateway 设置：{error}"))?;
    Ok(())
}

pub(super) fn wire_api(transport_mode: TransportMode) -> &'static str {
    match transport_mode {
        TransportMode::GatewayChatCompletions => "chat_completions",
        TransportMode::DirectResponses | TransportMode::GatewayPassthrough => "responses",
    }
}

pub(super) fn command_error_string(error: CommandError) -> String {
    serde_json::to_string(&error).unwrap_or_else(|_| error.to_string())
}

pub(super) fn validate_name(value: String) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 80 {
        return Err("Provider 名称不能为空且最多 80 个字符。".to_string());
    }
    Ok(value.to_string())
}
/// Canonical API prefix shared by channel probes and both Gateway sources.
/// A nonempty custom prefix is authoritative; never retry inference elsewhere.
pub(crate) fn normalize_api_base_url(value: &str) -> Result<String, String> {
    let mut url = Url::parse(value.trim()).map_err(|_| "upstream_base_url_invalid".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("upstream_base_url_invalid".to_string());
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(if path.is_empty() { "/v1" } else { &path });
    Ok(url.to_string())
}

pub(crate) fn validate_base_url(value: String) -> Result<String, String> {
    let url =
        Url::parse(value.trim()).map_err(|_| "请输入有效的 Provider Base URL。".to_string())?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err("Provider Base URL 不可包含查询参数或片段。".to_string());
    }
    let loopback = matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("::1")
    );
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("Provider 必须使用 HTTPS；仅回环地址可使用 HTTP。".to_string());
    }
    normalize_api_base_url(url.as_str())
}
