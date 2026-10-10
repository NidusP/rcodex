//! 模型渠道 IPC。
//!
//! 覆盖 API Key 渠道的列表、创建、改名、更新、删除、默认渠道、用量、余额、超时、
//! 草稿探针和连通性报告。事务、凭据和探针执行仍在对应 Store。
//! 由模型渠道抽屉和工作区调用。

use crate::balance::{BalanceStore, UpstreamBalance};
use crate::channel_probe::{ProbeTicketStore, TicketMatch};
use crate::codex_control::CodexControlStore;
use crate::diagnostics::{self, DiagnosticsStore, ProviderConnectivityReport, ProviderModelOption};
use crate::providers::{CreateProviderInput, ProviderStore, ProviderSummary, UpdateProviderInput};
use crate::settings::{GatewayTimeoutSettings, SettingsStore, UpdateGatewayTimeoutInput};
use crate::usage::{ProviderUsageSummary, UsageStore};
use crate::{channel_probe, connectivity, providers, secrets};

#[tauri::command]
pub fn list_api_key_providers(
    provider_store: tauri::State<'_, ProviderStore>,
) -> Result<Vec<ProviderSummary>, String> {
    provider_store.list()
}

#[tauri::command]
pub fn get_provider_usage_summary(
    provider_id: String,
    usage_store: tauri::State<'_, UsageStore>,
) -> Result<ProviderUsageSummary, String> {
    usage_store.summary(&provider_id)
}

/// Returns only archive metadata. Bodies are read through a separate, paged

#[tauri::command]
pub async fn refresh_provider_upstream_balance(
    provider_id: String,
    provider_store: tauri::State<'_, ProviderStore>,
    balance_store: tauri::State<'_, BalanceStore>,
) -> Result<UpstreamBalance, String> {
    balance_store.refresh(&provider_store, &provider_id).await
}

#[tauri::command]
pub fn get_gateway_timeout_settings(
    provider_id: String,
    settings_store: tauri::State<'_, SettingsStore>,
) -> Result<GatewayTimeoutSettings, String> {
    settings_store.get_gateway_timeout(&provider_id)
}

#[tauri::command]
pub async fn update_gateway_timeout_settings(
    provider_id: String,
    input: UpdateGatewayTimeoutInput,
    control: tauri::State<'_, CodexControlStore>,
    settings_store: tauri::State<'_, SettingsStore>,
) -> Result<GatewayTimeoutSettings, String> {
    control.assert_resource_mutable("api_key_provider", &provider_id)?;
    settings_store.save_gateway_timeout(&provider_id, input)
}

