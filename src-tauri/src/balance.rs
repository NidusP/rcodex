use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::DateTime;
use reqwest::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::{database::Database, providers::ProviderStore};

const TEAM_PROVIDER_HOSTS: &[&str] = &["ai.3rcd.com", "gpt.3rcd.com"];
const OFFICIAL_OPENAI_HOSTS: &[&str] = &["api.openai.com"];
const TEAM_PROFILE_PATH: &str = "/api/cockpit-tools/token-profile";
const TEAM_PROFILE_SOURCE: &str = "team token-profile";
const SUB2API_USAGE_PATH: &str = "/v1/usage";
const SUB2API_USAGE_SOURCE: &str = "Sub2API usage";
const CREDIT_GRANTS_PATH: &str = "/dashboard/billing/credit_grants";
const CREDIT_GRANTS_SOURCE: &str = "OpenAI credit_grants";
const BALANCE_UNIT: &str = "quota";
const BALANCE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamBalanceStatus {
    Supported,
    Stale,
    Unsupported,
    Unauthorized,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamBalance {
    pub status: UpstreamBalanceStatus,
    pub available: Option<f64>,
    pub granted: Option<f64>,
    pub used: Option<f64>,
    pub unlimited: bool,
    pub currency: Option<String>,
    pub unit: String,
    pub source: String,
    pub refreshed_at: Option<i64>,
    pub expires_at: Option<i64>,
    pub error: Option<String>,
}

impl UpstreamBalance {
    pub fn unsupported() -> Self {
        Self {
            status: UpstreamBalanceStatus::Unsupported,
            available: None,
            granted: None,
            used: None,
            unlimited: false,
            currency: None,
            unit: BALANCE_UNIT.to_string(),
            source: "no supported upstream adapter".to_string(),
            refreshed_at: None,
            expires_at: None,
            error: None,
        }
    }
}

#[derive(Clone)]
pub struct BalanceStore {
    connection: Arc<Mutex<Connection>>,
    client: Client,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BalanceAdapter {
    TeamTokenProfile,
    Sub2ApiUsage,
    OpenAiCreditGrants,
}

impl BalanceAdapter {
    fn source(self) -> &'static str {
        match self {
            Self::TeamTokenProfile => TEAM_PROFILE_SOURCE,
            Self::Sub2ApiUsage => SUB2API_USAGE_SOURCE,
            Self::OpenAiCreditGrants => CREDIT_GRANTS_SOURCE,
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::TeamTokenProfile => TEAM_PROFILE_PATH,
            Self::Sub2ApiUsage => SUB2API_USAGE_PATH,
            Self::OpenAiCreditGrants => CREDIT_GRANTS_PATH,
        }
    }
}

impl BalanceStore {
    pub fn from_database(database: Database) -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(BALANCE_TIMEOUT)
            .build()
            .expect("balance client should build");
        Self {
            connection: database.connection(),
            client,
        }
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::from_database(Database::in_memory())
    }

    pub fn cached(&self, provider_id: &str) -> Result<UpstreamBalance, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "余额数据库锁已损坏。".to_string())?;
        connection
            .query_row(
                "SELECT status, available, granted, used, unlimited, currency, unit, source, refreshed_at, expires_at, error
                 FROM provider_upstream_balances WHERE provider_id = ?1",
                [provider_id],
                row_to_balance,
            )
            .optional()
            .map_err(|error| format!("无法读取上游余额缓存：{error}"))
            .map(|value| value.unwrap_or_else(UpstreamBalance::unsupported))
    }

    pub async fn refresh(
        &self,
        provider_store: &ProviderStore,
        provider_id: &str,
    ) -> Result<UpstreamBalance, String> {
        let upstream = provider_store.upstream_for_provider(provider_id)?;
        let adapters = balance_adapters(&upstream.base_url);
        if adapters.is_empty() {
            let balance = UpstreamBalance::unsupported();
            self.save(provider_id, &balance)?;
            return Ok(balance);
        }

        let mut last_unsupported = None;
        for adapter in adapters {
            let Some(url) = balance_url(&upstream.base_url, adapter) else {
                continue;
            };
            let source = adapter.source();
            let response = match self
                .client
                .get(url)
                .bearer_auth(upstream.api_key.expose())
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    let code = if error.is_timeout() {
                        "balance_timeout"
                    } else {
                        "balance_unreachable"
                    };
                    return self.failed_balance(
                        provider_id,
                        UpstreamBalanceStatus::Error,
                        code,
                        source,
                    );
                }
            };

            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                return self.failed_balance(
                    provider_id,
                    UpstreamBalanceStatus::Unauthorized,
                    "upstream_unauthorized",
                    source,
                );
            }
            if status == reqwest::StatusCode::NOT_FOUND {
                last_unsupported = Some(("balance_endpoint_not_found", source));
                continue;
            }
            if !status.is_success() {
                return self.failed_balance(
                    provider_id,
                    UpstreamBalanceStatus::Error,
                    &format!("upstream_http_{}", status.as_u16()),
                    source,
                );
            }

            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !content_type.is_empty()
                && !content_type.starts_with("application/json")
                && !content_type.contains("+json")
            {
                last_unsupported = Some(("unsupported_balance_response", source));
                continue;
            }
            let payload = match response.json::<Value>().await {
                Ok(payload) => payload,
                Err(_) => {
                    last_unsupported = Some(("unsupported_balance_response", source));
                    continue;
                }
            };
            match parse_adapter_payload(adapter, &payload) {
                Ok(mut balance) => {
                    balance.refreshed_at = Some(now());
                    self.save(provider_id, &balance)?;
                    return Ok(balance);
                }
                Err(_) => {
                    last_unsupported = Some(("unsupported_balance_response", source));
                }
            }
        }

        let (error, source) = last_unsupported.unwrap_or((
            "balance_endpoint_not_found",
            "no supported upstream adapter",
        ));
        self.failed_balance(
            provider_id,
            UpstreamBalanceStatus::Unsupported,
            error,
            source,
        )
    }

    fn failed_balance(
        &self,
        provider_id: &str,
        status: UpstreamBalanceStatus,
        error: &str,
        source: &str,
    ) -> Result<UpstreamBalance, String> {
        let mut balance = self.cached(provider_id)?;
        if balance.refreshed_at.is_some() {
            balance.status = UpstreamBalanceStatus::Stale;
        } else {
            balance.status = status;
        }
        balance.source = source.to_string();
        balance.error = Some(error.to_string());
        self.save(provider_id, &balance)?;
        Ok(balance)
    }

    fn save(&self, provider_id: &str, balance: &UpstreamBalance) -> Result<(), String> {
        self.connection
            .lock()
            .map_err(|_| "余额数据库锁已损坏。".to_string())?
            .execute(
                "INSERT INTO provider_upstream_balances
                    (provider_id, status, available, granted, used, unlimited, currency, unit, source, refreshed_at, expires_at, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(provider_id) DO UPDATE SET
                    status=excluded.status, available=excluded.available, granted=excluded.granted,
                    used=excluded.used, unlimited=excluded.unlimited, currency=excluded.currency,
                    unit=excluded.unit, source=excluded.source, refreshed_at=excluded.refreshed_at,
                    expires_at=excluded.expires_at, error=excluded.error",
                params![
                    provider_id,
                    status_as_db(balance.status),
                    balance.available,
                    balance.granted,
                    balance.used,
                    balance.unlimited,
                    balance.currency,
                    balance.unit,
                    balance.source,
                    balance.refreshed_at,
                    balance.expires_at,
                    balance.error,
                ],
            )
            .map_err(|error| format!("无法保存上游余额缓存：{error}"))?;
        Ok(())
    }
}

