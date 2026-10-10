//! GatewaySummarizer: remote-v2 compact for third-party routes.
//!
//! Only `compactMode = remote_v2` plus `compaction_support = gateway_summarizer`
//! may enter this path. Isolated / local_client launches stay fail-closed.

use std::time::{Duration, Instant};

use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
    Engine as _,
};
use reqwest::Response as UpstreamResponse;
use serde_json::{json, Map, Value};
use tokio::time::timeout;
use uuid::Uuid;

use super::diagnostics::{
    record_request_best_effort, request_event, request_metadata, RequestRecorder,
};
use super::errors::{
    annotate_upstream_detail, api_error, api_error_with_detail, bounded_response_bytes,
    classify_upstream_send, upstream_error_detail,
};
use super::forward::{apply_upstream_auth, register_archive_secrets, transport_failure};
use super::helpers::{
    auth_kind, gateway_trace, is_loopback_url, route_kind, route_provider_id, safe_upstream_url,
    upstream_request_url,
};
use super::protocol::{
    is_event_stream_content_type, request_contains_compaction_trigger, response_request_to_chat,
    sse_event, usage_from_event_value,
};
use super::runtime_types::ProviderUpstreamRuntime;
use super::MAX_RESPONSE_BYTES;
use crate::diagnostics::RequestDiagnosticOutcome;
use crate::routing::{
    looks_like_gpt_model_id, CompactMode, CompactionSupport, ModelRoute, RouteSource, WireApi,
};

pub(super) const ENVELOPE_PREFIX: &str = "3r-compaction-v1:";

pub(super) fn preflight_compact(
    route: &ModelRoute,
    compact_mode: CompactMode,
) -> Result<(), &'static str> {
    if matches!(route.source, RouteSource::Official { .. }) {
        return Ok(());
    }
    match compact_mode {
        CompactMode::LocalClient => Err("unsupported_compaction"),
        CompactMode::RemoteV2 => match route.capabilities.compaction_support {
            CompactionSupport::NativeRemote | CompactionSupport::GatewaySummarizer => Ok(()),
            CompactionSupport::Unsupported => {
                if looks_like_gpt_model_id(&route.public_model_id, &route.upstream_model_id) {
                    Err("not_native_gpt_compaction")
                } else {
                    Err("unsupported_compaction")
                }
            }
        },
    }
}

pub(super) fn should_summarize(
    compact_mode: CompactMode,
    route: &ModelRoute,
    request: &Value,
) -> bool {
    compact_mode == CompactMode::RemoteV2
        && route.capabilities.compaction_support == CompactionSupport::GatewaySummarizer
        && request_contains_compaction_trigger(request)
}

pub(super) fn encode_envelope(text: &str) -> String {
    format!(
        "{ENVELOPE_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(text.as_bytes())
    )
}

fn decode_envelope(value: &str) -> Result<String, &'static str> {
    let payload = value
        .strip_prefix(ENVELOPE_PREFIX)
        .ok_or("incompatible_compaction_envelope")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.as_bytes())
        .or_else(|_| URL_SAFE.decode(payload.as_bytes()))
        .map_err(|_| "incompatible_compaction_envelope")?;
    String::from_utf8(bytes).map_err(|_| "incompatible_compaction_envelope")
}

fn encrypted_content_in(value: &Value) -> Option<&str> {
    match value {
        Value::Object(object) => {
            if let Some(content) = object.get("encrypted_content").and_then(Value::as_str) {
                return Some(content);
            }
            if object.get("type").and_then(Value::as_str) == Some("encrypted_content") {
                if let Some(content) = object.get("content").and_then(Value::as_str) {
                    return Some(content);
                }
                return Some("");
            }
            object.values().find_map(encrypted_content_in)
        }
        Value::Array(values) => values.iter().find_map(encrypted_content_in),
        _ => None,
    }
}

fn item_type(item: &Value) -> &str {
    item.get("type").and_then(Value::as_str).unwrap_or("")
}

fn strip_encrypted_content(value: &mut Value) -> bool {
    match value {
        Value::Object(object) => {
            let mut removed = object.remove("encrypted_content").is_some();
            let keys: Vec<String> = object.keys().cloned().collect();
            for key in keys {
                if let Some(nested) = object.get_mut(&key) {
                    if strip_encrypted_content(nested) {
                        removed = true;
                    }
                }
            }
            removed
        }
        Value::Array(values) => {
            let before = values.len();
            values.retain(|item| item_type(item) != "encrypted_content");
            let mut removed = values.len() != before;
            for nested in values.iter_mut() {
                if strip_encrypted_content(nested) {
                    removed = true;
                }
            }
            removed
        }
        _ => false,
    }
}

