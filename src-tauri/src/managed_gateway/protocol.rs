//! Responses/Chat 请求改写、能力预检和用量形状。
//!
//! 不发送上游请求，不管理 sidecar。GPT 族保留 Codex Responses 请求体。
//! 非 GPT 才按 CapabilitySet 删除或改写中继不兼容字段，不伪造工具/图片兼容。
//! HTTP 入口和转发在打上游之前调用。

use axum::body::Bytes;
use serde_json::{json, Map, Value};

use crate::diagnostics::RequestDiagnosticOutcome;
use crate::routing::{looks_like_gpt_model_id, CompactMode, ModelRoute};

use super::compaction;
use super::SUBTASK_FAILURE_RECOVERY_NOTICE;

/// Relay-compat rewrites exist for non-GPT Responses providers.
///
/// GPT-family models keep the Codex Responses body. Routing still rewrites the
/// public model id and injects this channel's API Key. Family is
/// `looks_like_gpt_model_id`: a Grok/DeepSeek/Claude upstream stays non-GPT
/// even if the public slug contains `gpt`.
pub(super) fn needs_relay_compat(route: &ModelRoute) -> bool {
    !looks_like_gpt_model_id(&route.public_model_id, &route.upstream_model_id)
}

pub(super) fn filter_unsupported_optional_tools(
    route: &ModelRoute,
    request: &mut serde_json::Map<String, Value>,
) {
    if !route.capabilities.supports_tools {
        request.remove("tools");
        request.remove("tool_choice");
        request.remove("parallel_tool_calls");
        return;
    }
    if !route.capabilities.supports_web_search {
        if let Some(Value::Array(tools)) = request.get_mut("tools") {
            tools.retain(|tool| {
                !tool
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind.contains("web_search"))
            });
        }
    }
    if request
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        request.remove("tools");
        request.remove("tool_choice");
        request.remove("parallel_tool_calls");
    }
}

/// Some OpenAI-compatible relays reject the otherwise-standard Responses
/// `metadata` member with HTTP 400. Codex does not depend on this field for
/// routing or conversation state, so omit it at the managed boundary to keep
/// non-GPT Responses providers interoperable. Request diagnostics retain
/// their own redacted metadata separately. GPT-family routes skip this filter.
pub(super) fn filter_relay_incompatible_fields(request: &mut serde_json::Map<String, Value>) {
    request.remove("metadata");
}

/// Normalize Codex's private multi-agent history for non-GPT Responses
/// providers. The local Codex client owns delegation and receives ordinary
/// Responses output unchanged; this only makes its replayed child messages
/// representable on the public wire. GPT-family routes keep `agent_message`.
pub(super) fn normalize_routed_agent_messages(
    request: &mut Map<String, Value>,
) -> Result<(), &'static str> {
    let Some(input) = request.get("input").and_then(Value::as_array) else {
        return Ok(());
    };
    let mut rewritten = Vec::with_capacity(input.len());
    let mut changed = false;
    for item in input {
        let Some(object) = item.as_object() else {
            rewritten.push(item.clone());
            continue;
        };
        if object.get("type").and_then(Value::as_str) != Some("agent_message") {
            let mut item = object.clone();
            changed |= item
                .remove("internal_chat_message_metadata_passthrough")
                .is_some();
            rewritten.push(Value::Object(item));
            continue;
        }
        rewritten.push(normalize_routed_agent_message(object)?);
        changed = true;
    }
    if changed {
        request.insert("input".to_string(), Value::Array(rewritten));
    }
    Ok(())
}

pub(super) fn normalize_routed_agent_message(
    item: &Map<String, Value>,
) -> Result<Value, &'static str> {
    let Some(content) = item.get("content").and_then(Value::as_array) else {
        return Err("unreadable_agent_message");
    };
    if content.is_empty() {
        return Err("unreadable_agent_message");
    }
    let mut normalized = Vec::with_capacity(content.len() + 1);
    if agent_message_reports_task_failure(content) {
        normalized.push(json!({
            "type": "input_text",
            "text": SUBTASK_FAILURE_RECOVERY_NOTICE
        }));
    }
    let identities = ["author", "recipient"]
        .into_iter()
        .filter_map(|key| {
            item.get(key)
                .and_then(Value::as_str)
                .map(|value| (key, value))
        })
        .collect::<Vec<_>>();
    if !identities.is_empty() {
        normalized.push(json!({
            "type": "input_text",
            "text": format!(
                "Agent message {}",
                serde_json::to_string(&identities).expect("string pairs serialize")
            )
        }));
    }
    for part in content {
        let Some(part) = part.as_object() else {
            return Err("unreadable_agent_message");
        };
        match part.get("type").and_then(Value::as_str) {
            Some("input_text") => {
                if !part.get("text").is_some_and(Value::is_string) {
                    return Err("unreadable_agent_message");
                }
                normalized.push(json!({
                    "type": "input_text",
                    "text": part["text"]
                }));
            }
            Some("encrypted_content") => {
                let Some(text) = part.get("encrypted_content").and_then(Value::as_str) else {
                    return Err("unreadable_agent_message");
                };
                if text.trim().is_empty() || is_opaque_agent_message_content(text) {
                    return Err("unreadable_agent_message");
                }
                normalized.push(json!({"type": "input_text", "text": text}));
            }
            _ => return Err("unreadable_agent_message"),
        }
    }
    Ok(json!({
        "type": "message",
        "role": "user",
        "content": normalized
    }))
}