fn balance_adapters(base_url: &str) -> Vec<BalanceAdapter> {
    let host = Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()));
    let is_team_host = host
        .as_deref()
        .map(|host| TEAM_PROVIDER_HOSTS.iter().any(|value| *value == host))
        .unwrap_or(false);
    if host
        .as_deref()
        .map(|host| OFFICIAL_OPENAI_HOSTS.iter().any(|value| *value == host))
        .unwrap_or(false)
    {
        return Vec::new();
    }
    let mut adapters = Vec::with_capacity(if is_team_host { 3 } else { 2 });
    if is_team_host {
        adapters.push(BalanceAdapter::TeamTokenProfile);
    }
    adapters.push(BalanceAdapter::Sub2ApiUsage);
    adapters.push(BalanceAdapter::OpenAiCreditGrants);
    adapters
}

fn balance_url(base_url: &str, adapter: BalanceAdapter) -> Option<Url> {
    let mut url = Url::parse(base_url).ok()?;
    if adapter == BalanceAdapter::TeamTokenProfile {
        let host = url.host_str()?.to_ascii_lowercase();
        if !TEAM_PROVIDER_HOSTS.iter().any(|value| *value == host) {
            return None;
        }
    }
    if adapter == BalanceAdapter::Sub2ApiUsage {
        let base_path = url.path().trim_end_matches('/');
        let path = if base_path.ends_with("/v1") {
            format!("{base_path}/usage")
        } else if base_path.is_empty() {
            adapter.path().to_string()
        } else {
            format!("{base_path}{SUB2API_USAGE_PATH}")
        };
        url.set_path(&path);
    } else {
        url.set_path(adapter.path());
    }
    url.set_query(None);
    url.set_fragment(None);
    Some(url)
}

