//! 上游转发：Responses 透传和 Responses→Chat 文本转换。
//!
//! 不登记 sidecar 绑定，不改工作区。凭据只在当前请求边界使用，官方 Authorization 不发给第三方。
//! 由 `/v1/responses` 和压缩路径调用。

use std::time::{Duration, Instant};

use axum::{
    http::{
        header::{HeaderName, AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Response as UpstreamResponse;
use serde_json::{json, Value};
use tokio::time::timeout;

use crate::conversation_archive::RequestCapture;
use crate::diagnostics::RequestDiagnosticOutcome;
use crate::routing::{GatewayTimeoutSnapshot, ModelRoute, WireApi};

use super::diagnostics::RequestMeta;
use super::diagnostics::{
    record_request_best_effort, request_event, request_event_without_status, request_metadata,
    RequestRecorder,
};
use super::errors::{
    annotate_upstream_detail, api_error, api_error_with_detail, bounded_response_bytes,
    classify_upstream_send, upstream_error_detail,
};
use super::helpers::{
    auth_kind, gateway_trace, is_loopback_url, route_kind, route_provider_id, safe_upstream_url,
    upstream_request_url,
};
use super::protocol::{
    is_event_stream_content_type, response_json, response_request_to_chat, rewrite_model_fields,
    usage_from_event_value, validate_response,
};
use super::runtime_types::ProviderUpstreamRuntime;
use super::stream::{chat_sse_to_responses, rewrite_sse_response, synchronous_response_as_sse};
use super::MAX_RESPONSE_BYTES;

pub(super) async fn forward_provider(
    upstream: &ProviderUpstreamRuntime,
    route: &ModelRoute,
    request: serde_json::Map<String, Value>,
    instance_id: Option<String>,
    recorder: Option<RequestRecorder>,
    forwarded_auth: Option<&HeaderMap>,
) -> Response {
    let started = Instant::now();
    if let Some(capture) = recorder.as_ref().and_then(|r| r.archive.as_ref()) {
        register_archive_secrets(capture, upstream, forwarded_auth);
        capture.metadata(json!({
            "providerId": upstream.provider_id,
            "providerRevisionId": upstream.provider_revision_id,
            "publicModel": route.public_model_id,
            "upstreamModel": route.upstream_model_id,
            "wireApi": format!("{:?}", route.wire_api),
        }));
    }
    match route.wire_api {
        WireApi::Responses => {
            let stream = request
                .get("stream")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let body = match serde_json::to_vec(&Value::Object(request.clone())) {
                Ok(body) => body,
                Err(_) => {
                    return api_error(StatusCode::INTERNAL_SERVER_ERROR, "serialization_failed")
                }
            };
            if let Some(capture) = recorder.as_ref().and_then(|r| r.archive.as_ref()) {
                capture.stage("upstream-request", &body);
            }
            let url = upstream_request_url(&upstream.base_url, WireApi::Responses);
            let safe_url = safe_upstream_url(&url);
            let instance = instance_id.clone().unwrap_or_default();
            gateway_trace(&[
                ("event", "forward"),
                ("instance", &instance),
                ("public_model", &route.public_model_id),
                ("upstream_model", &route.upstream_model_id),
                ("provider", &upstream.provider_id),
                ("route_kind", route_kind(route)),
                ("auth", auth_kind(upstream, forwarded_auth)),
                ("url", &safe_url),
                ("loopback", if is_loopback_url(&url) { "1" } else { "0" }),
                ("stream", if stream { "1" } else { "0" }),
            ]);
            let outbound = apply_upstream_auth(
                upstream
                    .client
                    .post(&url)
                    .header(CONTENT_TYPE, "application/json"),
                upstream,
                forwarded_auth,
            )
            .body(body);
            let input_meta = request_metadata(&Value::Object(request.clone()));
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
                        "responses",
                        stream,
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
                        "responses",
                        stream,
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
            forward_responses_response(
                response,
                route,
                stream,
                &upstream.timeouts,
                instance_id,
                recorder,
                request,
                &url,
                started,
            )
            .await
        }
        WireApi::ChatCompletions => {
            forward_chat_completions(
                upstream,
                route,
                Value::Object(request),
                instance_id,
                recorder,
                forwarded_auth,
            )
            .await
        }
    }
}

pub(super) fn register_archive_secrets(
    capture: &RequestCapture,
    upstream: &ProviderUpstreamRuntime,
    forwarded_auth: Option<&HeaderMap>,
) {
    if let Some(key) = upstream.api_key.as_ref() {
        capture.add_secret(key.expose());
        capture.add_secret(&format!("Bearer {}", key.expose()));
    }
    if let Some(headers) = forwarded_auth {
        for name in [
            AUTHORIZATION,
            HeaderName::from_static("x-api-key"),
            HeaderName::from_static("x-goog-api-key"),
        ] {
            if let Some(value) = headers.get(&name).and_then(|value| value.to_str().ok()) {
                capture.add_secret(value);
            }
        }
    }
    if let Ok(url) = reqwest::Url::parse(&upstream.base_url) {
        if !url.username().is_empty() {
            capture.add_secret(url.username());
        }
        if let Some(password) = url.password() {
            capture.add_secret(password);
        }
    }
}

pub(super) fn transport_failure(
    recorder: &Option<RequestRecorder>,
    input_meta: &RequestMeta,
    instance_id: Option<String>,
    route: &ModelRoute,
    wire_api: &'static str,
    stream: bool,
    outcome: RequestDiagnosticOutcome,
    code: &'static str,
    status: StatusCode,
    url: &str,
    cause: Option<&str>,
    started: Instant,
) -> Response {
    let instance = instance_id.clone().unwrap_or_default();
    let safe_url = safe_upstream_url(url);
    gateway_trace(&[
        ("event", "transport_error"),
        ("instance", &instance),
        ("public_model", &route.public_model_id),
        ("upstream_model", &route.upstream_model_id),
        ("provider", route_provider_id(route)),
        ("route_kind", route_kind(route)),
        ("code", code),
        ("url", &safe_url),
        ("cause", cause.unwrap_or("-")),
    ]);
    let mut event = request_event_without_status(
        input_meta,
        instance_id,
        route,
        wire_api,
        started.elapsed().as_millis() as i64,
        outcome,
        code,
    );
    event.stream = stream;
    event.error_detail = annotate_upstream_detail(url, cause.map(str::to_owned));
    record_request_best_effort(recorder, event);
    api_error_with_detail(status, code, Some(url), cause.map(str::to_owned))
}

pub(super) fn apply_upstream_auth(
    request: reqwest::RequestBuilder,
    upstream: &ProviderUpstreamRuntime,
    _forwarded_auth: Option<&HeaderMap>,
) -> reqwest::RequestBuilder {
    match upstream.api_key.as_ref() {
        Some(api_key) => request.bearer_auth(api_key.expose()),
        None => request,
    }
}

pub(super) async fn forward_responses_response(
    response: UpstreamResponse,
    route: &ModelRoute,
    stream: bool,
    timeouts: &GatewayTimeoutSnapshot,
    instance_id: Option<String>,
    recorder: Option<RequestRecorder>,
    request: serde_json::Map<String, Value>,
    url: &str,
    started: Instant,
) -> Response {
    let input_meta = request_metadata(&Value::Object(request));
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if let Some(capture) = recorder.as_ref().and_then(|r| r.archive.as_ref()) {
        capture.metadata(serde_json::json!({
            "stage": "upstream-response",
            "status": status.as_u16(),
            "contentType": content_type,
        }));
    }
    if !status.is_success() {
        let detail =
            upstream_error_detail(response, recorder.as_ref().and_then(|r| r.archive.as_ref()))
                .await;
        let annotated = annotate_upstream_detail(url, detail.clone());
        let instance = instance_id.clone().unwrap_or_default();
        gateway_trace(&[
            ("event", "upstream_error"),
            ("instance", &instance),
            ("public_model", &route.public_model_id),
            ("upstream_model", &route.upstream_model_id),
            ("provider", route_provider_id(route)),
            ("route_kind", route_kind(route)),
            ("status", &status.as_u16().to_string()),
            ("url", &safe_upstream_url(url)),
            ("detail", annotated.as_deref().unwrap_or("-")),
        ]);
        let mut event = request_event(
            &input_meta,
            instance_id,
            route,
            "responses",
            status.as_u16(),
            content_type.as_deref(),
            None,
            Some(started.elapsed().as_millis() as i64),
            RequestDiagnosticOutcome::Failed,
            Some("upstream_error"),
            0,
            None,
        );
        event.stream = stream;
        event.error_detail = annotated;
        record_request_best_effort(&recorder, event);
        return api_error_with_detail(status, "upstream_error", Some(url), detail);
    }
    if stream && !is_event_stream_content_type(content_type.as_deref()) {
        return synchronous_response_as_sse(
            response,
            route,
            timeouts,
            instance_id,
            recorder,
            input_meta,
            status,
            content_type,
            started,
        )
        .await;
    }
    if stream {
        return rewrite_sse_response(
            response,
            route.upstream_model_id.clone(),
            route.public_model_id.clone(),
            Duration::from_secs(timeouts.stream_idle_seconds),
            recorder,
            input_meta,
            instance_id,
            route,
            status.as_u16(),
            content_type,
            started,
        );
    }
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
            record_request_best_effort(
                &recorder,
                request_event(
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
                ),
            );
            return api_error(StatusCode::GATEWAY_TIMEOUT, "upstream_response_timeout");
        }
        Ok(Err(_)) => {
            record_request_best_effort(
                &recorder,
                request_event(
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
                ),
            );
            return api_error(StatusCode::BAD_GATEWAY, "upstream_interrupted");
        }
        Ok(Ok(bytes)) => bytes,
    };
    let mut value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => value,
        Err(_) => {
            record_request_best_effort(
                &recorder,
                request_event(
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
                ),
            );
            return api_error(StatusCode::BAD_GATEWAY, "invalid_upstream_response");
        }
    };
    let terminal = match validate_response(&value) {
        Ok(terminal) => terminal,
        Err(code) => {
            record_request_best_effort(
                &recorder,
                request_event(
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
                ),
            );
            return api_error(StatusCode::BAD_GATEWAY, code);
        }
    };
    rewrite_model_fields(&mut value, &route.upstream_model_id, &route.public_model_id);
    let usage = usage_from_event_value(&value);
    let elapsed = started.elapsed().as_millis() as i64;
    let mut event = request_event(
        &input_meta,
        instance_id,
        route,
        "responses",
        status.as_u16(),
        content_type.as_deref(),
        None,
        Some(elapsed),
        terminal.diagnostic().0,
        terminal.diagnostic().1,
        bytes.len(),
        Some(&bytes),
    );
    if let Some(usage) = usage {
        event.input_tokens = usage.input_tokens;
        event.output_tokens = usage.output_tokens;
        event.total_tokens = usage.total_tokens;
    }
    record_request_best_effort(&recorder, event);
    (status, Json(value)).into_response()
}

