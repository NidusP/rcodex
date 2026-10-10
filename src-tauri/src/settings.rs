use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::database::Database;

const DEFAULT_CONNECT_SECONDS: u64 = 10;
const DEFAULT_FIRST_BYTE_SECONDS: u64 = 30;
const DEFAULT_STREAM_IDLE_SECONDS: u64 = 45;
const DEFAULT_NON_STREAM_TOTAL_SECONDS: u64 = 120;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayTimeoutSettings {
    pub provider_id: String,
    pub connect_seconds: u64,
    pub first_byte_seconds: u64,
    pub stream_idle_seconds: u64,
    pub non_stream_total_seconds: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGatewayTimeoutInput {
    pub connect_seconds: u64,
    pub first_byte_seconds: u64,
    pub stream_idle_seconds: u64,
    pub non_stream_total_seconds: u64,
}

#[derive(Clone)]
pub struct SettingsStore {
    connection: Arc<Mutex<Connection>>,
}

impl SettingsStore {
    pub fn from_database(database: Database) -> Self {
        Self {
            connection: database.connection(),
        }
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::from_database(Database::in_memory())
    }

    pub fn get_gateway_timeout(&self, provider_id: &str) -> Result<GatewayTimeoutSettings, String> {
        let values = self
            .connection
            .lock()
            .map_err(|_| "设置数据库锁已损坏。".to_string())?
            .query_row(
                "SELECT connect_seconds, first_byte_seconds, stream_idle_seconds, non_stream_total_seconds FROM gateway_timeout_settings WHERE provider_id = ?1",
                [provider_id],
                |row| {
                    Ok((
                        row.get::<_, u64>(0)?,
                        row.get::<_, u64>(1)?,
                        row.get::<_, u64>(2)?,
                        row.get::<_, u64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("无法读取 Gateway 超时策略：{error}"))?
            .unwrap_or((
                DEFAULT_CONNECT_SECONDS,
                DEFAULT_FIRST_BYTE_SECONDS,
                DEFAULT_STREAM_IDLE_SECONDS,
                DEFAULT_NON_STREAM_TOTAL_SECONDS,
            ));
        Ok(GatewayTimeoutSettings {
            provider_id: provider_id.to_string(),
            connect_seconds: values.0,
            first_byte_seconds: values.1,
            stream_idle_seconds: values.2,
            non_stream_total_seconds: values.3,
        })
    }

    pub fn save_gateway_timeout(
        &self,
        provider_id: &str,
        input: UpdateGatewayTimeoutInput,
    ) -> Result<GatewayTimeoutSettings, String> {
        validate_seconds("连接", input.connect_seconds, 1, 120)?;
        validate_seconds("首字节", input.first_byte_seconds, 1, 600)?;
        validate_seconds("流式空闲", input.stream_idle_seconds, 1, 1800)?;
        validate_seconds("非流式总计", input.non_stream_total_seconds, 1, 7200)?;
        if input.non_stream_total_seconds < input.first_byte_seconds {
            return Err("非流式总超时不能小于首字节超时。".to_string());
        }
        self.connection
            .lock()
            .map_err(|_| "设置数据库锁已损坏。".to_string())?
            .execute(
                "INSERT INTO gateway_timeout_settings (provider_id, connect_seconds, first_byte_seconds, stream_idle_seconds, non_stream_total_seconds, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s','now')) ON CONFLICT(provider_id) DO UPDATE SET connect_seconds=excluded.connect_seconds, first_byte_seconds=excluded.first_byte_seconds, stream_idle_seconds=excluded.stream_idle_seconds, non_stream_total_seconds=excluded.non_stream_total_seconds, updated_at=excluded.updated_at",
                params![provider_id, input.connect_seconds, input.first_byte_seconds, input.stream_idle_seconds, input.non_stream_total_seconds],
            )
            .map_err(|error| format!("无法保存 Gateway 超时策略：{error}"))?;
        self.get_gateway_timeout(provider_id)
    }
}

fn validate_seconds(label: &str, value: u64, min: u64, max: u64) -> Result<(), String> {
    if !(min..=max).contains(&value) {
        return Err(format!("{label}超时必须在 {min} 到 {max} 秒之间。"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_stable_and_can_be_updated_per_provider() {
        let store = SettingsStore::in_memory();
        let defaults = store.get_gateway_timeout("provider-a").unwrap();
        assert_eq!(defaults.connect_seconds, DEFAULT_CONNECT_SECONDS);
        let saved = store
            .save_gateway_timeout(
                "provider-a",
                UpdateGatewayTimeoutInput {
                    connect_seconds: 5,
                    first_byte_seconds: 20,
                    stream_idle_seconds: 60,
                    non_stream_total_seconds: 180,
                },
            )
            .unwrap();
        assert_eq!(saved.connect_seconds, 5);
    }

    #[test]
    fn invalid_timeout_relationship_is_rejected() {
        let store = SettingsStore::in_memory();
        let error = store
            .save_gateway_timeout(
                "provider-a",
                UpdateGatewayTimeoutInput {
                    connect_seconds: 5,
                    first_byte_seconds: 30,
                    stream_idle_seconds: 60,
                    non_stream_total_seconds: 20,
                },
            )
            .unwrap_err();
        assert!(error.contains("不能小于"));
    }
}