fn parse_adapter_payload(
    adapter: BalanceAdapter,
    payload: &Value,
) -> Result<UpstreamBalance, &'static str> {
    match adapter {
        BalanceAdapter::TeamTokenProfile => parse_team_payload(payload),
        BalanceAdapter::Sub2ApiUsage => parse_sub2api_usage_payload(payload),
        BalanceAdapter::OpenAiCreditGrants => parse_credit_grants_payload(payload),
    }
}

fn parse_sub2api_usage_payload(payload: &Value) -> Result<UpstreamBalance, &'static str> {
    let mode = string_value(payload.get("mode")).unwrap_or_default();
    let quota = payload.get("quota");
    let subscription = payload.get("subscription");

    let mut available = number_value(payload.get("remaining"));
    let mut granted = None;
    let mut used = None;
    let mut unlimited = false;
    let mut unit = string_value(payload.get("unit"));
    let mut expires_at = timestamp_value(payload.get("expires_at"));

    if mode == "quota_limited" || quota.is_some() {
        let quota = quota.unwrap_or(payload);
        available = available.or_else(|| number_value(quota.get("remaining")));
        granted = number_value(quota.get("limit")).or_else(|| number_value(payload.get("limit")));
        used = number_value(quota.get("used")).or_else(|| number_value(payload.get("used")));
        unit = unit
            .or_else(|| string_value(quota.get("unit")))
            .or_else(|| Some("USD".to_string()));
        expires_at = expires_at.or_else(|| timestamp_value(quota.get("expires_at")));
    } else if mode == "unrestricted" || subscription.is_some() || payload.get("balance").is_some() {
        if let Some(subscription) = subscription {
            let limits = [
                number_value(subscription.get("daily_limit_usd")),
                number_value(subscription.get("weekly_limit_usd")),
                number_value(subscription.get("monthly_limit_usd")),
            ];
            granted = limits.into_iter().flatten().reduce(f64::min);
            used = number_value(subscription.get("daily_usage_usd"));
            expires_at = expires_at.or_else(|| timestamp_value(subscription.get("expires_at")));
        }
        available = available.or_else(|| number_value(payload.get("balance")));
        unit = unit.or_else(|| Some("USD".to_string()));
    }

    if available == Some(-1.0) {
        available = None;
        unlimited = true;
    }
    if used.is_none() {
        if let (Some(limit), Some(remaining)) = (granted, available) {
            used = Some((limit - remaining).max(0.0));
        }
    }
    if available.is_none() && !unlimited {
        return Err("missing_sub2api_balance");
    }

    Ok(UpstreamBalance {
        status: UpstreamBalanceStatus::Supported,
        available,
        granted,
        used,
        unlimited,
        currency: unit
            .as_deref()
            .filter(|value| value.eq_ignore_ascii_case("usd"))
            .map(ToOwned::to_owned),
        unit: unit.unwrap_or_else(|| BALANCE_UNIT.to_string()),
        source: SUB2API_USAGE_SOURCE.to_string(),
        refreshed_at: None,
        expires_at,
        error: None,
    })
}