pub(super) fn agent_message_reports_task_failure(content: &[Value]) -> bool {
    content.iter().any(|part| {
        part.get("type").and_then(Value::as_str) == Some("input_text")
            && part
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("Agent errored:"))
    })
}

pub(super) fn is_opaque_agent_message_content(content: &str) -> bool {
    [
        compaction::ENVELOPE_PREFIX,
        "ocx1:",
        "cc-switch-compaction-v1:",
        "openai-compaction-v1:",
        // Fernet tokens begin with this version/timestamp prefix. Treating one
        // as model-visible text would expose unusable opaque state.
        "gAAAA",
    ]
    .iter()
    .any(|prefix| content.starts_with(prefix))
}

pub(super) fn preflight_capabilities(
    route: &ModelRoute,
    compact_mode: CompactMode,
    request: &Value,
) -> Result<(), &'static str> {
    // Isolated / local_client launches never emit a remote compact contract.
    // Default-App remote_v2 dispatches by route capability: native passthrough,
    // GatewaySummarizer, or fail-closed fake gpt-*. Official routes keep their
    // native preflight and still hit official_route_unavailable after this.
    if request_contains_compaction_trigger(request) {
        compaction::preflight_compact(route, compact_mode)?;
    }
    if !needs_relay_compat(route) {
        return Ok(());
    }
    if request
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty())
        && !route.capabilities.supports_tools
    {
        return Err("unsupported_tools");
    }
    if !route.capabilities.supports_web_search
        && request
            .get("tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| {
                tools.iter().any(|tool| {
                    tool.get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| kind.contains("web_search"))
                })
            })
    {
        return Err("unsupported_web_search");
    }
    if request_contains_image(request) && !route.capabilities.supports_images {
        return Err("unsupported_images");
    }
    Ok(())
}

pub(super) fn request_contains_image(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(request_contains_image),
        Value::Object(object) => {
            object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "input_image" | "image_url"))
                || object.values().any(request_contains_image)
        }
        _ => false,
    }
}

pub(super) fn request_contains_compaction_trigger(request: &Value) -> bool {
    request
        .get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind == "compaction_trigger")
            })
        })
}

pub(super) fn response_request_to_chat(input: &Value) -> Result<Value, &'static str> {
    if input
        .get("previous_response_id")
        .is_some_and(|value| !value.is_null())
        || input
            .get("mcp_servers")
            .is_some_and(|value| !value.is_null())
    {
        return Err("unsupported_chat_capability");
    }
    let model = input
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("model_required")?;
    let mut messages = Vec::new();
    if let Some(instructions) = input
        .get("instructions")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        messages.push(json!({"role": "system", "content": instructions}));
    }
    match input.get("input") {
        Some(Value::String(text)) if !text.is_empty() => {
            messages.push(json!({"role": "user", "content": text}));
        }
        Some(Value::Array(items)) => {
            for item in items {
                // 文本适配器没有工具/推理项目转换合同，拒绝比静默丢掉历史更可解释。
                if item
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind != "message")
                {
                    return Err("unsupported_chat_history_item");
                }
                let role = item
                    .get("role")
                    .and_then(Value::as_str)
                    .ok_or("unsupported_chat_history_item")?;
                let content = chat_text_content(item.get("content"))?;
                if content.is_empty() {
                    continue;
                }
                let role = match role {
                    "developer" => "system",
                    "system" | "user" | "assistant" => role,
                    _ => return Err("unsupported_chat_history_item"),
                };
                messages.push(json!({"role": role, "content": content}));
            }
        }
        _ => return Err("unsupported_chat_input"),
    }
    if messages.is_empty() {
        return Err("unsupported_chat_input");
    }
    let mut chat = serde_json::Map::from_iter([
        ("model".to_string(), Value::String(model.to_string())),
        ("messages".to_string(), Value::Array(messages)),
        (
            "stream".to_string(),
            Value::Bool(
                input
                    .get("stream")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        ),
    ]);
    if chat.get("stream") == Some(&Value::Bool(true)) {
        chat.insert("stream_options".to_string(), json!({"include_usage": true}));
    }
    if let Some(max_tokens) = input.get("max_output_tokens").and_then(Value::as_u64) {
        chat.insert("max_tokens".to_string(), Value::Number(max_tokens.into()));
    }
    if let Some(temperature) = input.get("temperature").and_then(Value::as_f64) {
        if let Some(number) = serde_json::Number::from_f64(temperature) {
            chat.insert("temperature".to_string(), Value::Number(number));
        }
    }
    Ok(Value::Object(chat))
}

