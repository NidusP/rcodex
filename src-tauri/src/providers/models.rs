//! API Key Provider 的公开结构和内部记录。
//!
//! 只放序列化类型、传输模式和 SQLite 行映射。行为方法在 store / upstream / operations。
//! 由 ProviderStore 和 Gateway 上游解析使用。

use serde::{Deserialize, Serialize};

use crate::secrets::ApiKey;

#[cfg(test)]
pub(super) const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub(super) const PROVIDER_RECORD_COLUMNS: &str =
    "id, name, base_url, secret_ref, is_default, transport_mode, configured, current_revision_id";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransportMode {
    DirectResponses,
    GatewayPassthrough,
    GatewayChatCompletions,
}

impl Default for TransportMode {
    fn default() -> Self {
        Self::GatewayPassthrough
    }
}

impl TransportMode {
    pub(crate) fn from_db(value: String) -> rusqlite::Result<Self> {
        match value.as_str() {
            "direct_responses" => Ok(Self::DirectResponses),
            "gateway_passthrough" => Ok(Self::GatewayPassthrough),
            "gateway_chat_completions" => Ok(Self::GatewayChatCompletions),
            _ => Err(rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                format!("未知 Provider 传输模式：{value}").into(),
            )),
        }
    }

    pub(super) fn as_db(self) -> &'static str {
        match self {
            Self::DirectResponses => "direct_responses",
            Self::GatewayPassthrough => "gateway_passthrough",
            Self::GatewayChatCompletions => "gateway_chat_completions",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub is_default: bool,
    pub configured: bool,
    pub transport_mode: TransportMode,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProviderInput {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub transport_mode: TransportMode,
    #[serde(default)]
    pub gateway_timeout_settings: Option<ProviderGatewayTimeoutInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProviderInput {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub transport_mode: TransportMode,
    #[serde(default)]
    pub gateway_timeout_settings: Option<ProviderGatewayTimeoutInput>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderGatewayTimeoutInput {
    pub connect_seconds: u64,
    pub first_byte_seconds: u64,
    pub stream_idle_seconds: u64,
    pub non_stream_total_seconds: u64,
}

pub struct GatewayUpstream {
    pub provider_id: String,
    pub provider_revision_id: String,
    pub base_url: String,
    pub api_key: ApiKey,
    pub provider_name: String,
    pub transport_mode: TransportMode,
}

pub(super) struct ProviderRecord {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) base_url: String,
    pub(super) secret_ref: String,
    pub(super) is_default: bool,
    pub(super) transport_mode: TransportMode,
    pub(super) configured: bool,
    pub(super) current_revision_id: String,
}