fn collect_visible_text_into(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::String(text) if !text.is_empty() => parts.push(text.clone()),
        Value::Array(values) => {
            for item in values {
                collect_visible_text_into(item, parts);
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("encrypted_content") {
                return;
            }
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    parts.push(text.to_string());
                }
            }
            if let Some(summary) = object.get("summary") {
                collect_visible_text_into(summary, parts);
            }
            if let Some(content) = object.get("content") {
                collect_visible_text_into(content, parts);
            }
        }
        _ => {}
    }
}

fn visible_reasoning_text(item: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(summary) = item.get("summary") {
        collect_visible_text_into(summary, &mut parts);
    }
    if let Some(content) = item.get("content") {
        collect_visible_text_into(content, &mut parts);
    }
    parts.join("\n")
}

fn fold_non_compaction_item(mut item: Value) -> Result<Value, &'static str> {
    strip_encrypted_content(&mut item);
    if encrypted_content_in(&item).is_some() {
        Err("incompatible_reasoning_envelope")
    } else {
        Ok(item)
    }
}

/// Summarizer-only: drop reasoning ciphertext, keep visible summary/content
/// as assistant text so Chat adapters can still summarize. Ordinary
/// forwarding must not call this.
fn fold_summarizer_encrypted_content(request: &mut Map<String, Value>) -> Result<(), &'static str> {
    match request.get_mut("input") {
        Some(Value::Array(items)) => {
            let mut folded = Vec::with_capacity(items.len());
            for item in items.drain(..) {
                match item_type(&item) {
                    "compaction" => return Err("incompatible_compaction_envelope"),
                    "reasoning" => {
                        let text = visible_reasoning_text(&item);
                        if !text.trim().is_empty() {
                            folded.push(assistant_history_item(text));
                        }
                    }
                    _ => folded.push(fold_non_compaction_item(item)?),
                }
            }
            *items = folded;
            Ok(())
        }
        Some(value) if encrypted_content_in(value).is_some() => {
            Err("incompatible_reasoning_envelope")
        }
        _ => Ok(()),
    }
}

fn assistant_history_item(text: String) -> Value {
    json!({
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text}]
    })
}

/// Expand `3r-compaction-v1` compaction items to assistant text so later
/// ordinary turns can continue. Other item types, including
/// `reasoning.encrypted_content`, are left for the history-compat slice.
/// NativeRemote / compatibility fallback must not call this.
pub(super) fn expand_compaction_history(
    request: &Map<String, Value>,
) -> Result<Map<String, Value>, &'static str> {
    let mut prepared = request.clone();
    if let Some(Value::Array(items)) = prepared.get_mut("input") {
        let mut expanded = Vec::with_capacity(items.len());
        for item in items.iter() {
            let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
            if kind == "compaction" {
                let content = item
                    .get("encrypted_content")
                    .and_then(Value::as_str)
                    .ok_or("incompatible_compaction_envelope")?;
                expanded.push(assistant_history_item(decode_envelope(content)?));
                continue;
            }
            expanded.push(item.clone());
        }
        prepared.insert("input".to_string(), Value::Array(expanded));
    }
    Ok(prepared)
}

/// Summarizer path only: expand R Codex envelopes, fold reasoning ciphertext,
/// strip the trigger, and empty tools. Ordinary forwarding must not fold.
fn capped_summarizer_effort(effort: &str) -> Option<&'static str> {
    matches!(effort, "high" | "xhigh" | "max" | "ultra").then_some("medium")
}

fn cap_summarizer_reasoning_effort(request: &mut Map<String, Value>) {
    let replace_reasoning = match request.get("reasoning") {
        Some(Value::String(effort)) => capped_summarizer_effort(effort).map(|capped| json!(capped)),
        _ => None,
    };
    if let Some(capped) = replace_reasoning {
        request.insert("reasoning".to_string(), capped);
    } else if let Some(Value::Object(object)) = request.get_mut("reasoning") {
        let next = object
            .get("effort")
            .and_then(Value::as_str)
            .and_then(capped_summarizer_effort)
            .map(|capped| json!(capped));
        if let Some(capped) = next {
            object.insert("effort".to_string(), capped);
        }
    }
    let replace_effort = request
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .and_then(capped_summarizer_effort)
        .map(|capped| json!(capped));
    if let Some(capped) = replace_effort {
        request.insert("reasoning_effort".to_string(), capped);
    }
}

