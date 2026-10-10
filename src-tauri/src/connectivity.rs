#[cfg(test)]
use reqwest::Client;
#[cfg(test)]
use serde_json::{json, Value};

#[cfg(test)]
use crate::diagnostics::CheckStatus;
#[cfg(test)]
use crate::diagnostics::{now, ConnectivityStatus};
#[cfg(test)]
use crate::providers::{GatewayUpstream, TransportMode};
use crate::{
    diagnostics::{
        DiagnosticEventInput, DiagnosticOutcome, DiagnosticSource, DiagnosticsStore,
        ProviderConnectivityReport,
    },
    providers::ProviderStore,
};

pub async fn test_provider(
    provider_id: &str,
    provider_store: &ProviderStore,
    diagnostics_store: &DiagnosticsStore,
    mutable: bool,
) -> Result<ProviderConnectivityReport, String> {
    let upstream = provider_store.upstream_for_provider(provider_id)?;
    let client = crate::channel_probe::probe_client()?;
    let execution =
        crate::channel_probe::execute(&client, &upstream.base_url, upstream.api_key.expose()).await;
    let mut revision_id = upstream.provider_revision_id.clone();
    if let Some(desired) = crate::channel_probe::desired_transport(
        upstream.transport_mode,
        execution.snapshot.wire_api.as_deref(),
    ) {
        if mutable {
            let _ = provider_store.update(
                provider_id,
                crate::providers::UpdateProviderInput {
                    name: upstream.provider_name.clone(),
                    base_url: upstream.base_url.clone(),
                    api_key: None,
                    transport_mode: desired,
                    gateway_timeout_settings: None,
                },
            );
            if let Ok(next) = provider_store.upstream_for_provider(provider_id) {
                revision_id = next.provider_revision_id;
            }
        }
    }
    record_probe_calls(diagnostics_store, &upstream.provider_id, &execution.calls);
    diagnostics_store.store_channel_probe(provider_id, &revision_id, &execution.snapshot)
}

#[cfg(test)]
async fn probe_upstream(
    upstream: GatewayUpstream,
    diagnostics_store: &DiagnosticsStore,
    client: Client,
) -> ProviderConnectivityReport {
    let execution =
        crate::channel_probe::execute(&client, &upstream.base_url, upstream.api_key.expose()).await;
    record_probe_calls(diagnostics_store, &upstream.provider_id, &execution.calls);
    ProviderConnectivityReport {
        provider_id: upstream.provider_id,
        checked_at: now(),
        status: execution.snapshot.status,
        model: execution.snapshot.model,
        models: execution.snapshot.models,
        models_check: execution.snapshot.models_check,
        text_check: execution.snapshot.text_check,
        error_code: execution.snapshot.error_code,
        wire_api: execution.snapshot.wire_api,
        catalog_revision_id: None,
    }
}

pub(crate) fn record_probe_calls(
    diagnostics_store: &DiagnosticsStore,
    provider_id: &str,
    calls: &[crate::channel_probe::ProbeCall],
) {
    for call in calls {
        let _ = diagnostics_store.record(DiagnosticEventInput {
            provider_id,
            source: DiagnosticSource::Probe,
            route: call.route,
            model: call.model.as_deref(),
            transport_mode: call.transport_mode,
            stream: false,
            outcome: if call.success {
                DiagnosticOutcome::Success
            } else {
                DiagnosticOutcome::Failed
            },
            http_status: call.http_status,
            latency_ms: Some(call.latency_ms),
            error_code: call.error_code.as_deref(),
        });
    }
}

#[cfg(test)]
fn connectivity_from_error(status: u16, body: &[u8]) -> ConnectivityStatus {
    if matches!(status, 401 | 403) || body_indicates_auth_failure(body) {
        ConnectivityStatus::Unauthorized
    } else {
        ConnectivityStatus::Degraded
    }
}

#[cfg(test)]
fn status_error_code(status: u16) -> &'static str {
    match status {
        401 | 403 => "upstream_unauthorized",
        404 | 405 => "upstream_route_unsupported",
        _ => "upstream_http_error",
    }
}

#[cfg(test)]
fn upstream_error_code(status: u16, body: &[u8]) -> &'static str {
    if matches!(status, 401 | 403) || body_indicates_auth_failure(body) {
        "upstream_unauthorized"
    } else {
        status_error_code(status)
    }
}

#[cfg(test)]
fn body_indicates_auth_failure(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    [
        "authentication failed",
        "invalid api key",
        "invalid_api_key",
        "unauthorized",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

#[cfg(test)]
mod tests {
    use axum::{
        http::StatusCode,
        response::IntoResponse,
        routing::{get, post},
        Json, Router,
    };
    use tokio::net::TcpListener;

    use super::*;
    use crate::secrets::ApiKey;

    fn test_upstream(base_url: String) -> GatewayUpstream {
        GatewayUpstream {
            provider_id: "provider-test".to_string(),
            provider_revision_id: "provider-revision-test".to_string(),
            base_url,
            api_key: ApiKey::for_test("sk-test"),
            provider_name: "Test".to_string(),
            transport_mode: TransportMode::GatewayPassthrough,
        }
    }

    #[tokio::test]
    async fn successful_probe_records_models_and_text_checks() {
        let server = Router::new()
            .route("/models", get(models_response))
            .route("/responses", post(text_response));
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
        let diagnostics = DiagnosticsStore::in_memory();

        let report = probe_upstream(test_upstream(base_url), &diagnostics, Client::new()).await;

        assert!(matches!(report.status, ConnectivityStatus::Ready));
        assert!(matches!(report.models_check, CheckStatus::Ready));
        assert!(matches!(report.text_check, CheckStatus::Ready));
        assert_eq!(report.models.len(), 2);
        assert_eq!(report.models[0].upstream_model_id, "test-model");
        assert_eq!(report.models[1].upstream_model_id, "second-model");
        assert_eq!(diagnostics.recent(Some("provider-test")).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn unauthorized_probe_stops_before_the_text_request() {
        let server = Router::new().route(
            "/models",
            get(|| async { StatusCode::UNAUTHORIZED.into_response() }),
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
        let diagnostics = DiagnosticsStore::in_memory();

        let report = probe_upstream(test_upstream(base_url), &diagnostics, Client::new()).await;

        assert!(matches!(report.status, ConnectivityStatus::Unauthorized));
        assert!(matches!(report.text_check, CheckStatus::NotRun));
        assert_eq!(diagnostics.recent(Some("provider-test")).unwrap().len(), 1);
    }

    async fn models_response() -> Json<Value> {
        Json(json!({
            "object": "list",
            "data": [
                {"id": "test-model", "owned_by": "test"},
                {"id": "second-model", "name": "Second", "owned_by": "test"},
                {"id": "test-model", "owned_by": "duplicate"}
            ]
        }))
    }

    async fn text_response() -> Json<Value> {
        Json(json!({"id": "response-test"}))
    }

    #[test]
    fn relay_auth_error_is_classified_as_unauthorized_even_when_wrapped_in_502() {
        let body = br#"{"error":{"message":"Upstream authentication failed, please contact administrator"}}"#;
        assert_eq!(upstream_error_code(502, body), "upstream_unauthorized");
        assert!(matches!(
            connectivity_from_error(502, body),
            ConnectivityStatus::Unauthorized
        ));
    }
}
