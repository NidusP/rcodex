use std::sync::{Arc, Mutex};

use crate::database::Database;
use chrono::{Datelike, Duration, Local, TimeZone};
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::balance::{BalanceStore, UpstreamBalance};

#[derive(Clone)]
pub struct UsageStore {
    connection: Arc<Mutex<Connection>>,
    balance_store: BalanceStore,
}

#[derive(Clone, Copy)]
pub struct UsageObservation {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageSummary {
    pub today: UsagePeriod,
    pub week: UsagePeriod,
    pub month: UsagePeriod,
    pub last_request_at: Option<i64>,
    pub upstream_balance: UpstreamBalance,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePeriod {
    pub requests: i64,
    pub successes: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
}

impl UsageStore {
    pub fn from_database(database: Database) -> Self {
        Self {
            connection: database.connection(),
            balance_store: BalanceStore::from_database(database),
        }
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::from_database(Database::in_memory())
    }

    pub fn record(
        &self,
        provider_id: &str,
        model: Option<&str>,
        succeeded: bool,
        usage: Option<UsageObservation>,
    ) -> Result<(), String> {
        self.connection.lock().map_err(|_| "用量数据库锁已损坏。".to_string())?
            .execute("INSERT INTO provider_usage_events (provider_id, created_at, model, succeeded, input_tokens, output_tokens) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![provider_id, now(), model, succeeded, usage.and_then(|item| item.input_tokens), usage.and_then(|item| item.output_tokens)])
            .map_err(|error| format!("无法记录本地用量：{error}"))?;
        Ok(())
    }

    pub fn summary(&self, provider_id: &str) -> Result<ProviderUsageSummary, String> {
        Ok(ProviderUsageSummary {
            today: self.period(provider_id, start_of_today())?,
            week: self.period(provider_id, start_of_week())?,
            month: self.period(provider_id, start_of_month())?,
            last_request_at: self.last_request_at(provider_id)?,
            upstream_balance: self.balance_store.cached(provider_id)?,
        })
    }

    fn last_request_at(&self, provider_id: &str) -> Result<Option<i64>, String> {
        self.connection
            .lock()
            .map_err(|_| "用量数据库锁已损坏。".to_string())?
            .query_row(
                "SELECT MAX(created_at) FROM provider_usage_events WHERE provider_id = ?1",
                [provider_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("无法读取 Provider 最近活动时间：{error}"))
    }

    fn period(&self, provider_id: &str, start: i64) -> Result<UsagePeriod, String> {
        self.connection.lock().map_err(|_| "用量数据库锁已损坏。".to_string())?
            .query_row("SELECT COUNT(*), COALESCE(SUM(succeeded), 0), SUM(input_tokens), SUM(output_tokens) FROM provider_usage_events WHERE provider_id = ?1 AND created_at >= ?2", params![provider_id, start], |row| Ok(UsagePeriod { requests: row.get(0)?, successes: row.get(1)?, input_tokens: row.get(2)?, output_tokens: row.get(3)?, estimated_cost_usd: None }))
            .map_err(|error| format!("无法读取本地用量：{error}"))
    }
}

fn now() -> i64 {
    Local::now().timestamp()
}
fn start_of_today() -> i64 {
    let now = Local::now();
    Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .unwrap()
        .timestamp()
}
fn start_of_week() -> i64 {
    let today = Local::now().date_naive();
    let monday = today - Duration::days(today.weekday().num_days_from_monday().into());
    Local
        .from_local_datetime(&monday.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap()
        .timestamp()
}
fn start_of_month() -> i64 {
    let now = Local::now();
    Local
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .unwrap()
        .timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usage_is_provider_scoped_and_unknown_tokens_remain_unknown() {
        let store = UsageStore::in_memory();
        store
            .record(
                "a",
                Some("m"),
                true,
                Some(UsageObservation {
                    input_tokens: Some(10),
                    output_tokens: Some(4),
                }),
            )
            .unwrap();
        store.record("a", Some("m"), false, None).unwrap();
        store
            .record(
                "b",
                Some("m"),
                true,
                Some(UsageObservation {
                    input_tokens: Some(99),
                    output_tokens: Some(50),
                }),
            )
            .unwrap();
        let summary = store.summary("a").unwrap();
        assert_eq!(summary.today.requests, 2);
        assert_eq!(summary.today.successes, 1);
        assert_eq!(summary.today.input_tokens, Some(10));
        assert_eq!(summary.today.output_tokens, Some(4));
        assert!(summary.last_request_at.is_some());
    }
}
