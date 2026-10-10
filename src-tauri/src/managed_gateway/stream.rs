//! SSE 改写和 Chat 流到 Responses 事件的转换。
//!
//! 不解析完整 JSON 文档以外的私有字段，不记录正文。字节分帧交给 `sse.rs`。
//! 转发模块在上游开始流式返回后调用。

use std::collections::VecDeque;
use std::pin::Pin;
use std::time::{Duration, Instant};

use axum::{
    body::{Body, Bytes},
    http::{header::CONTENT_TYPE, StatusCode},
    response::Response,
};
use futures_util::stream::Stream;
use futures_util::StreamExt;
use reqwest::Response as UpstreamResponse;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::time::timeout;
use uuid::Uuid;

use crate::diagnostics::RequestDiagnosticOutcome;
use crate::routing::{GatewayTimeoutSnapshot, ModelRoute};

use super::diagnostics::{
    digest_hasher, record_request_best_effort, request_event, RequestMeta, RequestRecorder,
};
use super::errors::{api_error, bounded_response_bytes};
use super::protocol::{
    chat_finish_reason, chat_usage_as_responses, response_in_progress_json, response_json,
    response_value_as_sse, rewrite_model_fields, sse_event, usage_from_event_value,
    validate_response, ProtocolTerminal,
};
use super::sse::SseDecoder;
use super::{MAX_RESPONSE_BYTES, MAX_SSE_FRAME_BYTES};

pub(super) fn seeded_hasher() -> Sha256 {
    let mut hasher = Sha256::new();
    hasher.update(b"rcodex-diagnostic-v1\0");
    hasher
}

pub(super) fn record_stream_event(
    state: &mut SseRewriteState,
    outcome: RequestDiagnosticOutcome,
    error: Option<&str>,
) {
    if state.recorded {
        return;
    }
    let elapsed = state.started.elapsed().as_millis() as i64;
    let mut event = request_event(
        &state.input_meta,
        state.instance_id.clone(),
        &state.route,
        "responses",
        state.status,
        state.content_type.as_deref(),
        state.first_byte_ms,
        Some(elapsed),
        outcome,
        error,
        state.output_len,
        None,
    );
    event.stream = true;
    event.output_hash = Some(digest_hasher(&state.output_hasher));
    event.input_tokens = state.input_tokens;
    event.output_tokens = state.output_tokens;
    event.total_tokens = state.total_tokens;
    record_request_best_effort(&state.recorder, event);
    state.recorded = true;
}

pub(super) async fn synchronous_response_as_sse(
    response: UpstreamResponse,
    route: &ModelRoute,
    timeouts: &GatewayTimeoutSnapshot,
    instance_id: Option<String>,
    recorder: Option<RequestRecorder>,
    input_meta: RequestMeta,
    status: StatusCode,
    content_type: Option<String>,
    started: Instant,
) -> Response {
    let bytes = match timeout(
        Duration::from_secs(timeouts.non_stream_total_seconds),
        bounded_response_bytes(
            response,
            MAX_RESPONSE_BYTES,
            recorder.as_ref().and_then(|r| r.archive.as_ref()),
        ),
    )
    .await
    {
        Err(_) => {
            let mut event = request_event(
                &input_meta,
                instance_id,
                route,
                "responses",
                status.as_u16(),
                content_type.as_deref(),
                None,
                Some(started.elapsed().as_millis() as i64),
                RequestDiagnosticOutcome::Timeout,
                Some("upstream_response_timeout"),
                0,
                None,
            );
            event.stream = true;
            record_request_best_effort(&recorder, event);
            return api_error(StatusCode::GATEWAY_TIMEOUT, "upstream_response_timeout");
        }
        Ok(Err(_)) => {
            let mut event = request_event(
                &input_meta,
                instance_id,
                route,
                "responses",
                status.as_u16(),
                content_type.as_deref(),
                None,
                Some(started.elapsed().as_millis() as i64),
                RequestDiagnosticOutcome::Interrupted,
                Some("upstream_interrupted"),
                0,
                None,
            );
            event.stream = true;
            record_request_best_effort(&recorder, event);
            return api_error(StatusCode::BAD_GATEWAY, "upstream_interrupted");
        }
        Ok(Ok(bytes)) => bytes,
    };
    let mut value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => value,
        Err(_) => {
            let mut event = request_event(
                &input_meta,
                instance_id,
                route,
                "responses",
                status.as_u16(),
                content_type.as_deref(),
                None,
                Some(started.elapsed().as_millis() as i64),
                RequestDiagnosticOutcome::FormatError,
                Some("invalid_upstream_response"),
                bytes.len(),
                Some(&bytes),
            );
            event.stream = true;
            record_request_best_effort(&recorder, event);
            return api_error(StatusCode::BAD_GATEWAY, "invalid_upstream_response");
        }
    };
    rewrite_model_fields(&mut value, &route.upstream_model_id, &route.public_model_id);
    let events = match response_value_as_sse(&value) {
        Ok(events) => events,
        Err(code) => {
            let mut event = request_event(
                &input_meta,
                instance_id,
                route,
                "responses",
                status.as_u16(),
                content_type.as_deref(),
                None,
                Some(started.elapsed().as_millis() as i64),
                RequestDiagnosticOutcome::FormatError,
                Some(code),
                bytes.len(),
                Some(&bytes),
            );
            event.stream = true;
            record_request_best_effort(&recorder, event);
            return api_error(StatusCode::BAD_GATEWAY, code);
        }
    };
    let terminal = validate_response(&value).unwrap();
    let (outcome, code) = terminal.diagnostic();
    let mut event = request_event(
        &input_meta,
        instance_id,
        route,
        "responses",
        status.as_u16(),
        content_type.as_deref(),
        None,
        Some(started.elapsed().as_millis() as i64),
        outcome,
        code,
        bytes.len(),
        Some(&bytes),
    );
    event.stream = true;
    if let Some(usage) = usage_from_event_value(&value) {
        event.input_tokens = usage.input_tokens;
        event.output_tokens = usage.output_tokens;
        event.total_tokens = usage.total_tokens;
    }
    record_request_best_effort(&recorder, event);
    let body = events
        .into_iter()
        .flat_map(|event| event.to_vec())
        .collect::<Vec<_>>();
    let mut output = Response::new(Body::from(body));
    output
        .headers_mut()
        .insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
    output
}

