//! URL、路由元数据和 stderr 追踪。
//!
//! 不发 HTTP、不持有凭据、不解码 SSE。转发和压缩模块用这些纯函数打点。

use axum::http::HeaderMap;
use std::io::Write;

use crate::routing::{
    is_ephemeral_family_alias, ModelRoute, RouteSetRevision, RouteSource, WireApi,
    EPHEMERAL_FAMILY_ALIAS_ADAPTER,
};
use serde_json::Value;

use super::runtime_types::ProviderUpstreamRuntime;

/// One-line stderr trace for locating loopback 502s. Values must already be
/// free of secrets, request bodies, and response bodies.
pub(super) fn gateway_trace(fields: &[(&str, &str)]) {
    let mut line = String::from("codex-gateway:");
    for (key, value) in fields {
        if value.is_empty() {
            continue;
        }
        line.push(' ');
        line.push_str(key);
        line.push('=');
        if value
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
        {
            line.push('"');
            line.push_str(&value.replace(['"', '\n', '\r'], "'"));
            line.push('"');
        } else {
            line.push_str(value);
        }
    }
    // stderr is fully buffered when it is a file (the sidecar log), so a
    // 502 can happen and still leave gateway-sidecar.log empty until exit.
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "{line}");
    let _ = stderr.flush();
}

pub(super) fn safe_upstream_url(value: &str) -> String {
    let Ok(mut url) = url::Url::parse(value) else {
        return "[invalid-upstream-url]".to_string();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

pub(super) fn upstream_request_url(base_url: &str, wire_api: WireApi) -> String {
    match wire_api {
        WireApi::Responses => format!("{base_url}/responses"),
        WireApi::ChatCompletions => format!("{base_url}/chat/completions"),
    }
}

pub(super) fn is_loopback_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

pub(super) fn ephemeral_family_alias_route(
    route_set: &RouteSetRevision,
    public_model: &str,
) -> Option<ModelRoute> {
    let snapshot =
        serde_json::from_str::<Value>(&route_set.gateway_service.config_snapshot).ok()?;
    let standin_id = snapshot
        .get("ephemeralFamilyAliases")?
        .get(public_model)?
        .as_str()?;
    let standin = route_set
        .routes
        .iter()
        .find(|route| route.public_model_id == standin_id)?;
    let mut alias = standin.clone();
    alias.public_model_id = public_model.to_string();
    alias.adapter_kind = EPHEMERAL_FAMILY_ALIAS_ADAPTER.to_string();
    Some(alias)
}

pub(super) fn route_kind(route: &ModelRoute) -> &'static str {
    if route.adapter_kind == "compatibility_native_passthrough" {
        return "compatibility_native";
    }
    if is_ephemeral_family_alias(route) {
        return EPHEMERAL_FAMILY_ALIAS_ADAPTER;
    }
    match &route.source {
        RouteSource::Official { .. } => "official",
        RouteSource::ApiProvider { .. } => "channel",
    }
}

pub(super) fn route_provider_id(route: &ModelRoute) -> &str {
    match &route.source {
        RouteSource::ApiProvider { provider_id, .. } => provider_id,
        RouteSource::Official { account_id, .. } => account_id,
    }
}

pub(super) fn auth_kind(
    upstream: &ProviderUpstreamRuntime,
    forwarded_auth: Option<&HeaderMap>,
) -> &'static str {
    if upstream.api_key.is_some() {
        "provider_key"
    } else if forwarded_auth.is_some() {
        "forwarded"
    } else {
        "none"
    }
}
