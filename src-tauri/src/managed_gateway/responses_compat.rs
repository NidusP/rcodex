//! Ordered rewrite for third-party Responses requests.
//!
//! HTTP calls prepare_responses_relay once, before the upstream send. GPT
//! requests leave this pipeline before any field changes. Chat Completions
//! still pass through the optional-tool and metadata steps when the route is
//! not GPT, because those fields are not Responses-only. Agent-message
//! normalization stays Responses-only.
//!
//! Chat function tools and apply_patch wrapping are not implemented here.
//! They belong in chat_bridge when that conversion starts.
//!
//! Each step keeps a field, rewrites it, or rejects locally with an existing
//! error code. This module does not send the upstream request.

use serde_json::{json, Map, Value};

use crate::routing::looks_like_gpt_model_id;
use crate::routing::{CompactMode, ModelRoute, RouteSource, WireApi};

use super::compaction;
use super::protocol::request_contains_compaction_trigger;
use super::SUBTASK_FAILURE_RECOVERY_NOTICE;

/// Relay-compat rewrites exist for non-GPT Responses providers.
///
/// GPT-family models keep the Codex Responses body. Routing still rewrites the
/// public model id and injects this channel's API Key. Family is
/// `looks_like_gpt_model_id`: a non-GPT upstream stays on this path
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

/// Prepare one routed request before model-id replacement and upstream send.
///
/// Order matches the previous two call sites: optional-tool filtering, metadata
/// removal, capability preflight, then Responses agent-message normalization.
/// Official routes do not normalize agent messages; they still fail closed as
/// official_route_unavailable.
pub(super) fn prepare_responses_relay(
    route: &ModelRoute,
    compact_mode: CompactMode,
    request: &mut Map<String, Value>,
) -> Result<(), &'static str> {
    if needs_relay_compat(route) {
        filter_unsupported_optional_tools(route, request);
        filter_relay_incompatible_fields(request);
    }
    preflight_capabilities(route, compact_mode, &Value::Object(request.clone()))?;
    let normalize_agents = needs_relay_compat(route)
        && matches!(route.source, RouteSource::ApiProvider { .. })
        && route.wire_api == WireApi::Responses
        && !request_contains_compaction_trigger(&Value::Object(request.clone()));
    if normalize_agents {
        normalize_routed_agent_messages(request)?;
    }
    Ok(())
}