pub(super) fn rewrite_sse_response(
    response: UpstreamResponse,
    upstream_model: String,
    public_model: String,
    idle_timeout: Duration,
    recorder: Option<RequestRecorder>,
    input_meta: RequestMeta,
    instance_id: Option<String>,
    route: &ModelRoute,
    status: u16,
    content_type: Option<String>,
    started: Instant,
) -> Response {
    let state = SseRewriteState {
        upstream: response.bytes_stream().boxed(),
        decoder: SseDecoder::new(MAX_SSE_FRAME_BYTES),
        pending: VecDeque::new(),
        protocol_terminal: None,
        upstream_model,
        public_model,
        idle_timeout,
        recorder,
        input_meta,
        instance_id,
        route: route.clone(),
        status,
        content_type,
        started,
        first_byte_ms: None,
        output_len: 0,
        output_hasher: seeded_hasher(),
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        recorded: false,
    };
    let stream = futures_util::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.pending.pop_front() {
                return Some((Ok::<Bytes, std::io::Error>(event), state));
            }
            if state.recorded {
                return None;
            }
            match timeout(state.idle_timeout, state.upstream.next()).await {
                Err(_) => {
                    record_stream_event(
                        &mut state,
                        RequestDiagnosticOutcome::Timeout,
                        Some("upstream_stream_idle_timeout"),
                    );
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "upstream_stream_idle_timeout",
                        )),
                        state,
                    ));
                }
                Ok(Some(Ok(chunk))) => {
                    // Observe the provider bytes before the SSE decoder or
                    // model-name rewrite changes their shape.
                    if let Some(capture) = state.recorder.as_ref().and_then(|r| r.archive.as_ref())
                    {
                        capture.stage("upstream-response", &chunk);
                    }
                    if state.first_byte_ms.is_none() && !chunk.is_empty() {
                        state.first_byte_ms = Some(state.started.elapsed().as_millis() as i64);
                    }
                    state.output_len = state.output_len.saturating_add(chunk.len());
                    state.output_hasher.update(&chunk);
                    if let Err(code) = state.push_chunk(&chunk) {
                        state.pending.clear();
                        record_stream_event(
                            &mut state,
                            RequestDiagnosticOutcome::FormatError,
                            Some(code),
                        );
                        return Some((
                            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, code)),
                            state,
                        ));
                    }
                }
                Ok(Some(Err(_))) => {
                    record_stream_event(
                        &mut state,
                        RequestDiagnosticOutcome::Interrupted,
                        Some("upstream_interrupted"),
                    );
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "upstream_interrupted",
                        )),
                        state,
                    ));
                }
                Ok(None) => {
                    let terminal = state
                        .decoder
                        .finish()
                        .and_then(|_| state.protocol_terminal.ok_or("upstream_stream_incomplete"));
                    match terminal {
                        Ok(terminal) => {
                            let (outcome, code) = terminal.diagnostic();
                            record_stream_event(&mut state, outcome, code);
                            return None;
                        }
                        Err(code) => {
                            record_stream_event(
                                &mut state,
                                RequestDiagnosticOutcome::Interrupted,
                                Some(code),
                            );
                            return Some((
                                Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, code)),
                                state,
                            ));
                        }
                    }
                }
            }
        }
    });
    let mut response = Response::new(Body::from_stream(stream));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
    response
}

