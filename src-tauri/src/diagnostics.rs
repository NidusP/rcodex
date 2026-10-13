use std::sync::{Arc, Mutex};

use crate::{database::Database, providers::TransportMode};
use chrono::Local;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_RECENT_EVENTS: i64 = 20;

#[derive(Clone)]
pub struct DiagnosticsStore {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestDiagnosticOutcome {
    Success,
    Timeout,
    Interrupted,
    FormatError,
    Failed,
}

impl RequestDiagnosticOutcome {
    fn as_db(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Timeout => "timeout",
            Self::Interrupted => "interrupted",
            Self::FormatError => "format_error",
            Self::Failed => "failed",
        }
    }
    fn from_db(value: &str) -> rusqlite::Result<Self> {
        match value {
            "success" => Ok(Self::Success),
            "timeout" => Ok(Self::Timeout),
            "interrupted" => Ok(Self::Interrupted),
            "format_error" => Ok(Self::FormatError),
            "failed" => Ok(Self::Failed),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Clone)]
pub struct RequestDiagnosticInput {
    pub instance_id: Option<String>,
    pub session_id: Option<String>,
    pub route: String,
    pub stream: bool,
    pub wire_api: &'static str,
    pub public_model_id: String,
    pub upstream_model_id: String,
    pub provider_id: String,
    pub provider_revision_id: String,
    pub http_status: Option<u16>,
    pub response_content_type: Option<String>,
    pub first_byte_ms: Option<i64>,
    pub total_latency_ms: Option<i64>,
    pub outcome: RequestDiagnosticOutcome,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub input_length: usize,
    pub output_length: usize,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub input_hash: Option<String>,
    pub output_hash: Option<String>,
    pub tool_call_types: Vec<String>,
    pub tool_result_hashes: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RequestDiagnosticEvent {
    pub id: i64,
    pub instance_id: Option<String>,
    pub session_id: Option<String>,
    pub created_at: i64,
    pub route: String,
    pub stream: bool,
    pub wire_api: String,
    pub public_model_id: String,
    pub upstream_model_id: String,
    pub provider_id: String,
    pub provider_revision_id: String,
    pub http_status: Option<u16>,
    pub response_content_type: Option<String>,
    pub first_byte_ms: Option<i64>,
    pub total_latency_ms: Option<i64>,
    pub outcome: RequestDiagnosticOutcome,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub input_length: usize,
    pub output_length: usize,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub input_hash: Option<String>,
    pub output_hash: Option<String>,
    pub tool_call_types: Vec<String>,
    pub tool_result_hashes: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummaryEvent {
    pub id: i64,
    pub instance_id: String,
    pub session_id: Option<String>,
    pub created_at: i64,
    pub input_length: usize,
    pub output_length: usize,
    pub input_hash: Option<String>,
    pub output_hash: Option<String>,
    pub tool_call_types: Vec<String>,
    pub tool_result_hashes: Vec<String>,
}

#[derive(Clone, Copy)]
pub enum DiagnosticSource {
    Probe,
}

impl DiagnosticSource {
    fn as_db(self) -> &'static str {
        match self {
            Self::Probe => "probe",
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticOutcome {
    Success,
    Failed,
}

impl DiagnosticOutcome {
    fn as_db(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone)]
pub struct DiagnosticEventInput<'a> {
    pub provider_id: &'a str,
    pub source: DiagnosticSource,
    pub route: &'a str,
    pub model: Option<&'a str>,
    pub transport_mode: TransportMode,
    pub stream: bool,
    pub outcome: DiagnosticOutcome,
    pub http_status: Option<u16>,
    pub latency_ms: Option<i64>,
    pub error_code: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayDiagnosticEvent {
    pub id: i64,
    pub provider_id: String,
    pub created_at: i64,
    pub source: String,
    pub route: String,
    pub model: Option<String>,
    pub transport_mode: TransportMode,
    pub stream: bool,
    pub outcome: DiagnosticOutcome,
    pub http_status: Option<u16>,
    pub latency_ms: Option<i64>,
    pub error_code: Option<String>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectivityStatus {
    Ready,
    Degraded,
    Unreachable,
    Unauthorized,
}

impl ConnectivityStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Unreachable => "unreachable",
            Self::Unauthorized => "unauthorized",
        }
    }

    fn from_db(value: String) -> rusqlite::Result<Self> {
        match value.as_str() {
            "ready" => Ok(Self::Ready),
            "degraded" => Ok(Self::Degraded),
            "unreachable" => Ok(Self::Unreachable),
            "unauthorized" => Ok(Self::Unauthorized),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ready,
    Failed,
    NotRun,
}

impl CheckStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::NotRun => "not_run",
        }
    }

    fn from_db(value: String) -> rusqlite::Result<Self> {
        match value.as_str() {
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            "not_run" => Ok(Self::NotRun),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnectivityReport {
    pub provider_id: String,
    pub checked_at: i64,
    pub status: ConnectivityStatus,
    pub model: Option<String>,
    pub models: Vec<ProviderModelOption>,
    pub models_check: CheckStatus,
    pub text_check: CheckStatus,
    pub error_code: Option<String>,
    pub wire_api: Option<String>,
    pub catalog_revision_id: Option<String>,
}

#[derive(Clone)]
pub struct ProbeSnapshot {
    pub status: ConnectivityStatus,
    pub wire_api: Option<String>,
    /// Prefix that answered the text probe. Forwarding stores this as base_url.
    pub confirmed_base_url: Option<String>,
    pub model: Option<String>,
    pub models: Vec<ProviderModelOption>,
    pub models_check: CheckStatus,
    pub text_check: CheckStatus,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelOption {
    pub upstream_model_id: String,
    pub display_name: String,
    pub owned_by: Option<String>,
}

impl DiagnosticsStore {
    pub fn from_database(database: Database) -> Self {
        Self {
            connection: database.connection(),
        }
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::from_database(Database::in_memory())
    }

    pub fn record(&self, event: DiagnosticEventInput<'_>) -> Result<(), String> {
        self.connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?
            .execute(
                "INSERT INTO gateway_diagnostic_events (provider_id, created_at, source, route, model, transport_mode, stream, outcome, http_status, latency_ms, error_code) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![event.provider_id, now(), event.source.as_db(), event.route, event.model, transport_mode_db(event.transport_mode), event.stream, event.outcome.as_db(), event.http_status, event.latency_ms, event.error_code],
            )
            .map_err(|error| format!("无法记录 Gateway 诊断：{error}"))?;
        Ok(())
    }

    pub fn record_request(&self, event: RequestDiagnosticInput) -> Result<(), String> {
        let tool_call_types =
            serde_json::to_string(&event.tool_call_types).unwrap_or_else(|_| "[]".into());
        let tool_result_hashes =
            serde_json::to_string(&event.tool_result_hashes).unwrap_or_else(|_| "[]".into());
        let instance_id = event.instance_id.clone();
        let session_id = event.session_id.clone();
        let connection = self
            .connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?;
        connection.execute(
            "INSERT INTO gateway_request_diagnostics (instance_id, session_id, created_at, route, stream, wire_api, public_model_id, upstream_model_id, provider_id, provider_revision_id, http_status, response_content_type, first_byte_ms, total_latency_ms, outcome, error_code, error_detail, input_length, output_length, input_tokens, output_tokens, total_tokens, input_hash, output_hash, tool_call_types, tool_result_hashes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
            params![event.instance_id, event.session_id, now(), event.route, event.stream, event.wire_api, event.public_model_id, event.upstream_model_id, event.provider_id, event.provider_revision_id, event.http_status, event.response_content_type, event.first_byte_ms, event.total_latency_ms, event.outcome.as_db(), event.error_code, event.error_detail, event.input_length as i64, event.output_length as i64, event.input_tokens, event.output_tokens, event.total_tokens, event.input_hash, event.output_hash, tool_call_types, tool_result_hashes],
        ).map_err(|error| format!("无法记录请求级 Gateway 诊断：{error}"))?;
        if let Some(instance_id) = instance_id {
            connection.execute(
                "INSERT INTO conversation_summaries (instance_id, session_id, created_at, input_length, output_length, input_hash, output_hash, tool_call_types, tool_result_hashes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![instance_id, session_id, now(), event.input_length as i64, event.output_length as i64, event.input_hash, event.output_hash, tool_call_types, tool_result_hashes],
            ).map_err(|error| format!("无法记录对话摘要：{error}"))?;
        }
        Ok(())
    }

    pub fn request_recent(&self, instance_id: &str) -> Result<Vec<RequestDiagnosticEvent>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?;
        let mut statement = connection.prepare("SELECT id, instance_id, session_id, created_at, route, stream, wire_api, public_model_id, upstream_model_id, provider_id, provider_revision_id, http_status, response_content_type, first_byte_ms, total_latency_ms, outcome, error_code, error_detail, input_length, output_length, input_tokens, output_tokens, total_tokens, input_hash, output_hash, tool_call_types, tool_result_hashes FROM gateway_request_diagnostics WHERE instance_id=?1 ORDER BY created_at DESC, id DESC LIMIT 100").map_err(|error| format!("无法读取请求级诊断：{error}"))?;
        let rows = statement
            .query_map([instance_id], |row| {
                let parse_vec = |index: usize| -> rusqlite::Result<Vec<String>> {
                    let raw = row.get::<_, String>(index)?;
                    Ok(serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default())
                };
                Ok(RequestDiagnosticEvent {
                    id: row.get(0)?,
                    instance_id: row.get(1)?,
                    session_id: row.get(2)?,
                    created_at: row.get(3)?,
                    route: row.get(4)?,
                    stream: row.get(5)?,
                    wire_api: row.get(6)?,
                    public_model_id: row.get(7)?,
                    upstream_model_id: row.get(8)?,
                    provider_id: row.get(9)?,
                    provider_revision_id: row.get(10)?,
                    http_status: row.get(11)?,
                    response_content_type: row.get(12)?,
                    first_byte_ms: row.get(13)?,
                    total_latency_ms: row.get(14)?,
                    outcome: RequestDiagnosticOutcome::from_db(&row.get::<_, String>(15)?)?,
                    error_code: row.get(16)?,
                    error_detail: row.get(17)?,
                    input_length: row.get::<_, i64>(18)? as usize,
                    output_length: row.get::<_, i64>(19)? as usize,
                    input_tokens: row.get(20)?,
                    output_tokens: row.get(21)?,
                    total_tokens: row.get(22)?,
                    input_hash: row.get(23)?,
                    output_hash: row.get(24)?,
                    tool_call_types: parse_vec(25)?,
                    tool_result_hashes: parse_vec(26)?,
                })
            })
            .map_err(|error| format!("无法读取请求级诊断：{error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法解析请求级诊断：{error}"))
    }

    pub fn recent_requests(&self) -> Result<Vec<RequestDiagnosticEvent>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?;
        let mut statement = connection
            .prepare("SELECT id, instance_id, session_id, created_at, route, stream, wire_api, public_model_id, upstream_model_id, provider_id, provider_revision_id, http_status, response_content_type, first_byte_ms, total_latency_ms, outcome, error_code, error_detail, input_length, output_length, input_tokens, output_tokens, total_tokens, input_hash, output_hash, tool_call_types, tool_result_hashes FROM gateway_request_diagnostics ORDER BY created_at DESC, id DESC LIMIT 100")
            .map_err(|error| format!("无法读取请求级诊断：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                let parse_vec = |index: usize| -> rusqlite::Result<Vec<String>> {
                    let raw = row.get::<_, String>(index)?;
                    Ok(serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default())
                };
                Ok(RequestDiagnosticEvent {
                    id: row.get(0)?,
                    instance_id: row.get(1)?,
                    session_id: row.get(2)?,
                    created_at: row.get(3)?,
                    route: row.get(4)?,
                    stream: row.get(5)?,
                    wire_api: row.get(6)?,
                    public_model_id: row.get(7)?,
                    upstream_model_id: row.get(8)?,
                    provider_id: row.get(9)?,
                    provider_revision_id: row.get(10)?,
                    http_status: row.get(11)?,
                    response_content_type: row.get(12)?,
                    first_byte_ms: row.get(13)?,
                    total_latency_ms: row.get(14)?,
                    outcome: RequestDiagnosticOutcome::from_db(&row.get::<_, String>(15)?)?,
                    error_code: row.get(16)?,
                    error_detail: row.get(17)?,
                    input_length: row.get::<_, i64>(18)? as usize,
                    output_length: row.get::<_, i64>(19)? as usize,
                    input_tokens: row.get(20)?,
                    output_tokens: row.get(21)?,
                    total_tokens: row.get(22)?,
                    input_hash: row.get(23)?,
                    output_hash: row.get(24)?,
                    tool_call_types: parse_vec(25)?,
                    tool_result_hashes: parse_vec(26)?,
                })
            })
            .map_err(|error| format!("无法读取请求级诊断：{error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法解析请求级诊断：{error}"))
    }

    pub fn conversation_recent(
        &self,
        instance_id: &str,
    ) -> Result<Vec<ConversationSummaryEvent>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?;
        let mut statement = connection.prepare("SELECT id, instance_id, session_id, created_at, input_length, output_length, input_hash, output_hash, tool_call_types, tool_result_hashes FROM conversation_summaries WHERE instance_id=?1 ORDER BY created_at DESC, id DESC LIMIT 100").map_err(|error| format!("无法读取对话摘要：{error}"))?;
        let rows = statement
            .query_map([instance_id], |row| {
                let parse_vec = |index: usize| -> rusqlite::Result<Vec<String>> {
                    let raw = row.get::<_, String>(index)?;
                    Ok(serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default())
                };
                Ok(ConversationSummaryEvent {
                    id: row.get(0)?,
                    instance_id: row.get(1)?,
                    session_id: row.get(2)?,
                    created_at: row.get(3)?,
                    input_length: row.get::<_, i64>(4)? as usize,
                    output_length: row.get::<_, i64>(5)? as usize,
                    input_hash: row.get(6)?,
                    output_hash: row.get(7)?,
                    tool_call_types: parse_vec(8)?,
                    tool_result_hashes: parse_vec(9)?,
                })
            })
            .map_err(|error| format!("无法读取对话摘要：{error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法解析对话摘要：{error}"))
    }

    pub fn clear_instance(&self, instance_id: &str) -> Result<u64, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("无法开始清除实例诊断事务：{error}"))?;
        let deleted = transaction
            .execute(
                "DELETE FROM gateway_request_diagnostics WHERE instance_id=?1",
                [instance_id],
            )
            .map_err(|error| format!("无法清除实例诊断：{error}"))?;
        let summaries = transaction
            .execute(
                "DELETE FROM conversation_summaries WHERE instance_id=?1",
                [instance_id],
            )
            .map_err(|error| format!("无法清除对话摘要：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("无法提交实例诊断清除：{error}"))?;
        Ok((deleted + summaries) as u64)
    }

    pub fn save_connectivity_report(
        &self,
        report: &ProviderConnectivityReport,
    ) -> Result<(), String> {
        let wire_api = match report.wire_api.as_deref() {
            None => None,
            Some("responses" | "chat_completions") => report.wire_api.clone(),
            Some(_) => return Err("探针协议无效。".to_string()),
        };
        self.connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?
            .execute(
                "INSERT INTO provider_connectivity_reports (provider_id, checked_at, status, model, models_check, text_check, error_code, wire_api, catalog_revision_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(provider_id) DO UPDATE SET checked_at=excluded.checked_at, status=excluded.status, model=excluded.model, models_check=excluded.models_check, text_check=excluded.text_check, error_code=excluded.error_code, wire_api=excluded.wire_api, catalog_revision_id=excluded.catalog_revision_id",
                params![report.provider_id, report.checked_at, report.status.as_db(), report.model, report.models_check.as_db(), report.text_check.as_db(), report.error_code, wire_api, report.catalog_revision_id],
            )
            .map_err(|error| format!("无法保存 Provider 检测结果：{error}"))?;
        Ok(())
    }

    pub fn save_model_catalog(
        &self,
        provider_id: &str,
        provider_revision_id: &str,
        models: &[ProviderModelOption],
    ) -> Result<String, String> {
        let payload = serde_json::to_vec(models)
            .map_err(|error| format!("无法编码 Provider 模型目录：{error}"))?;
        let digest = Sha256::digest(&payload);
        let payload_sha256 = format!(
            "sha256:{}",
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let catalog_id = Uuid::new_v4().to_string();
        let discovered_at = now();
        let stale_after = discovered_at.saturating_add(7 * 24 * 60 * 60);
        let connection = self
            .connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("无法开始模型目录事务：{error}"))?;
        transaction
            .execute(
                "INSERT INTO provider_model_catalog_revisions
                 (id, provider_id, provider_revision_id, normalized_by, payload_sha256,
                  state, discovered_at, stale_after, error_code)
                 VALUES (?1, ?2, ?3, 'openai_models_v1', ?4, 'ready', ?5, ?6, NULL)",
                params![
                    catalog_id,
                    provider_id,
                    provider_revision_id,
                    payload_sha256,
                    discovered_at,
                    stale_after,
                ],
            )
            .map_err(|error| format!("无法保存 Provider 模型目录：{error}"))?;
        for (position, model) in models.iter().enumerate() {
            let capabilities = json!({
                "discovery": "openai_models",
                "ownedBy": model.owned_by,
            })
            .to_string();
            transaction
                .execute(
                    "INSERT INTO provider_model_catalog_entries
                     (catalog_revision_id, upstream_model_id, display_name,
                      capabilities_json, position)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        catalog_id,
                        model.upstream_model_id,
                        model.display_name,
                        capabilities,
                        position as i64,
                    ],
                )
                .map_err(|error| format!("无法保存 Provider 模型目录项：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("无法提交 Provider 模型目录：{error}"))?;
        Ok(catalog_id)
    }

    pub fn save_failed_probe_catalog(
        &self,
        provider_id: &str,
        provider_revision_id: &str,
        error_code: &str,
    ) -> Result<String, String> {
        let catalog_id = Uuid::new_v4().to_string();
        let discovered_at = now();
        let connection = self
            .connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?;
        connection
            .execute(
                "INSERT INTO provider_model_catalog_revisions
                 (id, provider_id, provider_revision_id, normalized_by, payload_sha256,
                  state, discovered_at, stale_after, error_code)
                 VALUES (?1, ?2, ?3, 'openai_models_v1', 'sha256:failed', 'failed', ?4, NULL, ?5)",
                params![
                    catalog_id,
                    provider_id,
                    provider_revision_id,
                    discovered_at,
                    error_code,
                ],
            )
            .map_err(|error| format!("无法保存失败的探针目录：{error}"))?;
        Ok(catalog_id)
    }

    pub fn store_channel_probe(
        &self,
        provider_id: &str,
        provider_revision_id: &str,
        snapshot: &ProbeSnapshot,
    ) -> Result<ProviderConnectivityReport, String> {
        let models_ready = matches!(snapshot.models_check, CheckStatus::Ready);
        let catalog_revision_id = if models_ready {
            self.save_model_catalog(provider_id, provider_revision_id, &snapshot.models)?
        } else {
            self.save_failed_probe_catalog(
                provider_id,
                provider_revision_id,
                snapshot.error_code.as_deref().unwrap_or("probe_incomplete"),
            )?
        };
        let report = ProviderConnectivityReport {
            provider_id: provider_id.to_string(),
            checked_at: now(),
            status: snapshot.status,
            model: snapshot.model.clone(),
            models: if models_ready {
                snapshot.models.clone()
            } else {
                Vec::new()
            },
            models_check: snapshot.models_check,
            text_check: snapshot.text_check,
            error_code: snapshot.error_code.clone(),
            wire_api: snapshot.wire_api.clone(),
            catalog_revision_id: Some(catalog_revision_id),
        };
        self.save_connectivity_report(&report)?;
        Ok(report)
    }

    pub fn connectivity_report(
        &self,
        provider_id: &str,
    ) -> Result<Option<ProviderConnectivityReport>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?;
        let report = connection
            .query_row(
                "SELECT provider_id, checked_at, status, model, models_check, text_check, error_code, wire_api, catalog_revision_id
                 FROM provider_connectivity_reports WHERE provider_id = ?1",
                [provider_id],
                |row| {
                    Ok(ProviderConnectivityReport {
                        provider_id: row.get(0)?,
                        checked_at: row.get(1)?,
                        status: ConnectivityStatus::from_db(row.get(2)?)?,
                        model: row.get(3)?,
                        models: Vec::new(),
                        models_check: CheckStatus::from_db(row.get(4)?)?,
                        text_check: CheckStatus::from_db(row.get(5)?)?,
                        error_code: row.get(6)?,
                        wire_api: row.get(7)?,
                        catalog_revision_id: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("无法读取 Provider 检测结果：{error}"))?;
        let Some(mut report) = report else {
            return Ok(None);
        };
        let sql = if report.catalog_revision_id.is_some() {
            "SELECT entry.upstream_model_id, entry.display_name, entry.capabilities_json
             FROM provider_model_catalog_entries entry
             WHERE entry.catalog_revision_id=?1
             ORDER BY entry.position ASC"
        } else {
            "SELECT entry.upstream_model_id, entry.display_name, entry.capabilities_json
             FROM provider_model_catalog_entries entry
             JOIN provider_model_catalog_revisions catalog
               ON catalog.id=entry.catalog_revision_id
             JOIN api_key_providers provider
               ON provider.id=catalog.provider_id
              AND provider.current_revision_id=catalog.provider_revision_id
             WHERE catalog.provider_id=?1 AND catalog.state='ready'
               AND catalog.id = (
                 SELECT latest.id
                 FROM provider_model_catalog_revisions latest
                 WHERE latest.provider_id=?1
                   AND latest.provider_revision_id=provider.current_revision_id
                   AND latest.state='ready'
                 ORDER BY latest.discovered_at DESC, latest.rowid DESC
                 LIMIT 1
               )
             ORDER BY entry.position ASC"
        };
        let catalog_key = report
            .catalog_revision_id
            .clone()
            .unwrap_or_else(|| provider_id.to_string());
        let mut statement = connection
            .prepare(sql)
            .map_err(|error| format!("无法读取 Provider 模型目录：{error}"))?;
        let rows = statement
            .query_map([catalog_key], |row| {
                let capabilities = row.get::<_, String>(2)?;
                let owned_by = serde_json::from_str::<serde_json::Value>(&capabilities)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("ownedBy")
                            .and_then(|item| item.as_str())
                            .map(str::to_owned)
                    });
                Ok(ProviderModelOption {
                    upstream_model_id: row.get(0)?,
                    display_name: row.get(1)?,
                    owned_by,
                })
            })
            .map_err(|error| format!("无法读取 Provider 模型目录：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法解析 Provider 模型目录：{error}"))?;
        report.models = rows;
        Ok(Some(report))
    }

    pub fn recent(&self, provider_id: Option<&str>) -> Result<Vec<GatewayDiagnosticEvent>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?;
        let mut statement = connection
            .prepare("SELECT id, provider_id, created_at, source, route, model, transport_mode, stream, outcome, http_status, latency_ms, error_code FROM gateway_diagnostic_events WHERE (?1 IS NULL OR provider_id = ?1) ORDER BY created_at DESC, id DESC LIMIT ?2")
            .map_err(|error| format!("无法读取 Gateway 诊断：{error}"))?;
        let result = statement
            .query_map(params![provider_id, MAX_RECENT_EVENTS], |row| {
                let transport_mode = match row.get::<_, String>(6)?.as_str() {
                    "direct_responses" | "gateway_passthrough" => TransportMode::GatewayPassthrough,
                    "gateway_chat_completions" => TransportMode::GatewayChatCompletions,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                let outcome = match row.get::<_, String>(8)?.as_str() {
                    "success" => DiagnosticOutcome::Success,
                    "failed" => DiagnosticOutcome::Failed,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                Ok(GatewayDiagnosticEvent {
                    id: row.get(0)?,
                    provider_id: row.get(1)?,
                    created_at: row.get(2)?,
                    source: row.get(3)?,
                    route: row.get(4)?,
                    model: row.get(5)?,
                    transport_mode,
                    stream: row.get(7)?,
                    outcome,
                    http_status: row.get(9)?,
                    latency_ms: row.get(10)?,
                    error_code: row.get(11)?,
                })
            })
            .map_err(|error| format!("无法读取 Gateway 诊断：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法读取 Gateway 诊断：{error}"));
        result
    }

    pub fn clear_all(&self) -> Result<u64, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "诊断数据库锁已损坏。".to_string())?;
        let deleted_events = connection
            .execute("DELETE FROM gateway_diagnostic_events", [])
            .map_err(|error| format!("无法清空 Gateway 诊断记录：{error}"))?;
        let deleted_requests = connection
            .execute("DELETE FROM gateway_request_diagnostics", [])
            .map_err(|error| format!("无法清空请求级诊断记录：{error}"))?;
        Ok((deleted_events + deleted_requests) as u64)
    }
}

fn transport_mode_db(mode: TransportMode) -> &'static str {
    match mode {
        TransportMode::GatewayPassthrough => "gateway_passthrough",
        TransportMode::GatewayChatCompletions => "gateway_chat_completions",
    }
}

pub fn now() -> i64 {
    Local::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_provider_scoped_and_recent_events_are_limited() {
        let store = DiagnosticsStore::in_memory();
        for index in 0..22 {
            store
                .record(DiagnosticEventInput {
                    provider_id: if index % 2 == 0 { "a" } else { "b" },
                    source: DiagnosticSource::Probe,
                    route: "/v1/responses",
                    model: None,
                    transport_mode: TransportMode::GatewayPassthrough,
                    stream: false,
                    outcome: DiagnosticOutcome::Success,
                    http_status: Some(200),
                    latency_ms: Some(2),
                    error_code: None,
                })
                .unwrap();
        }
        let report = ProviderConnectivityReport {
            provider_id: "a".into(),
            checked_at: now(),
            status: ConnectivityStatus::Ready,
            model: Some("m".into()),
            models: Vec::new(),
            models_check: CheckStatus::Ready,
            text_check: CheckStatus::Ready,
            error_code: None,
            wire_api: None,
            catalog_revision_id: None,
        };
        store.save_connectivity_report(&report).unwrap();
        assert_eq!(
            store
                .connectivity_report("a")
                .unwrap()
                .unwrap()
                .model
                .as_deref(),
            Some("m")
        );
        assert_eq!(store.recent(None).unwrap().len(), 20);
        assert_eq!(store.recent(Some("a")).unwrap().len(), 11);
    }

    #[test]
    fn clearing_events_does_not_remove_connectivity_reports() {
        let store = DiagnosticsStore::in_memory();
        store
            .record(DiagnosticEventInput {
                provider_id: "provider-a",
                source: DiagnosticSource::Probe,
                route: "/v1/models",
                model: None,
                transport_mode: TransportMode::GatewayPassthrough,
                stream: false,
                outcome: DiagnosticOutcome::Failed,
                http_status: Some(401),
                latency_ms: Some(12),
                error_code: Some("upstream_unauthorized"),
            })
            .unwrap();
        store
            .save_connectivity_report(&ProviderConnectivityReport {
                provider_id: "provider-a".to_string(),
                checked_at: now(),
                status: ConnectivityStatus::Unauthorized,
                model: None,
                models: Vec::new(),
                models_check: CheckStatus::Failed,
                text_check: CheckStatus::NotRun,
                error_code: Some("upstream_unauthorized".to_string()),
                wire_api: None,
                catalog_revision_id: None,
            })
            .unwrap();

        assert_eq!(store.clear_all().unwrap(), 1);
        assert!(store.recent(None).unwrap().is_empty());
        assert_eq!(
            store
                .connectivity_report("provider-a")
                .unwrap()
                .unwrap()
                .error_code
                .as_deref(),
            Some("upstream_unauthorized")
        );
    }

    #[test]
    fn model_catalog_reads_only_the_latest_current_revision_snapshot() {
        let store = DiagnosticsStore::in_memory();
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO api_key_providers (id, name, base_url, secret_ref, is_default, transport_mode, configured, created_at, current_revision_id, updated_at)
                 VALUES ('provider-a', 'Provider A', 'https://example.com/v1', 'provider-a-secret', 1, 'gateway_passthrough', 1, 1, NULL, 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO api_key_provider_revisions (id, provider_id, secret_ref, base_url, wire_api, transport_mode, state, created_at, retired_at)
                 VALUES ('revision-a', 'provider-a', 'provider-a-secret', 'https://example.com/v1', 'responses', 'gateway_passthrough', 'active', 1, NULL)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE api_key_providers SET current_revision_id='revision-a' WHERE id='provider-a'",
                [],
            )
            .unwrap();
        drop(connection);

        store
            .save_model_catalog(
                "provider-a",
                "revision-a",
                &[ProviderModelOption {
                    upstream_model_id: "old-model".to_string(),
                    display_name: "Old".to_string(),
                    owned_by: None,
                }],
            )
            .unwrap();
        store
            .save_model_catalog(
                "provider-a",
                "revision-a",
                &[ProviderModelOption {
                    upstream_model_id: "new-model".to_string(),
                    display_name: "New".to_string(),
                    owned_by: None,
                }],
            )
            .unwrap();
        store
            .save_connectivity_report(&ProviderConnectivityReport {
                provider_id: "provider-a".to_string(),
                checked_at: now(),
                status: ConnectivityStatus::Ready,
                model: Some("new-model".to_string()),
                models: Vec::new(),
                models_check: CheckStatus::Ready,
                text_check: CheckStatus::Ready,
                error_code: None,
                wire_api: None,
                catalog_revision_id: None,
            })
            .unwrap();

        let models = store
            .connectivity_report("provider-a")
            .unwrap()
            .unwrap()
            .models;
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].upstream_model_id, "new-model");
    }

