//! Gateway 错误响应、脱敏详情和 Codex 可见文案。
//!
//! 不转发上游、不改 RouteSet。只把本地/上游失败收成带 gateway_code 的 JSON。
//! HTTP 入口、转发和压缩在失败路径调用。

use crate::conversation_archive::RequestCapture;
use axum::body::Bytes;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use reqwest::Response as UpstreamResponse;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::timeout;

use crate::diagnostics::{RequestDiagnosticInput, RequestDiagnosticOutcome};

use super::diagnostics::{record_request_best_effort, RequestRecorder};
use super::helpers::{gateway_trace, safe_upstream_url};

pub(super) fn classify_upstream_send(error: &reqwest::Error) -> (&'static str, StatusCode) {
    if error.is_connect() && error.is_timeout() {
        ("upstream_connect_timeout", StatusCode::GATEWAY_TIMEOUT)
    } else if error.is_timeout() {
        ("upstream_first_byte_timeout", StatusCode::GATEWAY_TIMEOUT)
    } else if error.is_connect() {
        ("upstream_connect_failed", StatusCode::BAD_GATEWAY)
    } else if error.is_request() {
        ("upstream_request_failed", StatusCode::BAD_GATEWAY)
    } else {
        ("upstream_unavailable", StatusCode::BAD_GATEWAY)
    }
}

pub(super) fn annotate_upstream_detail(url: &str, detail: Option<String>) -> Option<String> {
    let url = safe_upstream_url(url);
    Some(sanitize_error_detail(match detail {
        Some(detail) if !detail.is_empty() => format!("{url} · {detail}"),
        _ => url,
    }))
}

pub(super) fn reject_local(
    recorder: &Option<RequestRecorder>,
    instance_id: &str,
    public_model: &str,
    code: &'static str,
    status: StatusCode,
) -> Response {
    gateway_trace(&[
        ("event", "local_reject"),
        ("instance", instance_id),
        ("public_model", public_model),
        ("code", code),
    ]);
    record_request_best_effort(
        recorder,
        local_failure_event(
            Some(instance_id.to_string()),
            public_model,
            code,
            status.as_u16(),
        ),
    );
    api_error(status, code)
}

pub(super) fn local_failure_event(
    instance_id: Option<String>,
    public_model_id: &str,
    code: &str,
    http_status: u16,
) -> RequestDiagnosticInput {
    RequestDiagnosticInput {
        instance_id,
        session_id: None,
        route: "/v1/responses".into(),
        stream: false,
        wire_api: "responses",
        public_model_id: public_model_id.to_string(),
        upstream_model_id: public_model_id.to_string(),
        provider_id: "gateway".into(),
        provider_revision_id: "local".into(),
        http_status: Some(http_status),
        response_content_type: None,
        first_byte_ms: None,
        total_latency_ms: None,
        outcome: RequestDiagnosticOutcome::Failed,
        error_code: Some(code.to_string()),
        error_detail: None,
        input_length: 0,
        output_length: 0,
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        input_hash: None,
        output_hash: None,
        tool_call_types: Vec::new(),
        tool_result_hashes: Vec::new(),
    }
}

pub(super) async fn not_found() -> Response {
    api_error(StatusCode::NOT_FOUND, "not_found")
}

pub(super) fn api_error(status: StatusCode, code: &'static str) -> Response {
    api_error_with_detail(status, code, None, None)
}

pub(super) async fn bounded_response_bytes(
    response: UpstreamResponse,
    limit: usize,
    capture: Option<&RequestCapture>,
) -> Result<Bytes, &'static str> {
    let mut output = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "upstream_response_read_failed")?;
        if let Some(capture) = capture {
            capture.stage("upstream-response", &chunk);
        }
        if output.len().saturating_add(chunk.len()) > limit {
            return Err("upstream_response_too_large");
        }
        output.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(output))
}