pub(super) struct SseRewriteState {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    decoder: SseDecoder,
    pending: VecDeque<Bytes>,
    protocol_terminal: Option<ProtocolTerminal>,
    upstream_model: String,
    public_model: String,
    idle_timeout: Duration,
    recorder: Option<RequestRecorder>,
    input_meta: RequestMeta,
    instance_id: Option<String>,
    route: ModelRoute,
    status: u16,
    content_type: Option<String>,
    started: Instant,
    first_byte_ms: Option<i64>,
    output_len: usize,
    output_hasher: Sha256,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    recorded: bool,
}

impl SseRewriteState {
    fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), &'static str> {
        for frame in self.decoder.push(chunk)? {
            let Some(data) = frame.data.as_ref() else {
                self.pending.push_back(Bytes::from(frame.raw));
                continue;
            };
            if data == b"[DONE]" {
                // [DONE] 只能结束传输；Responses 成功还必须有 response.completed。
                if self.protocol_terminal.is_none() {
                    return Err("upstream_stream_incomplete");
                }
                self.pending.push_back(Bytes::from(frame.raw));
                continue;
            }
            let mut value: Value =
                serde_json::from_slice(data).map_err(|_| "invalid_upstream_response")?;
            if !value.is_object() {
                return Err("invalid_upstream_response");
            }
            let kind = value
                .get("type")
                .and_then(Value::as_str)
                .or(frame.event.as_deref())
                .unwrap_or("");
            let terminal = match kind {
                "response.completed" | "response.failed" | "response.incomplete" => {
                    let response = value.get("response").ok_or("invalid_upstream_response")?;
                    let terminal = validate_response(response)?;
                    if terminal.event_name() != kind {
                        return Err("invalid_upstream_response_status");
                    }
                    Some(terminal)
                }
                "error" | "response.error" => Some(ProtocolTerminal::Failed),
                _ => None,
            };
            if let Some(terminal) = terminal {
                if self
                    .protocol_terminal
                    .is_some_and(|previous| previous != terminal)
                {
                    return Err("conflicting_upstream_terminal");
                }
                self.protocol_terminal = Some(terminal);
            }
            if let Some(usage) = usage_from_event_value(&value) {
                self.input_tokens = usage.input_tokens;
                self.output_tokens = usage.output_tokens;
                self.total_tokens = usage.total_tokens;
            }
            rewrite_model_fields(&mut value, &self.upstream_model, &self.public_model);
            self.pending.push_back(Bytes::from(frame.with_data(&value)));
        }
        Ok(())
    }
}

impl Drop for SseRewriteState {
    fn drop(&mut self) {
        if !self.recorded {
            record_stream_event(
                self,
                RequestDiagnosticOutcome::Interrupted,
                Some("client_cancelled"),
            );
        }
    }
}