pub(super) fn prepare_summarizer_request(
    request: &Map<String, Value>,
) -> Result<Map<String, Value>, &'static str> {
    let mut prepared = expand_compaction_history(request)?;
    fold_summarizer_encrypted_content(&mut prepared)?;
    if let Some(Value::Array(items)) = prepared.get_mut("input") {
        items.retain(|item| item.get("type").and_then(Value::as_str) != Some("compaction_trigger"));
    }
    prepared.insert("tools".to_string(), json!([]));
    prepared.remove("tool_choice");
    prepared.remove("parallel_tool_calls");
    prepared.insert("stream".to_string(), Value::Bool(true));
    cap_summarizer_reasoning_effort(&mut prepared);
    Ok(prepared)
}

fn collect_output_text(value: &Value) -> String {
    let mut parts = Vec::new();
    collect_output_text_into(value, &mut parts);
    parts.join("\n")
}

fn collect_output_text_into(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::Array(values) => {
            for item in values {
                collect_output_text_into(item, parts);
            }
        }
        Value::Object(object) => {
            let kind = object.get("type").and_then(Value::as_str).unwrap_or("");
            if kind == "output_text" || kind == "text" {
                if let Some(text) = object.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        parts.push(text.to_string());
                    }
                }
                return;
            }
            for (key, nested) in object {
                if key == "encrypted_content" {
                    continue;
                }
                collect_output_text_into(nested, parts);
            }
        }
        _ => {}
    }
}

pub(super) fn extract_summary_from_responses(value: &Value) -> Result<String, &'static str> {
    match value.get("status").and_then(Value::as_str) {
        Some("incomplete") => return Err("compaction_summary_truncated"),
        Some("failed") => return Err("compaction_summary_failed"),
        Some("completed") => {}
        _ => return Err("invalid_upstream_response"),
    }
    let text = collect_output_text(value.get("output").unwrap_or(&Value::Null));
    if text.trim().is_empty() {
        Err("compaction_summary_empty")
    } else {
        Ok(text)
    }
}

pub(super) fn extract_summary_from_chat(value: &Value) -> Result<String, &'static str> {
    let reason = value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");
    match reason {
        "length" | "content_filter" => return Err("compaction_summary_truncated"),
        "tool_calls" | "function_call" => return Err("unsupported_chat_output"),
        _ => {}
    }
    let text = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/choices/0/text").and_then(Value::as_str))
        .unwrap_or("");
    if text.trim().is_empty() {
        Err("compaction_summary_empty")
    } else {
        Ok(text.to_string())
    }
}

fn summary_from_responses_bytes(
    bytes: &[u8],
    content_type: Option<&str>,
) -> Result<String, &'static str> {
    if is_event_stream_content_type(content_type) || looks_like_sse(bytes) {
        return extract_summary_from_responses_sse(bytes);
    }
    let value = serde_json::from_slice::<Value>(bytes).map_err(|_| "invalid_upstream_response")?;
    extract_summary_from_responses(&value)
}

fn summary_from_chat_bytes(
    bytes: &[u8],
    content_type: Option<&str>,
) -> Result<String, &'static str> {
    if is_event_stream_content_type(content_type) || looks_like_sse(bytes) {
        return extract_summary_from_chat_sse(bytes);
    }
    let value = serde_json::from_slice::<Value>(bytes).map_err(|_| "invalid_upstream_response")?;
    extract_summary_from_chat(&value)
}

fn looks_like_sse(bytes: &[u8]) -> bool {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|index| &bytes[index..])
        .unwrap_or(bytes);
    start.starts_with(b"event:") || start.starts_with(b"data:")
}

fn extract_summary_from_responses_sse(bytes: &[u8]) -> Result<String, &'static str> {
    let mut completed = None;
    for event in sse_data_payloads(bytes)? {
        if event.get("type").and_then(Value::as_str) == Some("response.completed") {
            completed = event.get("response").cloned();
        } else if event.get("object").and_then(Value::as_str) == Some("response")
            && event.get("status").and_then(Value::as_str) == Some("completed")
        {
            completed = Some(event);
        }
    }
    extract_summary_from_responses(completed.as_ref().ok_or("invalid_upstream_response")?)
}

