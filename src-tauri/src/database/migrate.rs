//! Schema 迁移调度和当前版本。
//!
//! 不打开数据库文件，不清理过期行。
//! 由 `connection` 在打开后调用。

use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::migrate_legacy::{migrate_to_v5, migrate_to_v6};
use super::migrate_v7_plus::{
    migrate_to_v10, migrate_to_v11, migrate_to_v12, migrate_to_v13, migrate_to_v14, migrate_to_v15,
    migrate_to_v16, migrate_to_v17, migrate_to_v18, migrate_to_v19, migrate_to_v20, migrate_to_v21,
    migrate_to_v22, migrate_to_v23, migrate_to_v7, migrate_to_v8, migrate_to_v9,
};

pub(super) const CURRENT_SCHEMA_VERSION: i64 = 23;

pub(super) fn migrate(connection: &Connection) -> Result<(), String> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| format!("无法读取 SQLite schema 版本：{error}"))?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "本地数据库 schema 版本 {version} 高于当前支持版本 {CURRENT_SCHEMA_VERSION}。"
        ));
    }

    // v14 rebuilds `launch_presets` to add the provider-less `none` state.
    // Disable FK actions for that atomic rebuild so dependent composition and
    // history rows are not cascaded while the parent table is replaced.
    let rebuild_launch_presets = version < 14;
    if rebuild_launch_presets {
        connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .map_err(|error| format!("无法准备空工作区迁移：{error}"))?;
    }
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|error| format!("无法开始 SQLite 迁移：{error}"))?;
    // Keep the original table shapes compatible with databases created by
    // earlier builds. CREATE IF NOT EXISTS is idempotent for all new tables.
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS api_key_providers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                base_url TEXT NOT NULL,
                secret_ref TEXT NOT NULL UNIQUE,
                is_default INTEGER NOT NULL CHECK(is_default IN (0, 1)),
                transport_mode TEXT NOT NULL DEFAULT 'gateway_passthrough',
                configured INTEGER NOT NULL DEFAULT 1 CHECK(configured IN (0, 1)),
                created_at INTEGER NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS one_default_provider
                ON api_key_providers(is_default) WHERE is_default = 1;
            CREATE TABLE IF NOT EXISTS provider_usage_events (
                id INTEGER PRIMARY KEY,
                provider_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                model TEXT,
                succeeded INTEGER NOT NULL CHECK(succeeded IN (0, 1)),
                input_tokens INTEGER,
                output_tokens INTEGER
            );
            CREATE INDEX IF NOT EXISTS provider_usage_events_provider_time
                ON provider_usage_events(provider_id, created_at);
            CREATE TABLE IF NOT EXISTS gateway_diagnostic_events (
                id INTEGER PRIMARY KEY,
                provider_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                source TEXT NOT NULL,
                route TEXT NOT NULL,
                model TEXT,
                transport_mode TEXT NOT NULL,
                stream INTEGER NOT NULL CHECK(stream IN (0, 1)),
                outcome TEXT NOT NULL,
                http_status INTEGER,
                latency_ms INTEGER,
                error_code TEXT
            );
            CREATE INDEX IF NOT EXISTS gateway_diagnostic_events_provider_time
                ON gateway_diagnostic_events(provider_id, created_at DESC);
            CREATE TABLE IF NOT EXISTS provider_connectivity_reports (
                provider_id TEXT PRIMARY KEY,
                checked_at INTEGER NOT NULL,
                status TEXT NOT NULL,
                model TEXT,
                models_check TEXT NOT NULL,
                text_check TEXT NOT NULL,
                error_code TEXT
            );
            CREATE TABLE IF NOT EXISTS gateway_timeout_settings (
                provider_id TEXT PRIMARY KEY,
                connect_seconds INTEGER NOT NULL,
                first_byte_seconds INTEGER NOT NULL,
                stream_idle_seconds INTEGER NOT NULL,
                non_stream_total_seconds INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS provider_upstream_balances (
                provider_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                available REAL,
                granted REAL,
                used REAL,
                unlimited INTEGER NOT NULL DEFAULT 0 CHECK(unlimited IN (0, 1)),
                currency TEXT,
                unit TEXT NOT NULL DEFAULT 'quota',
                source TEXT NOT NULL,
                refreshed_at INTEGER,
                expires_at INTEGER,
                error TEXT
            );
            CREATE TABLE IF NOT EXISTS provider_migration_state (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS gateway_request_diagnostics (
                id INTEGER PRIMARY KEY,
                instance_id TEXT REFERENCES codex_instances(id) ON DELETE CASCADE,
                session_id TEXT,
                created_at INTEGER NOT NULL,
                route TEXT NOT NULL,
                stream INTEGER NOT NULL CHECK(stream IN (0, 1)),
                wire_api TEXT NOT NULL CHECK(wire_api IN ('responses', 'chat_completions')),
                public_model_id TEXT NOT NULL,
                upstream_model_id TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                provider_revision_id TEXT NOT NULL,
                http_status INTEGER,
                response_content_type TEXT,
                first_byte_ms INTEGER,
                total_latency_ms INTEGER,
                outcome TEXT NOT NULL CHECK(outcome IN ('success', 'timeout', 'interrupted', 'format_error', 'failed')),
                error_code TEXT,
                error_detail TEXT,
                input_length INTEGER NOT NULL DEFAULT 0,
                output_length INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                input_hash TEXT,
                output_hash TEXT,
                tool_call_types TEXT,
                tool_result_hashes TEXT
            );
            CREATE INDEX IF NOT EXISTS gateway_request_diagnostics_instance_time
                ON gateway_request_diagnostics(instance_id, created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS gateway_request_diagnostics_provider_time
                ON gateway_request_diagnostics(provider_id, created_at DESC, id DESC);
            CREATE TABLE IF NOT EXISTS conversation_summaries (
                id INTEGER PRIMARY KEY,
                instance_id TEXT NOT NULL REFERENCES codex_instances(id) ON DELETE CASCADE,
                session_id TEXT,
                created_at INTEGER NOT NULL,
                input_length INTEGER NOT NULL DEFAULT 0,
                output_length INTEGER NOT NULL DEFAULT 0,
                input_hash TEXT,
                output_hash TEXT,
                tool_call_types TEXT,
                tool_result_hashes TEXT
            );
            CREATE INDEX IF NOT EXISTS conversation_summaries_instance_time
                ON conversation_summaries(instance_id, created_at DESC, id DESC);
            CREATE TABLE IF NOT EXISTS provider_credentials (
                secret_ref TEXT PRIMARY KEY,
                api_key TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )
        .map_err(|error| format!("无法执行 SQLite 迁移：{error}"))?;

    // The first shipped schema did not have transport_mode. Existing user
    // databases therefore need this additive, safe migration.
    let has_transport_mode: bool = transaction
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('api_key_providers') WHERE name = 'transport_mode'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("无法检查 Provider 数据库结构：{error}"))?;
    if !has_transport_mode {
        transaction
            .execute(
                "ALTER TABLE api_key_providers ADD COLUMN transport_mode TEXT NOT NULL DEFAULT 'gateway_passthrough'",
                [],
            )
            .map_err(|error| format!("无法迁移 Provider 数据库：{error}"))?;
    }

    // This is non-sensitive metadata. Existing rows predate the field and are
    // treated as configured without enumerating the OS credential store.
    let has_configured: bool = transaction
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('api_key_providers') WHERE name = 'configured'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("无法检查 Provider 凭据状态字段：{error}"))?;
    if !has_configured {
        transaction
            .execute(
                "ALTER TABLE api_key_providers ADD COLUMN configured INTEGER NOT NULL DEFAULT 1 CHECK(configured IN (0, 1))",
                [],
            )
            .map_err(|error| format!("无法迁移 Provider 凭据状态字段：{error}"))?;
    }

    migrate_to_v5(&transaction, version < 5)?;
    if version < 6 {
        migrate_to_v6(&transaction)?;
    }
    if version < 7 {
        migrate_to_v7(&transaction)?;
    }
    if version < 8 {
        migrate_to_v8(&transaction)?;
    }
    if version < 9 {
        migrate_to_v9(&transaction)?;
    }
    if version < 10 {
        migrate_to_v10(&transaction)?;
    }
    if version < 11 {
        migrate_to_v11(&transaction)?;
    }
    if version < 12 {
        migrate_to_v12(&transaction)?;
    }
    if version < 13 {
        migrate_to_v13(&transaction)?;
    }
    if version < 14 {
        migrate_to_v14(&transaction)?;
    }
    // Additive even when user_version is already 15: a previous build could
    // bump the pragma without adding `error_detail`, after which INSERT
    // into the request ledger fails silently.
    migrate_to_v15(&transaction)?;
    // Additive even when user_version is already 16: a previous build could
    // bump the pragma without the selection window columns.
    migrate_to_v16(&transaction)?;
    if version < 17 {
        migrate_to_v17(&transaction)?;
    }
    migrate_to_v18(&transaction)?;
    migrate_to_v19(&transaction)?;
    migrate_to_v20(&transaction)?;
    migrate_to_v21(&transaction)?;
    migrate_to_v22(&transaction, version < 22)?;
    migrate_to_v23(&transaction)?;
    transaction
        .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
        .map_err(|error| format!("无法保存 SQLite schema 版本：{error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("无法提交 SQLite 迁移：{error}"))?;
    if rebuild_launch_presets {
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|error| format!("无法恢复 SQLite 外键约束：{error}"))?;
    }
    Ok(())
}