pub(super) fn chat_sse_to_responses(
    response: UpstreamResponse,
    public_model: String,
    idle_timeout: Duration,
    recorder: Option<RequestRecorder>,
    input_meta: RequestMeta,
    instance_id: Option<String>,
    route: &ModelRoute,
    status: u16,
    content_type: Option<String>,
    started: Instant,
) -> Response {
    let response_id = format!("resp_{}", Uuid::new_v4().simple());
    let item_id = format!("msg_{}", Uuid::new_v4().simple());
    let pending = VecDeque::from([
        sse_event(
            "response.created",
            json!({
                "type": "response.created",
                "sequence_number": 0,
                "response": response_in_progress_json(&response_id, &public_model)
            }),
        ),
        sse_event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "sequence_number": 1,
                "output_index": 0,
                "item": {"id": item_id, "type": "message", "status": "in_progress", "role": "assistant", "content": []}
            }),
        ),
        sse_event(
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "sequence_number": 2,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": [], "logprobs": []}
            }),
        ),
    ]);
    let state = ChatSseState {
        upstream: response.bytes_stream().boxed(),
        decoder: SseDecoder::new(MAX_SSE_FRAME_BYTES),
        pending,
        protocol_terminal: None,
        finish_reason: None,
        raw_usage: Value::Null,
        stream_error: Value::Null,
        response_id,
        item_id,
        public_model,
        text: String::new(),
        sequence_number: 3,
        idle_timeout,
        finished: false,
        recorder,
        input_meta,
        instance_id,
        route: route.clone(),
        status,
        content_type,
        started,
        first_byte_ms: None,
        output_len: 0,
        output_hasher: seeded_hasher(),
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        recorded: false,
    };
    let stream = futures_util::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.pending.pop_front() {
                return Some((Ok::<Bytes, std::io::Error>(event), state));
            }
            if state.finished {
                if !state.recorded {
                    let (outcome, code) = state.protocol_terminal.unwrap().diagnostic();
                    record_chat_stream_event(&mut state, outcome, code);
                }
                return None;
            }
            if state.recorded {
                return None;
            }
            match timeout(state.idle_timeout, state.upstream.next()).await {
                Err(_) => {
                    record_chat_stream_event(
                        &mut state,
                        RequestDiagnosticOutcome::Timeout,
                        Some("upstream_stream_idle_timeout"),
                    );
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "upstream_stream_idle_timeout",
                        )),
                        state,
                    ));
                }
                Ok(Some(Ok(chunk))) => {
                    if let Some(capture) = state.recorder.as_ref().and_then(|r| r.archive.as_ref())
                    {
                        capture.stage("upstream-response", &chunk);
                    }
                    if state.first_byte_ms.is_none() {
                        state.first_byte_ms = Some(state.started.elapsed().as_millis() as i64);
                    }
                    state.output_len = state.output_len.saturating_add(chunk.len());
                    state.output_hasher.update(&chunk);
                    if let Err(code) = state.push_chunk(&chunk) {
                        state.pending.clear();
                        record_chat_stream_event(
                            &mut state,
                            RequestDiagnosticOutcome::FormatError,
                            Some(code),
                        );
                        return Some((
                            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, code)),
                            state,
                        ));
                    }
                }
                Ok(Some(Err(_))) => {
                    record_chat_stream_event(
                        &mut state,
                        RequestDiagnosticOutcome::Interrupted,
                        Some("upstream_interrupted"),
                    );
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "upstream_interrupted",
                        )),
                        state,
                    ));
                }
                Ok(None) => {
                    if let Err(code) = state.decoder.finish() {
                        record_chat_stream_event(
                            &mut state,
                            RequestDiagnosticOutcome::FormatError,
                            Some(code),
                        );
                        return Some((
                            Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, code)),
                            state,
                        ));
                    }
                    if state.protocol_terminal.is_none() {
                        record_chat_stream_event(
                            &mut state,
                            RequestDiagnosticOutcome::Interrupted,
                            Some("upstream_stream_incomplete"),
                        );
                        return Some((
                            Err(std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "upstream_stream_incomplete",
                            )),
                            state,
                        ));
                    }
                    state.finish();
                }
            }
        }
    });
    let mut response = Response::new(Body::from_stream(stream));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
    response
}

pub(super) struct ChatSseState {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    decoder: SseDecoder,
    pending: VecDeque<Bytes>,
    protocol_terminal: Option<ProtocolTerminal>,
    finish_reason: Option<String>,
    raw_usage: Value,
    stream_error: Value,
    response_id: String,
    item_id: String,
    public_model: String,
    text: String,
    sequence_number: u64,
    idle_timeout: Duration,
    finished: bool,
    recorder: Option<RequestRecorder>,
    input_meta: RequestMeta,
    instance_id: Option<String>,
    route: ModelRoute,
    status: u16,
    content_type: Option<String>,
    started: Instant,
    first_byte_ms: Option<i64>,
    output_len: usize,
    output_hasher: Sha256,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    recorded: bool,
}

pub(super) fn record_chat_stream_event(
    state: &mut ChatSseState,
    outcome: RequestDiagnosticOutcome,
    error: Option<&str>,
) {
    if state.recorded {
        return;
    }
    let elapsed = state.started.elapsed().as_millis() as i64;
    let mut event = request_event(
        &state.input_meta,
        state.instance_id.clone(),
        &state.route,
        "chat_completions",
        state.status,
        state.content_type.as_deref(),
        state.first_byte_ms,
        Some(elapsed),
        outcome,
        error,
        state.output_len,
        None,
    );
    event.stream = true;
    event.output_hash = Some(digest_hasher(&state.output_hasher));
    event.input_tokens = state.input_tokens;
    event.output_tokens = state.output_tokens;
    event.total_tokens = state.total_tokens;
    record_request_best_effort(&state.recorder, event);
    state.recorded = true;
}

