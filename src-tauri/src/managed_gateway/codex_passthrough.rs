//! Codex client identity headers for GPT-family routes (and optional whole-channel `pass_through`).
//!
//! `Authorization` is never forwarded: upstream auth stays on the RouteSet-bound provider API Key.
//! Compat (`GatewayPassthrough`) must not synthesize Codex CLI fingerprint headers on non-GPT routes.

use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderName};
use reqwest::RequestBuilder;

use crate::managed_codex_config::{INSTANCE_GATEWAY_HEADER, LEGACY_INSTANCE_GATEWAY_HEADER};
use crate::providers::TransportMode;
use crate::routing::{looks_like_gpt_model_id, ModelRoute, RouteSource};

use super::errors::{api_error_with_detail, bounded_response_bytes};
use super::helpers::{gateway_trace, safe_upstream_url};
use super::runtime_types::{BindingRuntime, ProviderUpstreamRuntime};
use super::MAX_RESPONSE_BYTES;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::time::timeout;

const HOP_BY_HOP: &[&str] = &[
    "host",
    "connection",
    "content-length",
    "transfer-encoding",
    "te",
    "trailer",
    "upgrade",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
];

pub(super) fn is_codex_header_passthrough(upstream: &ProviderUpstreamRuntime) -> bool {
    upstream.transport_mode == TransportMode::CodexHeaderPassthrough
}

/// GPT-family routes (or whole-provider `pass_through`) need Codex client identity on the wire.
pub(super) fn route_needs_codex_identity_passthrough(
    route: &ModelRoute,
    upstream: &ProviderUpstreamRuntime,
) -> bool {
    is_codex_header_passthrough(upstream)
        || looks_like_gpt_model_id(&route.public_model_id, &route.upstream_model_id)
}

pub(super) fn binding_should_proxy_models_with_client_identity<'a>(
    binding: &'a BindingRuntime,
) -> Option<&'a ProviderUpstreamRuntime> {
    if let Some(upstream) = binding
        .upstreams
        .values()
        .find(|upstream| is_codex_header_passthrough(upstream))
    {
        return Some(upstream);
    }
    let needs_proxy = binding.route_set.routes.iter().any(|route| {
        looks_like_gpt_model_id(&route.public_model_id, &route.upstream_model_id)
            && matches!(route.source, RouteSource::ApiProvider { .. })
    });
    if !needs_proxy {
        return None;
    }
    binding
        .route_set
        .providers
        .first()
        .and_then(|provider| binding.upstreams.get(&provider.provider_revision_id))
}

fn hop_by_hop(name: &HeaderName) -> bool {
    let tag = name.as_str();
    HOP_BY_HOP
        .iter()
        .any(|blocked| tag.eq_ignore_ascii_case(blocked))
        || tag.eq_ignore_ascii_case(INSTANCE_GATEWAY_HEADER)
        || tag.eq_ignore_ascii_case(LEGACY_INSTANCE_GATEWAY_HEADER)
}

pub(super) fn attach_client_headers(builder: RequestBuilder, client: &HeaderMap) -> RequestBuilder {
    let mut headers = HeaderMap::new();
    for (name, value) in client.iter() {
        if hop_by_hop(name) || name == AUTHORIZATION {
            continue;
        }
        headers.append(name, value.clone());
    }
    builder.headers(headers)
}

pub(super) fn upstream_failure_code(status: StatusCode, detail: Option<&str>) -> &'static str {
    if status == StatusCode::FORBIDDEN {
        let detail = detail.unwrap_or("").to_ascii_lowercase();
        if detail.contains("cli_only")
            || detail.contains("codex_cli")
            || detail.contains("codex-cli")
        {
            return "upstream_cli_rejected";
        }
        return "upstream_forbidden";
    }
    if status == StatusCode::UNAUTHORIZED {
        return "upstream_unauthorized";
    }
    "upstream_error"
}

pub(super) fn passthrough_models_url(base_url: &str, query: Option<&str>) -> String {
    let trimmed = base_url.trim_end_matches('/');
    match query.filter(|value| !value.is_empty()) {
        Some(query) => format!("{trimmed}/models?{query}"),
        None => format!("{trimmed}/models"),
    }
}

pub(super) async fn proxy_models_list(
    upstream: &ProviderUpstreamRuntime,
    client_headers: &HeaderMap,
    query: Option<&str>,
    binding_id: &str,
) -> Response {
    let url = passthrough_models_url(&upstream.base_url, query);
    let safe_url = safe_upstream_url(&url);
    gateway_trace(&[
        ("event", "models_proxy"),
        ("instance", binding_id),
        ("provider", &upstream.provider_id),
        ("url", &safe_url),
        ("mode", "pass_through"),
    ]);
    let started = Instant::now();
    let mut builder = attach_client_headers(upstream.client.get(&url), client_headers);
    if let Some(api_key) = upstream.api_key.as_ref() {
        builder = builder.bearer_auth(api_key.expose());
    }
    let response = match timeout(
        Duration::from_secs(upstream.timeouts.first_byte_seconds),
        builder.send(),
    )
    .await
    {
        Err(_) => {
            return api_error_with_detail(
                StatusCode::GATEWAY_TIMEOUT,
                "upstream_first_byte_timeout",
                Some(&url),
                None,
            )
        }
        Ok(Err(_)) => {
            return api_error_with_detail(
                StatusCode::BAD_GATEWAY,
                "upstream_connect_failed",
                Some(&url),
                None,
            )
        }
        Ok(Ok(response)) => response,
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    gateway_trace(&[
        ("event", "models_proxy_done"),
        ("instance", binding_id),
        ("status", &status.as_u16().to_string()),
        ("latency_ms", &started.elapsed().as_millis().to_string()),
    ]);
    if !status.is_success() {
        let detail = response
            .text()
            .await
            .ok()
            .and_then(|body| sanitize_models_error_detail(&body));
        let code = upstream_failure_code(status, detail.as_deref());
        return api_error_with_detail(status, code, Some(&url), detail);
    }
    let bytes = match timeout(
        Duration::from_secs(upstream.timeouts.non_stream_total_seconds),
        bounded_response_bytes(response, MAX_RESPONSE_BYTES, None),
    )
    .await
    {
        Err(_) => {
            return api_error_with_detail(
                StatusCode::GATEWAY_TIMEOUT,
                "upstream_response_timeout",
                Some(&url),
                None,
            )
        }
        Ok(Err(_)) => {
            return api_error_with_detail(
                StatusCode::BAD_GATEWAY,
                "upstream_interrupted",
                Some(&url),
                None,
            )
        }
        Ok(Ok(bytes)) => bytes,
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return api_error_with_detail(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                Some(&url),
                None,
            )
        }
    };
    (status, Json(value)).into_response()
}

fn sanitize_models_error_detail(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    let sample: String = trimmed.chars().take(512).collect();
    Some(sample)
}