pub(super) async fn upstream_error_detail(
    response: UpstreamResponse,
    capture: Option<&RequestCapture>,
) -> Option<String> {
    let bytes = timeout(Duration::from_secs(2), async {
        let mut stream = response.bytes_stream();
        let mut sample = Vec::with_capacity(4096);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.ok()?;
            if let Some(capture) = capture {
                capture.stage("upstream-response", &chunk);
            }
            let remaining = 4096usize.saturating_sub(sample.len());
            sample.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }
        Some(sample)
    })
    .await
    .ok()??;
    if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
        let error = value.get("error").unwrap_or(&value);
        let code = error.get("code").and_then(Value::as_str);
        let message = error.get("message").and_then(Value::as_str);
        if let Some(detail) = match (code, message) {
            (Some(code), Some(message)) => Some(format!("{code}: {message}")),
            (None, Some(message)) => Some(message.to_string()),
            _ => None,
        } {
            return Some(sanitize_error_detail(detail));
        }
    }
    // Some relays return a plain-text 502 (often literally "Unknown error").
    // Preserve the bounded body so the App can distinguish an upstream failure
    // from a local admission or route-set failure.
    std::str::from_utf8(&bytes)
        .ok()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| sanitize_error_detail(text.to_string()))
}

pub(super) fn sanitize_error_detail(value: String) -> String {
    let mut safe = value
        .chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .collect::<String>();
    for marker in [
        "Bearer ",
        "Authorization:",
        "OPENAI_API_KEY=",
        "CODEX_API_KEY=",
        "THREER_INSTANCE_KEY=",
        "RCODEX_INSTANCE_KEY=",
        "api_key=",
        "api-key=",
    ] {
        if let Some(index) = safe.find(marker) {
            safe.truncate(index);
            safe.push_str("[redacted]");
        }
    }
    safe.chars().take(1024).collect()
}

pub(super) fn api_error_with_detail(
    status: StatusCode,
    code: &'static str,
    url: Option<&str>,
    detail: Option<String>,
) -> Response {
    let layer = error_layer(code);
    let safe_url = url.map(safe_upstream_url);
    let message = codex_visible_error_message(code, layer, safe_url.as_deref(), detail.as_deref());
    let error_type = match layer {
        "upstream" => "upstream_error",
        "transport" => "transport_error",
        _ => "gateway_error",
    };
    // Codex prints `unexpected status …: {error.message}, url: <Codex's request URL>`.
    // That trailing url is always the loopback Gateway; the real upstream is in message.
    let mut error = json!({
        "message": message,
        "type": error_type,
        "code": code,
        "gateway_code": code,
        "layer": layer
    });
    if layer == "upstream" {
        error["upstream_status"] = json!(status.as_u16());
    }
    if let Some(url) = safe_url {
        error["upstream_url"] = Value::String(url);
    }
    if let Some(detail) = detail {
        error["upstream_detail"] = Value::String(detail);
    }
    (
        status,
        Json(json!({
            "error": error,
            "message": message
        })),
    )
        .into_response()
}

pub(super) fn error_layer(code: &str) -> &'static str {
    match code {
        "upstream_error" => "upstream",
        code if code.starts_with("upstream_") => "transport",
        "invalid_upstream_response" => "gateway",
        _ => "gateway",
    }
}