#[tauri::command]
pub async fn test_api_key_provider(
    provider_id: String,
    control: tauri::State<'_, CodexControlStore>,
    provider_store: tauri::State<'_, ProviderStore>,
    diagnostics_store: tauri::State<'_, DiagnosticsStore>,
) -> Result<ProviderConnectivityReport, String> {
    let mutable = control
        .assert_resource_mutable("api_key_provider", &provider_id)
        .is_ok();
    connectivity::test_provider(&provider_id, &provider_store, &diagnostics_store, mutable).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DraftChannelProbe {
    probe_ticket_id: String,
    status: diagnostics::ConnectivityStatus,
    wire_api: Option<String>,
    model: Option<String>,
    models: Vec<ProviderModelOption>,
    models_check: diagnostics::CheckStatus,
    text_check: diagnostics::CheckStatus,
    error_code: Option<String>,
}

#[tauri::command]
pub async fn probe_channel_draft(
    base_url: String,
    api_key: String,
    tickets: tauri::State<'_, ProbeTicketStore>,
) -> Result<DraftChannelProbe, String> {
    let (base_url, api_key) = channel_probe::validate_draft(base_url, api_key)?;
    let client = channel_probe::probe_client()?;
    let execution = channel_probe::execute(&client, &base_url, &api_key).await;
    let snapshot = execution.snapshot.clone();
    let probe_ticket_id = tickets.insert(&base_url, &api_key, execution);
    Ok(DraftChannelProbe {
        probe_ticket_id,
        status: snapshot.status,
        wire_api: snapshot.wire_api,
        model: snapshot.model,
        models: snapshot.models,
        models_check: snapshot.models_check,
        text_check: snapshot.text_check,
        error_code: snapshot.error_code,
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateProviderRequest {
    #[serde(flatten)]
    input: CreateProviderInput,
    #[serde(default)]
    probe_ticket_id: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreatedApiKeyProvider {
    #[serde(flatten)]
    provider: ProviderSummary,
    probe_attachment: String,
}

#[tauri::command]
pub fn get_provider_connectivity_report(
    provider_id: String,
    diagnostics_store: tauri::State<'_, DiagnosticsStore>,
) -> Result<Option<ProviderConnectivityReport>, String> {
    diagnostics_store.connectivity_report(&provider_id)
}

#[tauri::command]
pub async fn create_api_key_provider(
    input: CreateProviderRequest,
    provider_store: tauri::State<'_, ProviderStore>,
    diagnostics_store: tauri::State<'_, DiagnosticsStore>,
    tickets: tauri::State<'_, ProbeTicketStore>,
) -> Result<CreatedApiKeyProvider, String> {
    let CreateProviderRequest {
        mut input,
        probe_ticket_id,
    } = input;
    let mut pending = None;
    let mut probe_attachment = "not_requested".to_string();
    if let Some(ticket_id) = probe_ticket_id {
        let base_url = providers::validate_base_url(input.base_url.clone())?;
        let api_key = secrets::validate_api_key(input.api_key.clone())?;
        match tickets.take_matching(&ticket_id, &base_url, api_key.expose()) {
            TicketMatch::Matched(execution) => {
                if let Some(mode) = execution
                    .snapshot
                    .wire_api
                    .as_deref()
                    .and_then(channel_probe::transport_for_wire)
                {
                    input.transport_mode = mode;
                }
                pending = Some(execution);
                probe_attachment = "attached".to_string();
            }
            TicketMatch::Mismatched => probe_attachment = "mismatched".to_string(),
            TicketMatch::Missing => probe_attachment = "expired".to_string(),
        }
    }
    let provider = provider_store.create(input)?;
    if let Some(execution) = pending {
        match provider_store.upstream_for_provider(&provider.id) {
            Ok(upstream) => {
                if diagnostics_store
                    .store_channel_probe(
                        &provider.id,
                        &upstream.provider_revision_id,
                        &execution.snapshot,
                    )
                    .is_err()
                {
                    probe_attachment = "failed".to_string();
                } else {
                    connectivity::record_probe_calls(
                        &diagnostics_store,
                        &provider.id,
                        &execution.calls,
                    );
                }
            }
            Err(_) => probe_attachment = "failed".to_string(),
        }
    }
    Ok(CreatedApiKeyProvider {
        provider,
        probe_attachment,
    })
}

#[tauri::command]
pub async fn select_default_api_key_provider(
    provider_id: String,
    control: tauri::State<'_, CodexControlStore>,
    provider_store: tauri::State<'_, ProviderStore>,
) -> Result<(), String> {
    control.assert_resource_mutable("api_key_provider", &provider_id)?;
    provider_store.select_default(&provider_id)
}

#[tauri::command]
pub async fn update_api_key_provider(
    provider_id: String,
    input: UpdateProviderInput,
    control: tauri::State<'_, CodexControlStore>,
    provider_store: tauri::State<'_, ProviderStore>,
) -> Result<ProviderSummary, String> {
    control.assert_resource_mutable("api_key_provider", &provider_id)?;
    provider_store.update(&provider_id, input)
}

#[tauri::command]
pub async fn rename_api_key_provider(
    provider_id: String,
    name: String,
    provider_store: tauri::State<'_, ProviderStore>,
) -> Result<ProviderSummary, String> {
    provider_store.rename(&provider_id, name)
}

#[tauri::command]
pub async fn delete_api_key_provider(
    provider_id: String,
    control: tauri::State<'_, CodexControlStore>,
    provider_store: tauri::State<'_, ProviderStore>,
) -> Result<(), String> {
    control.assert_resource_mutable("api_key_provider", &provider_id)?;
    provider_store.delete(&provider_id)
}