    #[test]
    fn request_diagnostics_store_hashes_without_persisting_payload() {
        let store = DiagnosticsStore::in_memory();
        let sentinel = "request-secret-sentinel";
        store
            .record_request(RequestDiagnosticInput {
                instance_id: None,
                session_id: Some("sha256:session".into()),
                route: "/v1/responses".into(),
                stream: false,
                wire_api: "responses",
                public_model_id: "managed-model".into(),
                upstream_model_id: "upstream-model".into(),
                provider_id: "provider-a".into(),
                provider_revision_id: "revision-a".into(),
                http_status: Some(200),
                response_content_type: Some("application/json".into()),
                first_byte_ms: Some(4),
                total_latency_ms: Some(8),
                outcome: RequestDiagnosticOutcome::Success,
                error_code: None,
                error_detail: None,
                input_length: sentinel.len(),
                output_length: sentinel.len(),
                input_tokens: Some(10),
                output_tokens: Some(4),
                total_tokens: Some(14),
                input_hash: Some("sha256:input".into()),
                output_hash: Some("sha256:output".into()),
                tool_call_types: vec!["function".into()],
                tool_result_hashes: vec!["sha256:tool-result".into()],
            })
            .unwrap();
        let events = store.request_recent("missing-instance").unwrap();
        assert!(events.is_empty());
        let connection = store.connection.lock().unwrap();
        let raw: String = connection
            .query_row(
                "SELECT input_hash || output_hash || tool_call_types || tool_result_hashes FROM gateway_request_diagnostics",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!raw.contains(sentinel));
        assert!(raw.contains("sha256:output"));
        let usage: (Option<i64>, Option<i64>, Option<i64>) = connection
            .query_row(
                "SELECT input_tokens, output_tokens, total_tokens FROM gateway_request_diagnostics",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(usage, (Some(10), Some(4), Some(14)));
    }

    #[test]
    fn clearing_instance_removes_request_and_summary_rows_together() {
        let store = DiagnosticsStore::in_memory();
        let connection = store.connection.lock().unwrap();
        connection.execute_batch("PRAGMA foreign_keys=OFF;
            INSERT INTO gateway_request_diagnostics (instance_id, created_at, route, stream, wire_api, public_model_id, upstream_model_id, provider_id, provider_revision_id, outcome, input_length, output_length, tool_call_types, tool_result_hashes) VALUES ('instance-a', 1, '/v1/responses', 0, 'responses', 'public', 'upstream', 'provider', 'revision', 'success', 1, 2, '[]', '[]');
            INSERT INTO conversation_summaries (instance_id, created_at, input_length, output_length, tool_call_types, tool_result_hashes) VALUES ('instance-a', 1, 1, 2, '[]', '[]');").unwrap();
        drop(connection);
        assert_eq!(store.clear_instance("instance-a").unwrap(), 2);
        let connection = store.connection.lock().unwrap();
        let request_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM gateway_request_diagnostics",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let summary_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM conversation_summaries", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(request_count, 0);
        assert_eq!(summary_count, 0);
    }
}