fn parse_team_payload(payload: &Value) -> Result<UpstreamBalance, &'static str> {
    if payload.get("success").and_then(Value::as_bool) == Some(false) {
        return Err("upstream_rejected");
    }
    let root = payload.get("data").unwrap_or(payload);
    let usage = root.get("usage").unwrap_or(root);
    let unlimited = bool_value(usage.get("unlimited_quota"));
    let available = number_value(usage.get("total_available"));
    if available.is_none() && !unlimited {
        return Err("missing_total_available");
    }
    let currency =
        string_value(usage.get("currency")).or_else(|| string_value(root.get("currency")));
    Ok(UpstreamBalance {
        status: UpstreamBalanceStatus::Supported,
        available,
        granted: number_value(usage.get("total_granted")),
        used: number_value(usage.get("total_used")),
        unlimited,
        currency,
        unit: string_value(usage.get("unit")).unwrap_or_else(|| BALANCE_UNIT.to_string()),
        source: TEAM_PROFILE_SOURCE.to_string(),
        refreshed_at: None,
        expires_at: timestamp_value(usage.get("expires_at"))
            .or_else(|| timestamp_value(root.get("expires_at"))),
        error: None,
    })
}

fn parse_credit_grants_payload(payload: &Value) -> Result<UpstreamBalance, &'static str> {
    let root = payload.get("data").unwrap_or(payload);
    let unlimited = bool_value(root.get("unlimited_quota")) || bool_value(root.get("unlimited"));
    let available = number_value(root.get("total_available"));
    if available.is_none() && !unlimited {
        return Err("missing_total_available");
    }
    let expires_at = timestamp_value(root.get("expires_at")).or_else(|| {
        root.get("grants")
            .and_then(|grants| grants.get("data"))
            .and_then(Value::as_array)
            .and_then(|grants| {
                grants
                    .iter()
                    .filter_map(|grant| timestamp_value(grant.get("expires_at")))
                    .min()
            })
    });
    Ok(UpstreamBalance {
        status: UpstreamBalanceStatus::Supported,
        available,
        granted: number_value(root.get("total_granted")),
        used: number_value(root.get("total_used")),
        unlimited,
        currency: string_value(root.get("currency")),
        unit: string_value(root.get("unit")).unwrap_or_else(|| "credits".to_string()),
        source: CREDIT_GRANTS_SOURCE.to_string(),
        refreshed_at: None,
        expires_at,
        error: None,
    })
}

fn row_to_balance(row: &rusqlite::Row<'_>) -> rusqlite::Result<UpstreamBalance> {
    let status: String = row.get(0)?;
    Ok(UpstreamBalance {
        status: status_from_db(&status),
        available: row.get(1)?,
        granted: row.get(2)?,
        used: row.get(3)?,
        unlimited: row.get::<_, i64>(4)? != 0,
        currency: row.get(5)?,
        unit: row.get(6)?,
        source: row.get(7)?,
        refreshed_at: row.get(8)?,
        expires_at: row.get(9)?,
        error: row.get(10)?,
    })
}

fn status_as_db(status: UpstreamBalanceStatus) -> &'static str {
    match status {
        UpstreamBalanceStatus::Supported => "supported",
        UpstreamBalanceStatus::Stale => "stale",
        UpstreamBalanceStatus::Unsupported => "unsupported",
        UpstreamBalanceStatus::Unauthorized => "unauthorized",
        UpstreamBalanceStatus::Error => "error",
    }
}

fn status_from_db(value: &str) -> UpstreamBalanceStatus {
    match value {
        "supported" => UpstreamBalanceStatus::Supported,
        "stale" => UpstreamBalanceStatus::Stale,
        "unauthorized" => UpstreamBalanceStatus::Unauthorized,
        "error" => UpstreamBalanceStatus::Error,
        _ => UpstreamBalanceStatus::Unsupported,
    }
}

fn number_value(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.parse::<f64>().ok(),
        _ => None,
    }
}

