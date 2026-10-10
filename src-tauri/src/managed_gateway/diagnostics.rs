//! Gateway 请求诊断形状：计数、哈希和枚举，不含正文或 Key。
//!
//! 不阻断模型请求。完整档案走 conversation_archive，不进这里。

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    conversation_archive::RequestCapture,
    diagnostics::{DiagnosticsStore, RequestDiagnosticInput, RequestDiagnosticOutcome},
    routing::{ModelRoute, RouteSource},
    usage::{UsageObservation, UsageStore},
};

#[derive(Clone)]
pub(super) struct RequestMeta {
    pub(super) session_id: Option<String>,
    pub(super) input_len: usize,
    pub(super) input_hash: Option<String>,
    pub(super) tool_call_types: Vec<String>,
    pub(super) tool_result_hashes: Vec<String>,
}

pub(super) fn request_metadata(value: &Value) -> RequestMeta {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let input_hash = (!bytes.is_empty()).then(|| digest(&bytes));
    let session_id = value
        .get("session_id")
        .or_else(|| value.get("conversation_id"))
        .and_then(Value::as_str)
        .map(|id| digest(id.as_bytes()));
    let tool_call_types = value
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool.get("type").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    RequestMeta {
        session_id,
        input_len: bytes.len(),
        input_hash,
        tool_call_types,
        tool_result_hashes: Vec::new(),
    }
}

pub(super) fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rcodex-diagnostic-v1\0");
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

pub(super) fn digest_hasher(hasher: &Sha256) -> String {
    format!("sha256:{:x}", hasher.clone().finalize())
}

pub(super) fn request_event(
    meta: &RequestMeta,
    instance_id: Option<String>,
    route: &ModelRoute,
    wire_api: &'static str,
    status: u16,
    content_type: Option<&str>,
    first_byte_ms: Option<i64>,
    total_latency_ms: Option<i64>,
    outcome: RequestDiagnosticOutcome,
    error_code: Option<&str>,
    output_len: usize,
    output: Option<&[u8]>,
) -> RequestDiagnosticInput {
    RequestDiagnosticInput {
        instance_id,
        session_id: meta.session_id.clone(),
        route: "/v1/responses".into(),
        stream: false,
        wire_api,
        public_model_id: route.public_model_id.clone(),
        upstream_model_id: route.upstream_model_id.clone(),
        provider_id: match &route.source {
            RouteSource::ApiProvider { provider_id, .. } => provider_id.clone(),
            RouteSource::Official { account_id, .. } => account_id.clone(),
        },
        provider_revision_id: match &route.source {
            RouteSource::ApiProvider {
                provider_revision_id,
                ..
            } => provider_revision_id.clone(),
            RouteSource::Official {
                credential_revision_id,
                ..
            } => credential_revision_id.clone(),
        },
        http_status: Some(status),
        response_content_type: content_type.map(str::to_owned),
        first_byte_ms,
        total_latency_ms,
        outcome,
        error_code: error_code.map(str::to_owned),
        error_detail: None,
        input_length: meta.input_len,
        output_length: output_len,
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        input_hash: meta.input_hash.clone(),
        output_hash: output.map(digest),
        tool_call_types: meta.tool_call_types.clone(),
        tool_result_hashes: meta.tool_result_hashes.clone(),
    }
}

#[derive(Clone)]
pub(super) struct RequestRecorder {
    pub(super) diagnostics: Option<DiagnosticsStore>,
    pub(super) usage: Option<UsageStore>,
    pub(super) archive: Option<RequestCapture>,
}

pub(super) fn record_request_best_effort(
    recorder: &Option<RequestRecorder>,
    event: RequestDiagnosticInput,
) {
    let Some(recorder) = recorder else {
        return;
    };
    if let Some(archive) = &recorder.archive {
        // Keep archive delivery independent from the legacy synchronous
        // diagnostic projection. The latter may be unavailable or locked.
        archive.metadata(serde_json::json!({
            "outcome": event.outcome,
            "httpStatus": event.http_status,
            "publicModel": event.public_model_id,
            "upstreamModel": event.upstream_model_id,
            "inputLength": event.input_length,
            "outputLength": event.output_length,
            "inputTokens": event.input_tokens,
            "outputTokens": event.output_tokens,
            "totalTokens": event.total_tokens,
            "errorCode": event.error_code,
        }));
    }
    if let Some(store) = &recorder.diagnostics {
        let _ = store.record_request(event.clone());
    }
    if let Some(store) = &recorder.usage {
        let usage = (event.input_tokens.is_some() || event.output_tokens.is_some()).then_some(
            UsageObservation {
                input_tokens: event.input_tokens,
                output_tokens: event.output_tokens,
            },
        );
        let _ = store.record(
            &event.provider_id,
            Some(&event.upstream_model_id),
            matches!(event.outcome, RequestDiagnosticOutcome::Success),
            usage,
        );
    }
}

pub(super) fn request_event_without_status(
    meta: &RequestMeta,
    instance_id: Option<String>,
    route: &ModelRoute,
    wire_api: &'static str,
    total_latency_ms: i64,
    outcome: RequestDiagnosticOutcome,
    error_code: &'static str,
) -> RequestDiagnosticInput {
    let mut event = request_event(
        meta,
        instance_id,
        route,
        wire_api,
        0,
        None,
        None,
        Some(total_latency_ms),
        outcome,
        Some(error_code),
        0,
        None,
    );
    event.http_status = None;
    event
}
