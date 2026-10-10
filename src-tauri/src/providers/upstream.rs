//! 按 Provider 或修订解析 Gateway 上游。
//!
//! 不写渠道元数据；只读当前修订并从 vault 取 Key。
//! Gateway 启动和转发通过这些方法拿到上游。

use rusqlite::OptionalExtension;

#[cfg(test)]
use super::models::PROVIDER_RECORD_COLUMNS;
use super::models::{GatewayUpstream, TransportMode};
use super::normalize_api_base_url;
use super::store::ProviderStore;

impl ProviderStore {
    pub fn load_secret_for_launch(&self, secret_ref: &str) -> Result<String, String> {
        self.vault
            .load(secret_ref)?
            .map(|key| key.expose().to_string())
            .ok_or_else(|| "credential_missing".to_string())
    }

    /// Load the active revision key for the API login in R Codex's own Codex home.
    /// The value is never returned through IPC.
    pub fn load_current_provider_key(
        &self,
        provider_id: &str,
    ) -> Result<crate::secrets::ApiKey, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database_lock_failed".to_string())?;
        let secret_ref: String = connection
            .query_row(
                "SELECT revision.secret_ref
                 FROM api_key_providers provider
                 JOIN api_key_provider_revisions revision
                   ON revision.id=provider.current_revision_id
                 WHERE provider.id=?1",
                [provider_id],
                |row| row.get(0),
            )
            .map_err(|_| "provider_not_found".to_string())?;
        drop(connection);
        self.load_active_revision_key(provider_id, &secret_ref)
    }
    #[cfg(test)]
    pub fn default_upstream(&self) -> Result<GatewayUpstream, String> {
        let record = self
            .records(
                &format!(
                    "SELECT {PROVIDER_RECORD_COLUMNS} FROM api_key_providers
                     WHERE is_default = 1 AND current_revision_id IS NOT NULL"
                ),
                [],
            )?
            .pop()
            .ok_or_else(|| "尚未配置默认 API-key Provider。".to_string())?;
        let api_key = self.api_key_for_record(&record)?;
        Ok(GatewayUpstream {
            provider_id: record.id,
            provider_revision_id: record.current_revision_id,
            base_url: normalize_api_base_url(&record.base_url)?,
            api_key,
            provider_name: record.name,
            transport_mode: record.transport_mode,
        })
    }

    pub fn upstream_for_provider(&self, id: &str) -> Result<GatewayUpstream, String> {
        let record = self
            .record_by_id(id)?
            .ok_or_else(|| "找不到指定 Provider。".to_string())?;
        let api_key = self.api_key_for_record(&record)?;
        Ok(GatewayUpstream {
            provider_id: record.id,
            provider_revision_id: record.current_revision_id,
            base_url: normalize_api_base_url(&record.base_url)?,
            api_key,
            provider_name: record.name,
            transport_mode: record.transport_mode,
        })
    }

    pub fn upstream_for_revision(&self, revision_id: &str) -> Result<GatewayUpstream, String> {
        let revision = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "本地数据库锁已损坏。".to_string())?;
            connection
                .query_row(
                    "SELECT provider.id, provider.name, revision.id,
                            revision.base_url, revision.secret_ref,
                            revision.transport_mode, revision.state
                     FROM api_key_provider_revisions revision
                     JOIN api_key_providers provider ON provider.id=revision.provider_id
                     WHERE revision.id=?1",
                    [revision_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            TransportMode::from_db(row.get(5)?)?,
                            row.get::<_, String>(6)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| format!("无法读取 Provider 修订：{error}"))?
        }
        .ok_or_else(|| "provider_revision_not_found".to_string())?;
        if revision.6 != "active" {
            return Err("provider_revision_not_active".to_string());
        }
        let api_key = self.load_active_revision_key(&revision.0, &revision.4)?;
        Ok(GatewayUpstream {
            provider_id: revision.0,
            provider_revision_id: revision.2,
            base_url: normalize_api_base_url(&revision.3)?,
            api_key,
            provider_name: revision.1,
            transport_mode: revision.5,
        })
    }
}