fn bool_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => {
            matches!(value.to_ascii_lowercase().as_str(), "true" | "1" | "yes")
        }
        Some(Value::Number(value)) => value.as_i64() == Some(1),
        _ => false,
    }
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| {
        value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn timestamp_value(value: Option<&Value>) -> Option<i64> {
    if let Some(number) = number_value(value) {
        return Some(if number > 10_000_000_000.0 {
            (number / 1000.0) as i64
        } else {
            number as i64
        });
    }
    value
        .and_then(Value::as_str)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|date| date.timestamp())
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{
        providers::{CreateProviderInput, ProviderStore},
        secrets::{ApiKey, ApiKeyVault},
    };
    use axum::{extract::State, http::HeaderMap, routing::get, Json, Router};
    use serde_json::json;

    #[derive(Default)]
    struct MemoryVault(Mutex<std::collections::HashMap<String, String>>);

    impl ApiKeyVault for MemoryVault {
        fn load(&self, secret_ref: &str) -> Result<Option<ApiKey>, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(secret_ref)
                .map(|key| ApiKey::for_test(key)))
        }

        fn save(&self, secret_ref: &str, api_key: &ApiKey) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(secret_ref.to_string(), api_key.expose().to_string());
            Ok(())
        }

        fn clear(&self, secret_ref: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(secret_ref);
            Ok(())
        }
    }

    #[test]
    fn only_team_hosts_get_a_profile_endpoint() {
        assert_eq!(
            balance_url("https://ai.3rcd.com/v1", BalanceAdapter::TeamTokenProfile)
                .unwrap()
                .path(),
            TEAM_PROFILE_PATH
        );
        assert_eq!(
            balance_url("https://gpt.3rcd.com/v1", BalanceAdapter::TeamTokenProfile)
                .unwrap()
                .path(),
            TEAM_PROFILE_PATH
        );
        assert!(balance_url(
            "https://api.openai.com/v1",
            BalanceAdapter::TeamTokenProfile
        )
        .is_none());
    }

    #[test]
    fn credit_grants_uses_provider_origin_without_duplicate_v1() {
        let url = balance_url(
            "https://relay.example/v1?foo=bar",
            BalanceAdapter::OpenAiCreditGrants,
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://relay.example/dashboard/billing/credit_grants"
        );
    }

    #[test]
    fn sub2api_usage_preserves_provider_v1_path() {
        let url = balance_url(
            "https://relay.example/v1?foo=bar",
            BalanceAdapter::Sub2ApiUsage,
        )
        .unwrap();
        assert_eq!(url.as_str(), "https://relay.example/v1/usage");
        let origin_url =
            balance_url("https://relay.example", BalanceAdapter::Sub2ApiUsage).unwrap();
        assert_eq!(origin_url.as_str(), "https://relay.example/v1/usage");
    }

    #[test]
    fn adapter_chain_prefers_team_contract_then_sub2api_then_credit_grants() {
        assert_eq!(
            balance_adapters("https://ai.3rcd.com/v1"),
            vec![
                BalanceAdapter::TeamTokenProfile,
                BalanceAdapter::Sub2ApiUsage,
                BalanceAdapter::OpenAiCreditGrants
            ]
        );
        assert_eq!(
            balance_adapters("https://relay.example/v1"),
            vec![
                BalanceAdapter::Sub2ApiUsage,
                BalanceAdapter::OpenAiCreditGrants
            ]
        );
        assert!(balance_adapters("https://api.openai.com/v1").is_empty());
    }

    #[test]
    fn parses_cockpit_compatible_usage_payload() {
        let payload = json!({"success": true, "data": {"usage": {
            "total_available": "112", "total_granted": 200, "total_used": 88,
            "unlimited_quota": false, "currency": "USD", "expires_at": "2030-01-01T00:00:00Z"
        }}});
        let balance = parse_team_payload(&payload).unwrap();
        assert_eq!(balance.status, UpstreamBalanceStatus::Supported);
        assert_eq!(balance.available, Some(112.0));
        assert_eq!(balance.granted, Some(200.0));
        assert_eq!(balance.used, Some(88.0));
        assert_eq!(balance.expires_at, Some(1893456000));
        assert_eq!(balance.currency.as_deref(), Some("USD"));
    }

    #[test]
    fn unlimited_payload_does_not_require_available_value() {
        let payload = json!({"data": {"usage": {"unlimited_quota": true}}});
        assert!(parse_team_payload(&payload).unwrap().unlimited);
    }

    #[test]
    fn parses_openai_credit_grants_payload() {
        let payload = json!({
            "object": "credit_summary",
            "total_granted": 50,
            "total_used": "12.5",
            "total_available": 37.5,
            "currency": "USD",
            "grants": {"data": [{"expires_at": 1893456000000i64}]}
        });
        let balance = parse_credit_grants_payload(&payload).unwrap();
        assert_eq!(balance.available, Some(37.5));
        assert_eq!(balance.granted, Some(50.0));
        assert_eq!(balance.used, Some(12.5));
        assert_eq!(balance.currency.as_deref(), Some("USD"));
        assert_eq!(balance.unit, "credits");
        assert_eq!(balance.expires_at, Some(1893456000));
    }

    #[test]
    fn rejects_credit_grants_without_balance_fields() {
        assert!(parse_credit_grants_payload(&json!({"object": "credit_summary"})).is_err());
    }

    #[test]
    fn parses_sub2api_quota_usage_payload() {
        let payload = json!({
            "mode": "quota_limited",
            "isValid": true,
            "quota": {"limit": 100, "used": 37.5, "remaining": 62.5, "unit": "USD"},
            "expires_at": "2030-01-01T00:00:00Z"
        });
        let balance = parse_sub2api_usage_payload(&payload).unwrap();
        assert_eq!(balance.available, Some(62.5));
        assert_eq!(balance.granted, Some(100.0));
        assert_eq!(balance.used, Some(37.5));
        assert_eq!(balance.unit, "USD");
        assert_eq!(balance.source, SUB2API_USAGE_SOURCE);
        assert_eq!(balance.expires_at, Some(1893456000));
    }

    #[test]
    fn parses_sub2api_wallet_and_unlimited_usage_payloads() {
        let wallet = parse_sub2api_usage_payload(&json!({
            "mode": "unrestricted",
            "planName": "钱包余额",
            "remaining": 12.25,
            "balance": 12.25,
            "unit": "USD"
        }))
        .unwrap();
        assert_eq!(wallet.available, Some(12.25));
        assert_eq!(wallet.unit, "USD");

        let unlimited = parse_sub2api_usage_payload(&json!({
            "mode": "unrestricted",
            "remaining": -1,
            "unit": "USD"
        }))
        .unwrap();
        assert!(unlimited.unlimited);
        assert!(unlimited.available.is_none());
    }

    #[tokio::test]
    async fn refreshes_credit_grants_with_the_provider_api_key() {
        let observed_auth = Arc::new(Mutex::new(None::<String>));
        let upstream = Router::new()
            .route(
                "/dashboard/billing/credit_grants",
                get(
                    |State(observed_auth): State<Arc<Mutex<Option<String>>>>,
                     headers: HeaderMap| async move {
                        *observed_auth.lock().unwrap() = headers
                            .get(reqwest::header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .map(ToOwned::to_owned);
                        Json(json!({
                            "object": "credit_summary",
                            "total_granted": 10,
                            "total_used": 3,
                            "total_available": 7
                        }))
                    },
                ),
            )
            .with_state(observed_auth.clone());
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        tauri::async_runtime::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });

        let provider_store = ProviderStore::from_database(
            crate::database::Database::in_memory(),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let provider = provider_store
            .create(CreateProviderInput {
                name: "Relay".to_string(),
                base_url,
                api_key: "sk-test-balance".to_string(),
                transport_mode: Default::default(),
                gateway_timeout_settings: None,
            })
            .unwrap();
        let balance = BalanceStore::in_memory()
            .refresh(&provider_store, &provider.id)
            .await
            .unwrap();

        assert_eq!(balance.status, UpstreamBalanceStatus::Supported);
        assert_eq!(balance.available, Some(7.0));
        assert_eq!(balance.source, CREDIT_GRANTS_SOURCE);
        assert_eq!(
            observed_auth.lock().unwrap().as_deref(),
            Some("Bearer sk-test-balance")
        );
    }

    #[test]
    fn cache_round_trip_preserves_amount_and_error_metadata() {
        let store = BalanceStore::in_memory();
        let balance = UpstreamBalance {
            status: UpstreamBalanceStatus::Supported,
            available: Some(112.0),
            granted: Some(200.0),
            used: Some(88.0),
            unlimited: false,
            currency: Some("USD".to_string()),
            unit: BALANCE_UNIT.to_string(),
            source: TEAM_PROFILE_SOURCE.to_string(),
            refreshed_at: Some(123),
            expires_at: Some(456),
            error: None,
        };
        store.save("provider-a", &balance).unwrap();
        let cached = store.cached("provider-a").unwrap();
        assert_eq!(cached.available, Some(112.0));
        assert_eq!(cached.refreshed_at, Some(123));
        assert_eq!(cached.currency.as_deref(), Some("USD"));
    }
}
