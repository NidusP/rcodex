//! Shared response shapes: usage, terminal status, and SSE framing.
//!
//! Non-GPT Responses rewriting lives in responses_compat. The text-only
//! Responses-to-Chat bridge lives in chat_bridge. Neither conversion belongs
//! in this file. This module does not send upstream requests.

use axum::body::Bytes;
use serde_json::{json, Map, Value};

use crate::diagnostics::RequestDiagnosticOutcome;

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