fn extract_summary_from_chat_sse(bytes: &[u8]) -> Result<String, &'static str> {
    let mut text = String::new();
    let mut finish_reason = None;
    for event in sse_data_payloads(bytes)? {
        if let Some(reason) = event
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
        {
            finish_reason = Some(reason.to_string());
        }
        if let Some(delta) = event
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            .or_else(|| {
                event
                    .pointer("/choices/0/message/content")
                    .and_then(Value::as_str)
            })
        {
            text.push_str(delta);
        }
    }
    extract_summary_from_chat(&json!({
        "choices": [{
            "message": {"content": text},
            "finish_reason": finish_reason.unwrap_or_else(|| "stop".to_string())
        }]
    }))
}

fn sse_data_payloads(bytes: &[u8]) -> Result<Vec<Value>, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "invalid_upstream_sse_utf8")?;
    let mut payloads = Vec::new();
    let mut data_lines = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.strip_prefix(' ').unwrap_or(data).to_string());
        } else if line.is_empty() && !data_lines.is_empty() {
            let payload = data_lines.join("\n");
            data_lines.clear();
            if payload == "[DONE]" {
                continue;
            }
            payloads.push(serde_json::from_str(&payload).map_err(|_| "invalid_upstream_response")?);
        }
    }
    if !data_lines.is_empty() {
        let payload = data_lines.join("\n");
        if payload != "[DONE]" {
            payloads.push(serde_json::from_str(&payload).map_err(|_| "invalid_upstream_response")?);
        }
    }
    Ok(payloads)
}

pub(super) fn compaction_response_json(public_model: &str, summary: &str) -> Value {
    let response_id = format!("resp_{}", Uuid::new_v4().simple());
    let item_id = format!("cmp_{}", Uuid::new_v4().simple());
    json!({
        "id": response_id,
        "object": "response",
        "created_at": 1,
        "model": public_model,
        "status": "completed",
        "error": null,
        "incomplete_details": null,
        "instructions": null,
        "max_output_tokens": null,
        "output": [{
            "id": item_id,
            "type": "compaction",
            "encrypted_content": encode_envelope(summary)
        }],
        "parallel_tool_calls": false,
        "previous_response_id": null,
        "reasoning": {"effort": null, "summary": null},
        "store": false,
        "temperature": 1.0,
        "text": {"format": {"type": "text"}},
        "tool_choice": "auto",
        "tools": [],
        "top_p": 1.0,
        "truncation": "disabled",
        "usage": null,
        "metadata": {}
    })
}

fn compaction_client_response(public_model: &str, summary: &str, stream: bool) -> Response {
    let value = compaction_response_json(public_model, summary);
    if !stream {
        return (StatusCode::OK, Json(value)).into_response();
    }
    let mut created = value.clone();
    created["status"] = json!("in_progress");
    created["output"] = json!([]);
    created["usage"] = Value::Null;
    let item = value["output"][0].clone();
    let body = [
        sse_event(
            "response.created",
            json!({
                "type": "response.created",
                "sequence_number": 0,
                "response": created
            }),
        ),
        sse_event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "sequence_number": 1,
                "output_index": 0,
                "item": item
            }),
        ),
        sse_event(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "sequence_number": 2,
                "output_index": 0,
                "item": item
            }),
        ),
        sse_event(
            "response.completed",
            json!({
                "type": "response.completed",
                "sequence_number": 3,
                "response": value
            }),
        ),
    ]
    .into_iter()
    .flat_map(|event| event.to_vec())
    .collect::<Vec<_>>();
    let mut response = Response::new(Body::from(body));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
    response
}