pub(super) fn chat_text_content(content: Option<&Value>) -> Result<String, &'static str> {
    match content {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(parts)) => {
            let mut text = String::new();
            for part in parts {
                let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
                if matches!(kind, "input_text" | "output_text" | "text") {
                    if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(part_text);
                    }
                } else {
                    return Err("unsupported_chat_input");
                }
            }
            Ok(text)
        }
        None | Some(Value::Null) => Ok(String::new()),
        _ => Err("unsupported_chat_input"),
    }
}

pub(super) fn rewrite_model_fields(value: &mut Value, upstream_model: &str, public_model: &str) {
    // model 只有在协议拥有的路径上才是路由名称；工具输出里的同名业务字段不能改。
    fn rewrite(object: &mut Value, upstream: &str, public: &str) {
        if object.get("model").and_then(Value::as_str) == Some(upstream) {
            object["model"] = Value::String(public.to_string());
        }
    }
    rewrite(value, upstream_model, public_model);
    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.starts_with("response."))
    {
        if let Some(response) = value.get_mut("response") {
            rewrite(response, upstream_model, public_model);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct RequestUsage {
    pub(super) input_tokens: Option<i64>,
    pub(super) output_tokens: Option<i64>,
    pub(super) total_tokens: Option<i64>,
}

pub(super) fn usage_from_value(value: &Value) -> Option<RequestUsage> {
    let usage = value.get("usage")?;
    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_i64);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_i64);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            input_tokens
                .and_then(|input| output_tokens.and_then(|output| input.checked_add(output)))
        });
    if input_tokens.is_none() && output_tokens.is_none() && total_tokens.is_none() {
        None
    } else {
        Some(RequestUsage {
            input_tokens,
            output_tokens,
            total_tokens,
        })
    }
}

pub(super) fn usage_from_event_value(value: &Value) -> Option<RequestUsage> {
    usage_from_value(value).or_else(|| value.get("response").and_then(usage_from_value))
}

pub(super) fn is_event_stream_content_type(content_type: Option<&str>) -> bool {
    content_type
        .map(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
        .unwrap_or(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProtocolTerminal {
    Completed,
    Failed,
    Incomplete,
}

impl ProtocolTerminal {
    pub(super) fn diagnostic(self) -> (RequestDiagnosticOutcome, Option<&'static str>) {
        match self {
            Self::Completed => (RequestDiagnosticOutcome::Success, None),
            Self::Failed => (
                RequestDiagnosticOutcome::Failed,
                Some("upstream_response_failed"),
            ),
            Self::Incomplete => (
                RequestDiagnosticOutcome::Interrupted,
                Some("upstream_response_incomplete"),
            ),
        }
    }
    pub(super) fn event_name(self) -> &'static str {
        match self {
            Self::Completed => "response.completed",
            Self::Failed => "response.failed",
            Self::Incomplete => "response.incomplete",
        }
    }
    pub(super) fn status(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Incomplete => "incomplete",
        }
    }
}

pub(super) fn validate_response(value: &Value) -> Result<ProtocolTerminal, &'static str> {
    if !value.is_object()
        || value.get("object").and_then(Value::as_str) != Some("response")
        || value
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !value.get("output").is_some_and(Value::is_array)
    {
        return Err("invalid_upstream_response");
    }
    match value.get("status").and_then(Value::as_str) {
        Some("completed") if value.get("error").is_none_or(Value::is_null) => {
            Ok(ProtocolTerminal::Completed)
        }
        Some("failed") => Ok(ProtocolTerminal::Failed),
        Some("incomplete") => Ok(ProtocolTerminal::Incomplete),
        _ => Err("invalid_upstream_response_status"),
    }
}

pub(super) fn chat_finish_reason(reason: &str) -> Result<ProtocolTerminal, &'static str> {
    match reason {
        "stop" => Ok(ProtocolTerminal::Completed),
        "length" | "content_filter" => Ok(ProtocolTerminal::Incomplete),
        "tool_calls" | "function_call" => Err("unsupported_chat_output"),
        _ => Err("invalid_upstream_finish_reason"),
    }
}