pub(super) async fn forward_chat_completions(
    upstream: &ProviderUpstreamRuntime,
    route: &ModelRoute,
    request: Value,
    instance_id: Option<String>,
    recorder: Option<RequestRecorder>,
    forwarded_auth: Option<&HeaderMap>,
) -> Response {
    let started = Instant::now();
    let input_meta = request_metadata(&request);
    let chat = match response_request_to_chat(&request) {
        Ok(chat) => chat,
        Err(code) => return api_error(StatusCode::BAD_REQUEST, code),
    };
    let chat_body = match serde_json::to_vec(&chat) {
        Ok(body) => body,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "serialization_failed"),
    };
    if let Some(capture) = recorder.as_ref().and_then(|r| r.archive.as_ref()) {
        capture.stage("upstream-request", &chat_body);
    }
    let stream = chat.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let url = upstream_request_url(&upstream.base_url, WireApi::ChatCompletions);
    let safe_url = safe_upstream_url(&url);
    let instance = instance_id.clone().unwrap_or_default();
    gateway_trace(&[
        ("event", "forward"),
        ("instance", &instance),
        ("public_model", &route.public_model_id),
        ("upstream_model", &route.upstream_model_id),
        ("provider", &upstream.provider_id),
        ("route_kind", route_kind(route)),
        ("auth", auth_kind(upstream, forwarded_auth)),
        ("url", &safe_url),
        ("loopback", if is_loopback_url(&url) { "1" } else { "0" }),
        ("stream", if stream { "1" } else { "0" }),
        ("wire", "chat_completions"),
    ]);
    let outbound = apply_upstream_auth(
        upstream
            .client
            .post(&url)
            .header(CONTENT_TYPE, "application/json"),
        upstream,
        forwarded_auth,
    )
    .body(chat_body);
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
                "chat_completions",
                stream,
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
                "chat_completions",
                stream,
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
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if !status.is_success() {
        let detail =
            upstream_error_detail(response, recorder.as_ref().and_then(|r| r.archive.as_ref()))
                .await;
        let annotated = annotate_upstream_detail(&url, detail.clone());
        gateway_trace(&[
            ("event", "upstream_error"),
            ("instance", &instance),
            ("public_model", &route.public_model_id),
            ("upstream_model", &route.upstream_model_id),
            ("provider", route_provider_id(route)),
            ("route_kind", route_kind(route)),
            ("status", &status.as_u16().to_string()),
            ("url", &safe_url),
            ("detail", annotated.as_deref().unwrap_or("-")),
            ("wire", "chat_completions"),
        ]);
        let mut event = request_event(
            &input_meta,
            instance_id,
            route,
            "chat_completions",
            status.as_u16(),
            content_type.as_deref(),
            None,
            Some(started.elapsed().as_millis() as i64),
            RequestDiagnosticOutcome::Failed,
            Some("upstream_error"),
            0,
            None,
        );
        event.stream = stream;
        event.error_detail = annotated;
        record_request_best_effort(&recorder, event);
        return api_error_with_detail(status, "upstream_error", Some(&url), detail);
    }
    if stream {
        return chat_sse_to_responses(
            response,
            route.public_model_id.clone(),
            Duration::from_secs(upstream.timeouts.stream_idle_seconds),
            recorder,
            input_meta,
            instance_id,
            route,
            status.as_u16(),
            content_type,
            started,
        );
    }
    let bytes = match timeout(
        Duration::from_secs(upstream.timeouts.non_stream_total_seconds),
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
                "chat_completions",
                status.as_u16(),
                content_type.as_deref(),
                None,
                Some(started.elapsed().as_millis() as i64),
                RequestDiagnosticOutcome::Timeout,
                Some("upstream_response_timeout"),
                0,
                None,
            );
            event.stream = stream;
            record_request_best_effort(&recorder, event);
            return api_error(StatusCode::GATEWAY_TIMEOUT, "upstream_response_timeout");
        }
        Ok(Err(_)) => {
            let mut event = request_event(
                &input_meta,
                instance_id,
                route,
                "chat_completions",
                status.as_u16(),
                content_type.as_deref(),
                None,
                Some(started.elapsed().as_millis() as i64),
                RequestDiagnosticOutcome::Interrupted,
                Some("upstream_interrupted"),
                0,
                None,
            );
            event.stream = stream;
            record_request_best_effort(&recorder, event);
            return api_error(StatusCode::BAD_GATEWAY, "upstream_interrupted");
        }
        Ok(Ok(bytes)) => bytes,
    };
    let chat = match serde_json::from_slice::<Value>(&bytes) {
        Ok(chat) => chat,
        Err(_) => {
            let mut event = request_event(
                &input_meta,
                instance_id,
                route,
                "chat_completions",
                status.as_u16(),
                content_type.as_deref(),
                None,
                Some(started.elapsed().as_millis() as i64),
                RequestDiagnosticOutcome::FormatError,
                Some("invalid_upstream_response"),
                bytes.len(),
                Some(&bytes),
            );
            event.stream = stream;
            record_request_best_effort(&recorder, event);
            return api_error(StatusCode::BAD_GATEWAY, "invalid_upstream_response");
        }
    };
    let text = chat
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .or_else(|| chat.pointer("/choices/0/text").and_then(Value::as_str));
    let usage = usage_from_event_value(&chat);
    match text {
        Some(text) => {
            let elapsed = started.elapsed().as_millis() as i64;
            let mut event = request_event(
                &input_meta,
                instance_id,
                route,
                "chat_completions",
                status.as_u16(),
                content_type.as_deref(),
                Some(elapsed),
                Some(elapsed),
                RequestDiagnosticOutcome::Success,
                None,
                text.len(),
                Some(&bytes),
            );
            event.stream = stream;
            if let Some(usage) = usage {
                event.input_tokens = usage.input_tokens;
                event.output_tokens = usage.output_tokens;
                event.total_tokens = usage.total_tokens;
            }
            record_request_best_effort(&recorder, event);
            Json(response_json("", "", &route.public_model_id, text)).into_response()
        }
        None => {
            let mut event = request_event(
                &input_meta,
                instance_id,
                route,
                "chat_completions",
                status.as_u16(),
                content_type.as_deref(),
                None,
                Some(started.elapsed().as_millis() as i64),
                RequestDiagnosticOutcome::FormatError,
                Some("invalid_upstream_response"),
                bytes.len(),
                Some(&bytes),
            );
            event.stream = stream;
            record_request_best_effort(&recorder, event);
            api_error(StatusCode::BAD_GATEWAY, "invalid_upstream_response")
        }
    }
}