fn tools_len(request: &Map<String, Value>) -> usize {
    request
        .get("tools")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn compact_trace(
    instance: &str,
    route: &ModelRoute,
    tools_len: usize,
    code: &str,
    url: Option<&str>,
) {
    gateway_trace(&[
        ("event", "compact_summarize"),
        ("instance", instance),
        ("route_id", &route.id),
        ("public_model", &route.public_model_id),
        ("upstream_model", &route.upstream_model_id),
        ("provider", route_provider_id(route)),
        ("route_kind", route_kind(route)),
        ("support", "gateway_summarizer"),
        ("envelope", "v1"),
        ("trigger", "1"),
        ("tools_len", &tools_len.to_string()),
        ("code", code),
        ("url", url.unwrap_or("-")),
    ]);
}

pub(super) async fn run_summarizer(
    upstream: &ProviderUpstreamRuntime,
    route: &ModelRoute,
    request: Map<String, Value>,
    instance_id: Option<String>,
    recorder: Option<RequestRecorder>,
) -> Response {
    let started = Instant::now();
    let client_stream = request
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let inbound_tools = tools_len(&request);
    let instance = instance_id.clone().unwrap_or_default();
    let original = Value::Object(request.clone());
    let input_meta = request_metadata(&original);

    let prepared = match prepare_summarizer_request(&request) {
        Ok(prepared) => prepared,
        Err(code) => {
            compact_trace(&instance, route, inbound_tools, code, None);
            record_request_best_effort(
                &recorder,
                request_event(
                    &input_meta,
                    instance_id,
                    route,
                    match route.wire_api {
                        WireApi::Responses => "responses",
                        WireApi::ChatCompletions => "chat_completions",
                    },
                    400,
                    None,
                    None,
                    Some(started.elapsed().as_millis() as i64),
                    RequestDiagnosticOutcome::Failed,
                    Some(code),
                    0,
                    None,
                ),
            );
            return api_error(StatusCode::BAD_REQUEST, code);
        }
    };

    if let Some(capture) = recorder.as_ref().and_then(|item| item.archive.as_ref()) {
        register_archive_secrets(capture, upstream, None);
        capture.metadata(json!({
            "compact": "gateway_summarizer",
            "envelope": "v1",
            "publicModel": route.public_model_id,
            "upstreamModel": route.upstream_model_id,
        }));
    }

    match route.wire_api {
        WireApi::Responses => {
            collect_and_wrap(
                upstream,
                route,
                Value::Object(prepared),
                false,
                instance_id,
                recorder,
                input_meta,
                inbound_tools,
                client_stream,
                started,
            )
            .await
        }
        WireApi::ChatCompletions => {
            let chat = match response_request_to_chat(&Value::Object(prepared)) {
                Ok(chat) => chat,
                Err(code) => {
                    compact_trace(&instance, route, inbound_tools, code, None);
                    return api_error(StatusCode::BAD_REQUEST, code);
                }
            };
            collect_and_wrap(
                upstream,
                route,
                chat,
                true,
                instance_id,
                recorder,
                input_meta,
                inbound_tools,
                client_stream,
                started,
            )
            .await
        }
    }
}

async fn collect_and_wrap(
    upstream: &ProviderUpstreamRuntime,
    route: &ModelRoute,
    outbound_body: Value,
    chat: bool,
    instance_id: Option<String>,
    recorder: Option<RequestRecorder>,
    input_meta: super::diagnostics::RequestMeta,
    inbound_tools: usize,
    client_stream: bool,
    started: Instant,
) -> Response {
    let instance = instance_id.clone().unwrap_or_default();
    let body = match serde_json::to_vec(&outbound_body) {
        Ok(body) => body,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "serialization_failed"),
    };
    if let Some(capture) = recorder.as_ref().and_then(|item| item.archive.as_ref()) {
        capture.stage("upstream-request", &body);
    }
    let wire = if chat {
        WireApi::ChatCompletions
    } else {
        WireApi::Responses
    };
    let url = upstream_request_url(&upstream.base_url, wire);
    let safe_url = safe_upstream_url(&url);
    compact_trace(&instance, route, inbound_tools, "forward", Some(&safe_url));
    gateway_trace(&[
        ("event", "forward"),
        ("instance", &instance),
        ("public_model", &route.public_model_id),
        ("upstream_model", &route.upstream_model_id),
        ("provider", &upstream.provider_id),
        ("route_kind", route_kind(route)),
        ("auth", auth_kind(upstream, None)),
        ("url", &safe_url),
        ("loopback", if is_loopback_url(&url) { "1" } else { "0" }),
        ("stream", "1"),
        ("compact", "gateway_summarizer"),
    ]);
    let outbound = apply_upstream_auth(
        upstream
            .client
            .post(&url)
            .header(CONTENT_TYPE, "application/json"),
        upstream,
        None,
    )
    .body(body);
    let response = match timeout(
        Duration::from_secs(upstream.timeouts.first_byte_seconds),
        outbound.send(),
    )
    .await
    {
        Err(_) => {
            return transport_failure(
                &recorder,
                &input_meta,
                instance_id,
                route,
                if chat {
                    "chat_completions"
                } else {
                    "responses"
                },
                client_stream,
                RequestDiagnosticOutcome::Timeout,
                "upstream_first_byte_timeout",
                StatusCode::GATEWAY_TIMEOUT,
                &url,
                None,
                started,
            );
        }
        Ok(Err(error)) => {
            let (code, status) = classify_upstream_send(&error);
            let kind = if error.is_timeout() {
                "timeout"
            } else if error.is_connect() {
                "connect"
            } else {
                "send"
            };
            return transport_failure(
                &recorder,
                &input_meta,
                instance_id,
                route,
                if chat {
                    "chat_completions"
                } else {
                    "responses"
                },
                client_stream,
                RequestDiagnosticOutcome::Failed,
                code,
                status,
                &url,
                Some(kind),
                started,
            );
        }
        Ok(Ok(response)) => response,
    };
    finish_summarizer_upstream(
        response,
        route,
        chat,
        instance_id,
        recorder,
        input_meta,
        inbound_tools,
        client_stream,
        &url,
        started,
        upstream.timeouts.non_stream_total_seconds,
    )
    .await
}

