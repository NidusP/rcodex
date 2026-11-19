//! Allow `pass_through` transport on provider revisions (Codex header passthrough).
//!
//! Runs outside the main migrate transaction: SQLite cannot toggle `foreign_keys`
//! while a transaction is open, and this migration rebuilds a referenced table.

use rusqlite::{params, Connection};

fn table_exists(connection: &Connection, name: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![name],
            |row| row.get(0),
        )
        .map_err(|error| format!("无法检查 {name}：{error}"))
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
            params![table, column],
            |row| row.get(0),
        )
        .map_err(|error| format!("无法检查 {table}.{column}：{error}"))
}

fn revisions_table_supports_pass_through_migration(
    connection: &Connection,
) -> Result<bool, String> {
    if !table_exists(connection, "api_key_provider_revisions")? {
        return Ok(false);
    }
    for column in [
        "secret_ref",
        "base_url",
        "wire_api",
        "transport_mode",
        "state",
        "created_at",
        "retired_at",
    ] {
        if !column_exists(connection, "api_key_provider_revisions", column)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn scrub_revision_references(connection: &Connection) -> Result<(), String> {
    let revision_ids = "SELECT id FROM api_key_provider_revisions";
    if table_exists(connection, "provider_model_image_probes")? {
        connection
            .execute(
                &format!("DELETE FROM provider_model_image_probes WHERE provider_revision_id NOT IN ({revision_ids})"),
                [],
            )
            .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    }
    if table_exists(connection, "route_set_provider_revisions")? {
        connection
            .execute(
                &format!(
                    "DELETE FROM route_set_provider_revisions WHERE provider_revision_id NOT IN ({revision_ids})"
                ),
                [],
            )
            .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    }
    if table_exists(connection, "provider_model_catalog_revisions")? {
        connection
            .execute(
                &format!(
                    "DELETE FROM provider_model_catalog_revisions WHERE provider_revision_id NOT IN ({revision_ids})"
                ),
                [],
            )
            .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    }
    if table_exists(connection, "codex_instances")? {
        connection
            .execute(
                &format!(
                    "DELETE FROM codex_instances
                     WHERE api_key_provider_revision_id IS NOT NULL
                       AND api_key_provider_revision_id NOT IN ({revision_ids})
                       AND state='exited'"
                ),
                [],
            )
            .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    }
    if column_exists(connection, "api_key_providers", "current_revision_id")? {
        connection
            .execute(
                &format!(
                    "UPDATE api_key_providers SET current_revision_id=NULL
                     WHERE current_revision_id IS NOT NULL
                       AND current_revision_id NOT IN ({revision_ids})"
                ),
                [],
            )
            .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    }
    Ok(())
}

pub(super) fn migrate_to_v25(connection: &Connection) -> Result<(), String> {
    if !revisions_table_supports_pass_through_migration(connection)? {
        return Ok(());
    }
    let table_sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='api_key_provider_revisions'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    if table_sql.contains("'pass_through'") {
        return Ok(());
    }
    connection
        .execute_batch(
            "PRAGMA foreign_keys=OFF;
             DROP TRIGGER IF EXISTS api_key_provider_current_revision_matches_provider;
             DROP TRIGGER IF EXISTS model_image_probe_revision_matches_provider;
             DROP TRIGGER IF EXISTS route_set_provider_revision_matches_provider;
             DROP TRIGGER IF EXISTS provider_catalog_revision_matches_provider;
             UPDATE api_key_provider_revisions
             SET transport_mode='gateway_passthrough'
             WHERE transport_mode NOT IN (
                'direct_responses', 'gateway_passthrough', 'gateway_chat_completions'
             );
             UPDATE api_key_provider_revisions
             SET retired_at=created_at
             WHERE state='retired' AND retired_at IS NULL;
             UPDATE api_key_provider_revisions
             SET retired_at=NULL
             WHERE state<>'retired';
             DELETE FROM api_key_provider_revisions
             WHERE provider_id NOT IN (SELECT id FROM api_key_providers);
             CREATE TABLE api_key_provider_revisions_v25 (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL REFERENCES api_key_providers(id) ON DELETE RESTRICT,
                secret_ref TEXT NOT NULL UNIQUE,
                base_url TEXT NOT NULL,
                wire_api TEXT NOT NULL CHECK(wire_api IN ('responses', 'chat_completions')),
                transport_mode TEXT NOT NULL CHECK(
                    transport_mode IN (
                        'direct_responses',
                        'gateway_passthrough',
                        'gateway_chat_completions',
                        'pass_through'
                    )
                ),
                state TEXT NOT NULL CHECK(state IN ('active', 'credential_missing', 'retired')),
                created_at INTEGER NOT NULL,
                retired_at INTEGER,
                CHECK((state = 'retired') = (retired_at IS NOT NULL))
             );
             INSERT INTO api_key_provider_revisions_v25
             SELECT old.*
             FROM api_key_provider_revisions old
             INNER JOIN api_key_providers provider ON provider.id = old.provider_id;
             DROP TABLE api_key_provider_revisions;
             ALTER TABLE api_key_provider_revisions_v25 RENAME TO api_key_provider_revisions;
             CREATE INDEX IF NOT EXISTS api_key_provider_revisions_provider_time
                ON api_key_provider_revisions(provider_id, created_at DESC);
             CREATE TRIGGER IF NOT EXISTS api_key_provider_current_revision_matches_provider
             BEFORE UPDATE OF current_revision_id ON api_key_providers
             WHEN NEW.current_revision_id IS NOT NULL AND NOT EXISTS (
                SELECT 1 FROM api_key_provider_revisions
                WHERE id = NEW.current_revision_id AND provider_id = NEW.id
             )
             BEGIN
                SELECT RAISE(ABORT, 'provider current revision mismatch');
             END;",
        )
        .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    scrub_revision_references(connection)?;
    connection
        .execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    if table_exists(connection, "provider_model_image_probes")? {
        connection
            .execute_batch(
                "CREATE TRIGGER IF NOT EXISTS model_image_probe_revision_matches_provider
                 BEFORE INSERT ON provider_model_image_probes
                 WHEN NOT EXISTS (
                    SELECT 1 FROM api_key_provider_revisions
                    WHERE id=NEW.provider_revision_id AND provider_id=NEW.provider_id
                      AND wire_api=NEW.wire_api
                 )
                 BEGIN
                    SELECT RAISE(ABORT, 'model image probe revision mismatch');
                 END;",
            )
            .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    }
    if table_exists(connection, "route_set_provider_revisions")? {
        connection
            .execute_batch(
                "CREATE TRIGGER IF NOT EXISTS route_set_provider_revision_matches_provider
                 BEFORE INSERT ON route_set_provider_revisions
                 WHEN NOT EXISTS (
                    SELECT 1 FROM api_key_provider_revisions
                    WHERE id = NEW.provider_revision_id AND provider_id = NEW.provider_id
                 )
                 BEGIN
                    SELECT RAISE(ABORT, 'route set provider revision mismatch');
                 END;",
            )
            .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    }
    if table_exists(connection, "provider_model_catalog_revisions")? {
        connection
            .execute_batch(
                "CREATE TRIGGER IF NOT EXISTS provider_catalog_revision_matches_provider
                 BEFORE INSERT ON provider_model_catalog_revisions
                 WHEN NOT EXISTS (
                    SELECT 1 FROM api_key_provider_revisions
                    WHERE id = NEW.provider_revision_id AND provider_id = NEW.provider_id
                 )
                 BEGIN
                    SELECT RAISE(ABORT, 'model catalog provider revision mismatch');
                 END;",
            )
            .map_err(|error| format!("pass_through_transport_migration_failed:{error}"))?;
    }
    Ok(())
}