impl Drop for ChatSseState {
    fn drop(&mut self) {
        if !self.recorded {
            record_chat_stream_event(
                self,
                RequestDiagnosticOutcome::Interrupted,
                Some("client_cancelled"),
            );
        }
    }
}

impl ChatSseState {
    fn push_chunk(&mut self, chunk: &Bytes) -> Result<(), &'static str> {
        for frame in self.decoder.push(chunk)? {
            let Some(data) = frame.data.as_ref() else {
                continue;
            };
            if data == b"[DONE]" {
                if self.protocol_terminal.is_none() {
                    return Err("upstream_stream_incomplete");
                }
                continue;
            }
            let value =
                serde_json::from_slice::<Value>(data).map_err(|_| "invalid_upstream_response")?;
            if !value.is_object() {
                return Err("invalid_upstream_response");
            }
            if let Some(usage) = usage_from_event_value(&value) {
                self.input_tokens = usage.input_tokens;
                self.output_tokens = usage.output_tokens;
                self.total_tokens = usage.total_tokens;
                self.raw_usage = chat_usage_as_responses(value.get("usage"));
            }
            if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
                self.protocol_terminal = Some(ProtocolTerminal::Failed);
                self.stream_error = error.clone();
                continue;
            }
            if value
                .pointer("/choices/0/delta/tool_calls")
                .is_some_and(|value| !value.is_null())
                || value
                    .pointer("/choices/0/delta/function_call")
                    .is_some_and(|value| !value.is_null())
            {
                return Err("unsupported_chat_output");
            }
            if let Some(reason) = value
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
            {
                let terminal = chat_finish_reason(reason)?;
                if self
                    .protocol_terminal
                    .is_some_and(|previous| previous != terminal)
                {
                    return Err("conflicting_upstream_terminal");
                }
                self.protocol_terminal = Some(terminal);
                self.finish_reason = Some(reason.to_string());
            }
            let Some(delta) = value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
            else {
                continue;
            };
            if self.text.len().saturating_add(delta.len()) > MAX_RESPONSE_BYTES {
                return Err("upstream_response_too_large");
            }
            self.text.push_str(delta);
            self.pending.push_back(sse_event("response.output_text.delta", json!({"type":"response.output_text.delta","sequence_number":self.sequence_number,"response_id":self.response_id,"item_id":self.item_id,"output_index":0,"content_index":0,"delta":delta})));
            self.sequence_number += 1;
        }
        Ok(())
    }

    fn finish(&mut self) {
        self.pending.push_back(sse_event(
            "response.output_text.done",
            json!({
                "type": "response.output_text.done",
                "sequence_number": self.sequence_number,
                "item_id": self.item_id,
                "output_index": 0,
                "content_index": 0,
                "text": self.text,
                "logprobs": []
            }),
        ));
        self.sequence_number += 1;
        self.pending.push_back(sse_event(
            "response.content_part.done",
            json!({
                "type": "response.content_part.done",
                "sequence_number": self.sequence_number,
                "item_id": self.item_id,
                "output_index": 0,
                "content_index": 0,
                "part": {"type": "output_text", "text": self.text, "annotations": [], "logprobs": []}
            }),
        ));
        self.sequence_number += 1;
        self.pending.push_back(sse_event(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "sequence_number": self.sequence_number,
                "output_index": 0,
                "item": {
                    "id": self.item_id,
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": self.text, "annotations": [], "logprobs": []}]
                }
            }),
        ));
        self.sequence_number += 1;
        let terminal = self.protocol_terminal.unwrap();
        let mut response = response_json(
            &self.response_id,
            &self.item_id,
            &self.public_model,
            &self.text,
        );
        response["status"] = json!(terminal.status());
        response["usage"] = self.raw_usage.clone();
        if terminal == ProtocolTerminal::Failed {
            response["error"] = self.stream_error.clone();
        }
        if terminal == ProtocolTerminal::Incomplete {
            response["incomplete_details"] = json!({"reason": self.finish_reason});
        }
        self.pending.push_back(sse_event(
            terminal.event_name(),
            json!({
                "type": terminal.event_name(),
                "sequence_number": self.sequence_number,
                "response": response
            }),
        ));
        self.finished = true;
    }
}