/// Body text Codex interpolates after `unexpected status 502 Bad Gateway:`.
/// Empty/whitespace bodies become `Unknown error` in Codex; this must stay non-empty.
pub(super) fn codex_visible_error_message(
    code: &str,
    layer: &str,
    url: Option<&str>,
    detail: Option<&str>,
) -> String {
    let local = match code {
        "invalid_instance_key" => "实例密钥无效。",
        "invalid_request" => "请求体不是合法 JSON 对象。",
        "model_required" => "缺少 model。",
        "model_not_in_route_set" => "该模型不在当前 RouteSet 中。",
        "official_authorization_missing" => "官方渠道缺少 Authorization。",
        "official_route_unavailable" => "官方渠道路由当前不可用。",
        "provider_revision_unavailable" => "渠道版本不可用。",
        "provider_revision_mismatch" => "渠道版本与绑定不一致。",
        "unsupported_tools" => "该路由不支持 tools。",
        "unsupported_web_search" => "该路由不支持 web_search。",
        "unsupported_images" => "该路由不支持图片输入。",
        "unreadable_agent_message" => {
            "子任务转发失败：任务消息含有当前第三方渠道无法安全转发的私有或加密内容。父任务可自行继续处理，或改用新的子任务。"
        }
        "unsupported_compaction" => {
            "第三方模型不接受 Codex 远程压缩请求，请新建会话或切回支持该合同的官方模型。"
        }
        "not_native_gpt_compaction" => {
            "当前模型不是具备官方远程压缩合同的 GPT，不能把 compact 请求发给中继。请换真实官方模型、改用隔离环境走本地压缩，或使用非 GPT 名称的三方模型。"
        }
        "incompatible_compaction_envelope" => "会话含有无法在当前渠道解开的压缩信封，请新建会话。",
        "incompatible_reasoning_envelope" => {
            "会话含有无法在当前压缩路径展开的推理密文，请新建会话或改用隔离环境。"
        }
        "compaction_summary_empty" => {
            "当前渠道返回了空摘要，远程压缩无法续接，请新建会话或改用隔离环境。"
        }
        "compaction_summary_truncated" => {
            "当前渠道的压缩摘要被截断，远程压缩无法续接，请新建会话或改用隔离环境。"
        }
        "compaction_summary_failed" => "当前渠道的压缩摘要失败，远程压缩无法续接，请新建会话。",
        "unsupported_chat_capability" => "该 chat 适配不支持请求中的能力。",
        "unsupported_chat_input" => "无法把该 Responses 请求适配为 chat/completions。",
        "unsupported_chat_history_item" => "无法把该历史项适配为 chat/completions。",
        "unsupported_chat_output" => "上游 chat 响应含有无法用于压缩摘要的输出。",
        "serialization_failed" => "无法序列化发往上游的请求。",
        "not_found" => "没有匹配的 Gateway 路径。",
        "upstream_connect_timeout" => "连接上游超时。",
        "upstream_first_byte_timeout" => "等待上游首字节超时。",
        "upstream_connect_failed" => "无法连接上游。",
        "upstream_request_failed" => "发往上游的请求失败。",
        "upstream_unavailable" => "上游不可达。",
        "upstream_response_timeout" => "读取上游响应超时。",
        "upstream_interrupted" => "上游响应中断。",
        "invalid_upstream_response" => "上游响应无法按协议解析。",
        "upstream_error" => "上游返回了失败响应。",
        _ => "请求失败。",
    };
    let loopback_note = format!(
        "Codex 打印的 url 是本机 Gateway（127.0.0.1:{}），不是上游。",
        super::gateway_listen_port()
    );
    match (layer, url, detail.filter(|value| !value.is_empty())) {
        ("upstream", Some(url), Some(detail)) => {
            format!("R Codex Gateway 已转发到 {url}，上游失败：{detail}。{loopback_note}")
        }
        ("upstream", Some(url), None) => {
            format!("R Codex Gateway 已转发到 {url}，{local}{loopback_note}")
        }
        ("transport", Some(url), Some(detail)) => {
            format!("R Codex Gateway 无法转发到 {url}：{detail}。{loopback_note}")
        }
        ("transport", Some(url), None) => {
            format!("R Codex Gateway 无法转发到 {url}：{local}{loopback_note}")
        }
        (_, Some(url), Some(detail)) => {
            format!("R Codex Gateway {local}上游 {url}：{detail} {loopback_note}")
        }
        (_, Some(url), None) => format!("R Codex Gateway {local}目标 {url}。{loopback_note}"),
        (_, None, Some(detail)) => format!("R Codex Gateway {local}{detail} {loopback_note}"),
        _ => format!("R Codex Gateway {local}{loopback_note}"),
    }
}