async fn finish_summarizer_upstream(
    response: UpstreamResponse,
    route: &ModelRoute,
    chat: bool,
    instance_id: Option<String>,
    recorder: Option<RequestRecorder>,
    input_meta: super::diagnostics::RequestMeta,
    inbound_tools: usize,
    client_stream: bool,
    url: &str,
    started: Instant,
    timeout_seconds: u64,
) -> Response {
    let instance = instance_id.clone().unwrap_or_default();
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if let Some(capture) = recorder.as_ref().and_then(|item| item.archive.as_ref()) {
        capture.metadata(json!({
            "stage": "upstream-response",
            "status": status.as_u16(),
            "contentType": content_type,
            "compact": "gateway_summarizer",
        }));
    }
    if !status.is_success() {
        let detail = upstream_error_detail(
            response,
            recorder.as_ref().and_then(|item| item.archive.as_ref()),
        )
        .await;
        let annotated = annotate_upstream_detail(url, detail.clone());
        compact_trace(
            &instance,
            route,
            inbound_tools,
            "upstream_error",
            Some(&safe_upstream_url(url)),
        );
        let mut event = request_event(
            &input_meta,
            instance_id,
            route,
            if chat {
                "chat_completions"
            } else {
                "responses"
            },
            status.as_u16(),
            content_type.as_deref(),
            None,
            Some(started.elapsed().as_millis() as i64),
            RequestDiagnosticOutcome::Failed,
            Some("upstream_error"),
            0,
            None,
        );
        event.stream = client_stream;
        event.error_detail = annotated;
        record_request_best_effort(&recorder, event);
        return api_error_with_detail(status, "upstream_error", Some(url), detail);
    }
    let bytes = match timeout(
        Duration::from_secs(timeout_seconds),
        bounded_response_bytes(
            response,
            MAX_RESPONSE_BYTES,
            recorder.as_ref().and_then(|item| item.archive.as_ref()),
        ),
    )
    .await
    {
        Err(_) => {
            compact_trace(
                &instance,
                route,
                inbound_tools,
                "upstream_response_timeout",
                Some(&safe_upstream_url(url)),
            );
            return api_error(StatusCode::GATEWAY_TIMEOUT, "upstream_response_timeout");
        }
        Ok(Err(_)) => {
            compact_trace(
                &instance,
                route,
                inbound_tools,
                "upstream_interrupted",
                Some(&safe_upstream_url(url)),
            );
            return api_error(StatusCode::BAD_GATEWAY, "upstream_interrupted");
        }
        Ok(Ok(bytes)) => bytes,
    };
    let summary = if chat {
        summary_from_chat_bytes(&bytes, content_type.as_deref())
    } else {
        summary_from_responses_bytes(&bytes, content_type.as_deref())
    };
    match summary {
        Ok(text) => {
            compact_trace(
                &instance,
                route,
                inbound_tools,
                "ok",
                Some(&safe_upstream_url(url)),
            );
            let mut event = request_event(
                &input_meta,
                instance_id,
                route,
                if chat {
                    "chat_completions"
                } else {
                    "responses"
                },
                status.as_u16(),
                content_type.as_deref(),
                None,
                Some(started.elapsed().as_millis() as i64),
                RequestDiagnosticOutcome::Success,
                None,
                bytes.len(),
                Some(&bytes),
            );
            event.stream = client_stream;
            if let Some(usage) = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|value| usage_from_event_value(&value))
            {
                event.input_tokens = usage.input_tokens;
                event.output_tokens = usage.output_tokens;
                event.total_tokens = usage.total_tokens;
            }
            record_request_best_effort(&recorder, event);
            compaction_client_response(&route.public_model_id, &text, client_stream)
        }
        Err(code) => {
            compact_trace(
                &instance,
                route,
                inbound_tools,
                code,
                Some(&safe_upstream_url(url)),
            );
            let status = if code.starts_with("compaction_summary_") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::BAD_GATEWAY
            };
            let mut event = request_event(
                &input_meta,
                instance_id,
                route,
                if chat {
                    "chat_completions"
                } else {
                    "responses"
                },
                status.as_u16(),
                content_type.as_deref(),
                None,
                Some(started.elapsed().as_millis() as i64),
                RequestDiagnosticOutcome::Failed,
                Some(code),
                bytes.len(),
                Some(&bytes),
            );
            event.stream = client_stream;
            record_request_best_effort(&recorder, event);
            api_error(status, code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prepare_summarizer_strips_trigger_expands_envelope_and_rejects_foreign_blobs() {
        let envelope = encode_envelope("prior summary");
        let prepared_request = json!({
            "model": "grok-4.6",
            "tools": [{"type": "function", "name": "shell"}],
            "tool_choice": "auto",
            "stream": true,
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                {"type": "compaction", "encrypted_content": envelope},
                {"type": "compaction_trigger"}
            ]
        });
        let prepared = prepare_summarizer_request(prepared_request.as_object().unwrap()).unwrap();
        let input = prepared["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[1]["content"][0]["text"], "prior summary");
        assert_eq!(prepared["tools"], json!([]));
        assert_eq!(prepared["stream"], true);
        assert!(prepared.get("tool_choice").is_none());

        let rejected_request = json!({
            "input": [{
                "type": "compaction",
                "encrypted_content": "openai-compaction-v1:abc"
            }]
        });
        let rejected = prepare_summarizer_request(rejected_request.as_object().unwrap());
        assert_eq!(rejected, Err("incompatible_compaction_envelope"));

        let continued = expand_compaction_history(prepared_request.as_object().unwrap()).unwrap();
        let continued_input = continued["input"].as_array().unwrap();
        assert_eq!(continued_input.len(), 3);
        assert_eq!(continued_input[1]["role"], "assistant");
        assert_eq!(continued_input[2]["type"], "compaction_trigger");
        assert_eq!(continued["tools"][0]["name"], "shell");
        assert_eq!(continued["stream"], true);

        let with_reasoning = json!({
            "input": [
                {"type": "reasoning", "encrypted_content": "provider-blob"},
                {"type": "compaction", "encrypted_content": envelope},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next"}]}
            ]
        });
        let expanded = expand_compaction_history(with_reasoning.as_object().unwrap()).unwrap();
        let items = expanded["input"].as_array().unwrap();
        assert_eq!(items[0]["type"], "reasoning");
        assert_eq!(items[0]["encrypted_content"], "provider-blob");
        assert_eq!(items[1]["role"], "assistant");
        assert_eq!(items[1]["content"][0]["text"], "prior summary");

        let folded = prepare_summarizer_request(with_reasoning.as_object().unwrap()).unwrap();
        let folded_input = folded["input"].as_array().unwrap();
        assert_eq!(folded_input.len(), 2);
        assert_eq!(folded_input[0]["role"], "assistant");
        assert_eq!(folded_input[0]["content"][0]["text"], "prior summary");
        assert_eq!(folded_input[1]["role"], "user");
        assert!(folded_input
            .iter()
            .all(|item| encrypted_content_in(item).is_none()));
    }

    #[test]
    fn prepare_summarizer_folds_reasoning_ciphertext_and_rejects_foreign_compaction() {
        let reasoning_only = json!({
            "model": "grok-4.6",
            "input": [
                {
                    "type": "reasoning",
                    "encrypted_content": "provider-blob",
                    "summary": [{"type": "summary_text", "text": "visible thought"}]
                },
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                {"type": "compaction_trigger"}
            ]
        });
        let prepared = prepare_summarizer_request(reasoning_only.as_object().unwrap()).unwrap();
        let input = prepared["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "assistant");
        assert_eq!(input[0]["content"][0]["text"], "visible thought");
        assert_eq!(input[1]["role"], "user");
        assert!(input
            .iter()
            .all(|item| encrypted_content_in(item).is_none()));
        assert_eq!(prepared["tools"], json!([]));
        assert_eq!(prepared["stream"], true);
        let chat = response_request_to_chat(&Value::Object(prepared.clone())).unwrap();
        let messages = chat["messages"].as_array().unwrap();
        assert!(messages.iter().any(
            |message| message["role"] == "assistant" && message["content"] == "visible thought"
        ));
        assert!(messages
            .iter()
            .any(|message| message["role"] == "user" && message["content"] == "hi"));

        let ciphertext_only = json!({
            "input": [
                {"type": "reasoning", "encrypted_content": "provider-blob"},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                {"type": "compaction_trigger"}
            ]
        });
        let dropped = prepare_summarizer_request(ciphertext_only.as_object().unwrap()).unwrap();
        let dropped_input = dropped["input"].as_array().unwrap();
        assert_eq!(dropped_input.len(), 1);
        assert_eq!(dropped_input[0]["role"], "user");

        let only_reasoning = json!({
            "input": [
                {"type": "reasoning", "encrypted_content": "provider-blob"},
                {"type": "compaction_trigger"}
            ]
        });
        let only = prepare_summarizer_request(only_reasoning.as_object().unwrap()).unwrap();
        assert_eq!(only["input"].as_array().unwrap().len(), 0);

        let tool_output = json!({
            "input": [
                {
                    "type": "custom_tool_call_output",
                    "call_id": "c1",
                    "output": "ok",
                    "encrypted_content": "tool-blob"
                },
                {"type": "compaction_trigger"}
            ]
        });
        let folded_tool = prepare_summarizer_request(tool_output.as_object().unwrap()).unwrap();
        let tool_items = folded_tool["input"].as_array().unwrap();
        assert_eq!(tool_items.len(), 1);
        assert_eq!(tool_items[0]["type"], "custom_tool_call_output");
        assert_eq!(tool_items[0]["output"], "ok");
        assert!(tool_items[0].get("encrypted_content").is_none());

        let foreign = json!({
            "input": [
                {"type": "compaction", "encrypted_content": "openai-compaction-v1:abc"},
                {"type": "compaction_trigger"}
            ]
        });
        assert_eq!(
            prepare_summarizer_request(foreign.as_object().unwrap()),
            Err("incompatible_compaction_envelope")
        );
    }

    #[test]
    fn empty_and_truncated_summaries_fail_closed() {
        assert_eq!(
            extract_summary_from_responses(&json!({
                "status": "completed",
                "output": [{"type": "message", "content": [{"type": "output_text", "text": ""}]}]
            })),
            Err("compaction_summary_empty")
        );
        assert_eq!(
            extract_summary_from_responses(&json!({
                "status": "incomplete",
                "output": [{"type": "message", "content": [{"type": "output_text", "text": "partial"}]}]
            })),
            Err("compaction_summary_truncated")
        );
        assert_eq!(
            extract_summary_from_chat(&json!({
                "choices": [{"message": {"content": "partial"}, "finish_reason": "length"}]
            })),
            Err("compaction_summary_truncated")
        );
        assert_eq!(
            extract_summary_from_chat(&json!({
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}]
            })),
            Ok("ok".to_string())
        );
    }

    #[tokio::test]
    async fn summarizer_sse_is_synthetic_compaction_events_not_token_deltas() {
        let response = compaction_client_response("grok-4.6", "summary text", true);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("event: response.created"));
        assert!(text.contains("event: response.output_item.added"));
        assert!(text.contains("event: response.output_item.done"));
        assert!(text.contains("event: response.completed"));
        assert!(text.contains(ENVELOPE_PREFIX));
        assert!(!text.contains("output_text.delta"));
        assert!(!text.contains("response.output_text"));
    }

    #[test]
    fn prepare_summarizer_caps_high_reasoning_and_keeps_low() {
        let high = json!({
            "reasoning": {"effort": "xhigh"},
            "reasoning_effort": "high",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                {"type": "compaction_trigger"}
            ]
        });
        let prepared = prepare_summarizer_request(high.as_object().unwrap()).unwrap();
        assert_eq!(prepared["stream"], true);
        assert_eq!(prepared["reasoning"]["effort"], "medium");
        assert_eq!(prepared["reasoning_effort"], "medium");

        let low = json!({
            "reasoning": {"effort": "low"},
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                {"type": "compaction_trigger"}
            ]
        });
        let kept = prepare_summarizer_request(low.as_object().unwrap()).unwrap();
        assert_eq!(kept["reasoning"]["effort"], "low");
    }

    #[test]
    fn chat_adapter_accepts_expanded_compaction_history() {
        let envelope = encode_envelope("prior summary");
        let prepared_request = json!({
            "model": "grok-4.6",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "continue"}]},
                {"type": "compaction", "encrypted_content": envelope},
                {"type": "compaction_trigger"}
            ]
        });
        let prepared = prepare_summarizer_request(prepared_request.as_object().unwrap()).unwrap();
        let chat = response_request_to_chat(&Value::Object(prepared)).unwrap();
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "continue");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "prior summary");
        assert_eq!(chat["stream"], true);
    }
}