pub(super) fn chat_usage_as_responses(value: Option<&Value>) -> Value {
    let Some(value) = value.filter(|value| value.is_object()) else {
        return Value::Null;
    };
    // 保留渠道扩展用量字段，只重命名 Chat 已定义的计数；未知值绝不补 0。
    let mut output = value.clone();
    let object = output.as_object_mut().unwrap();
    for (from, to) in [
        ("prompt_tokens", "input_tokens"),
        ("completion_tokens", "output_tokens"),
        ("prompt_tokens_details", "input_tokens_details"),
        ("completion_tokens_details", "output_tokens_details"),
    ] {
        if let Some(value) = object.remove(from) {
            object.insert(to.to_string(), value);
        }
    }
    output
}

// 同步回退只支持文本消息。多条消息/多段文字逐项生成；工具、推理等没有
// 明确增量合同的输出直接报错，避免伪造一个与 completed.output 不一致的消息。
pub(super) fn response_value_as_sse(value: &Value) -> Result<Vec<Bytes>, &'static str> {
    let terminal = validate_response(value)?;
    let items = value["output"].as_array().unwrap();
    for item in items {
        if item.get("type").and_then(Value::as_str) != Some("message")
            || item.get("role").and_then(Value::as_str) != Some("assistant")
            || item
                .get("id")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            return Err("unsupported_sync_response_output");
        }
        let parts = item
            .get("content")
            .and_then(Value::as_array)
            .ok_or("unsupported_sync_response_output")?;
        for part in parts {
            if part.get("type").and_then(Value::as_str) != Some("output_text")
                || !part.get("text").is_some_and(Value::is_string)
            {
                return Err("unsupported_sync_response_output");
            }
        }
    }
    let mut sequence = 0u64;
    let mut events = Vec::new();
    let mut emit = |name: &str, mut data: Value| {
        data["type"] = json!(name);
        data["sequence_number"] = json!(sequence);
        sequence += 1;
        events.push(sse_event(name, data));
    };
    let mut created = value.clone();
    created["status"] = json!("in_progress");
    created["output"] = json!([]);
    created["usage"] = Value::Null;
    created["error"] = Value::Null;
    created["incomplete_details"] = Value::Null;
    emit("response.created", json!({"response":created}));
    for (output_index, item) in items.iter().enumerate() {
        let mut added = item.clone();
        added["status"] = json!("in_progress");
        added["content"] = json!([]);
        emit(
            "response.output_item.added",
            json!({"output_index":output_index,"item":added}),
        );
        for (content_index, part) in item["content"].as_array().unwrap().iter().enumerate() {
            let mut added = part.clone();
            added["text"] = json!("");
            emit(
                "response.content_part.added",
                json!({"item_id":item["id"],"output_index":output_index,"content_index":content_index,"part":added}),
            );
            emit(
                "response.output_text.delta",
                json!({"item_id":item["id"],"output_index":output_index,"content_index":content_index,"delta":part["text"]}),
            );
            emit(
                "response.output_text.done",
                json!({"item_id":item["id"],"output_index":output_index,"content_index":content_index,"text":part["text"],"logprobs":part.get("logprobs").cloned().unwrap_or(json!([]))}),
            );
            emit(
                "response.content_part.done",
                json!({"item_id":item["id"],"output_index":output_index,"content_index":content_index,"part":part}),
            );
        }
        emit(
            "response.output_item.done",
            json!({"output_index":output_index,"item":item}),
        );
    }
    emit(terminal.event_name(), json!({"response":value}));
    Ok(events)
}

pub(super) fn response_json(response_id: &str, item_id: &str, model: &str, text: &str) -> Value {
    json!({
        "id": response_id,
        "object": "response",
        "created_at": 1,
        "model": model,
        "status": "completed",
        "error": null,
        "incomplete_details": null,
        "instructions": null,
        "max_output_tokens": null,
        "output": [{
            "id": item_id,
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{"type": "output_text", "text": text, "annotations": [], "logprobs": []}]
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

pub(super) fn response_in_progress_json(response_id: &str, model: &str) -> Value {
    json!({
        "id": response_id,
        "object": "response",
        "created_at": 1,
        "model": model,
        "status": "in_progress",
        "error": null,
        "incomplete_details": null,
        "instructions": null,
        "max_output_tokens": null,
        "output": [],
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

pub(super) fn sse_event(name: &str, payload: Value) -> Bytes {
    Bytes::from(format!("event: {name}\ndata: {payload}\n\n"))
}
