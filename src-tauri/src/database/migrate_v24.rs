//! Model exposure preferences survive provider revision/catalog changes; vision
//! evidence is scoped to an immutable provider revision and probe contract.

use rusqlite::{params, Transaction};

pub(super) fn migrate_to_v24(transaction: &Transaction<'_>, backfill: bool) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS provider_model_exposures (
                provider_id TEXT NOT NULL REFERENCES api_key_providers(id) ON DELETE CASCADE,
                upstream_model_id TEXT NOT NULL CHECK(length(trim(upstream_model_id)) > 0),
                exposed INTEGER NOT NULL CHECK(exposed IN (0, 1)),
                updated_at INTEGER NOT NULL,
                PRIMARY KEY(provider_id, upstream_model_id)
             );
             CREATE TABLE IF NOT EXISTS provider_model_image_probes (
                provider_id TEXT NOT NULL REFERENCES api_key_providers(id) ON DELETE CASCADE,
                provider_revision_id TEXT NOT NULL REFERENCES api_key_provider_revisions(id) ON DELETE CASCADE,
                upstream_model_id TEXT NOT NULL CHECK(length(trim(upstream_model_id)) > 0),
                wire_api TEXT NOT NULL CHECK(wire_api IN ('responses', 'chat_completions')),
                contract_version TEXT NOT NULL,
                state TEXT NOT NULL CHECK(state IN ('supported', 'unsupported', 'failed')),
                checked_at INTEGER NOT NULL,
                error_code TEXT,
                PRIMARY KEY(provider_revision_id, upstream_model_id, wire_api, contract_version),
                CHECK((state='supported' AND error_code IS NULL)
                   OR (state<>'supported' AND error_code IS NOT NULL))
             );
             CREATE TRIGGER IF NOT EXISTS model_image_probe_revision_matches_provider
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
        .map_err(|error| format!("model_exposure_migration_failed:{error}"))?;
    if !backfill {
        return Ok(());
    }
    // Older schema tests deliberately contain only the table/column under
    // inspection. Match the existing additive migration convention: there is
    // no usable workspace selection to migrate until all join columns exist.
    for (table, columns) in [
        ("launch_presets", &["id", "entry_kind", "deleted_at"][..]),
        (
            "model_route_identities",
            &[
                "id",
                "source_kind",
                "api_key_provider_id",
                "upstream_model_id",
            ][..],
        ),
        (
            "launch_preset_model_selections",
            &["launch_preset_id", "route_identity_id"][..],
        ),
    ] {
        for column in columns {
            let exists: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
                    params![table, column],
                    |row| row.get(0),
                )
                .map_err(|error| format!("model_exposure_backfill_schema_failed:{error}"))?;
            if !exists {
                return Ok(());
            }
        }
    }
    let rows = {
        let mut statement = transaction
            .prepare(
                "SELECT DISTINCT identity.api_key_provider_id, identity.upstream_model_id
                 FROM launch_preset_model_selections selection
                 JOIN launch_presets preset ON preset.id=selection.launch_preset_id
                 JOIN model_route_identities identity ON identity.id=selection.route_identity_id
                 WHERE preset.deleted_at IS NULL AND preset.entry_kind='managed_app'
                   AND identity.source_kind='api_provider'",
            )
            .map_err(|error| format!("model_exposure_backfill_read_failed:{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("model_exposure_backfill_read_failed:{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("model_exposure_backfill_decode_failed:{error}"))?;
        rows
    };
    let now = crate::support::unix_now_seconds();
    for (provider_id, model_id) in rows {
        if crate::model_exposure::is_selectable_model_id(&model_id) {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO provider_model_exposures
                     (provider_id, upstream_model_id, exposed, updated_at) VALUES (?1, ?2, 1, ?3)",
                    params![provider_id, model_id, now],
                )
                .map_err(|error| format!("model_exposure_backfill_write_failed:{error}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, TransactionBehavior};

    fn legacy_fixture() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE api_key_providers(id TEXT PRIMARY KEY);
             CREATE TABLE api_key_provider_revisions(id TEXT PRIMARY KEY,provider_id TEXT,wire_api TEXT);
             CREATE TABLE launch_presets(id TEXT PRIMARY KEY,name TEXT,entry_kind TEXT,deleted_at INTEGER);
             CREATE TABLE model_route_identities(id TEXT PRIMARY KEY,source_kind TEXT,api_key_provider_id TEXT,upstream_model_id TEXT);
             CREATE TABLE launch_preset_model_selections(launch_preset_id TEXT,route_identity_id TEXT);
             INSERT INTO api_key_providers VALUES ('p');
             INSERT INTO launch_presets VALUES ('a','隔离空间 A','managed_app',NULL),('deleted','Old','managed_app',2);
             INSERT INTO model_route_identities VALUES ('chat','api_provider','p','vision-chat'),('role','api_provider','p','codex-auto-review'),('image','api_provider','p','gpt-image-2'),('old','api_provider','p','old-chat');
             INSERT INTO launch_preset_model_selections VALUES ('a','chat'),('a','role'),('a','image'),('deleted','old');"
        ).unwrap();
        connection
    }

    #[test]
    fn backfills_only_used_ordinary_models_without_touching_source_or_evidence() {
        let mut connection = legacy_fixture();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        migrate_to_v24(&transaction, true).unwrap();
        transaction.commit().unwrap();
        let exposed: Vec<String> = connection
            .prepare("SELECT upstream_model_id FROM provider_model_exposures WHERE exposed=1")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(exposed, ["vision-chat"]);
        let probes: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM provider_model_image_probes",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let selections: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM launch_preset_model_selections",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(probes, 0);
        assert_eq!(selections, 4);
        connection
            .execute("UPDATE provider_model_exposures SET exposed=0", [])
            .unwrap();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        migrate_to_v24(&transaction, false).unwrap();
        transaction.commit().unwrap();
        let still_disabled: bool = connection
            .query_row(
                "SELECT exposed=0 FROM provider_model_exposures",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(still_disabled);
    }

    #[test]
    fn migration_rolls_back_tables_and_backfill_together() {
        let mut connection = legacy_fixture();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        migrate_to_v24(&transaction, true).unwrap();
        transaction.rollback().unwrap();
        let tables: i64 = connection.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('provider_model_exposures','provider_model_image_probes')", [], |row| row.get(0)).unwrap();
        assert_eq!(tables, 0);
        let selections: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM launch_preset_model_selections",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(selections, 4);
    }

    #[test]
    fn probe_evidence_rejects_a_revision_owned_by_another_provider_or_protocol() {
        let mut connection = legacy_fixture();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        migrate_to_v24(&transaction, true).unwrap();
        transaction.commit().unwrap();
        connection.execute_batch("INSERT INTO api_key_providers VALUES ('other'); INSERT INTO api_key_provider_revisions VALUES ('revision','p','responses');").unwrap();
        for (provider, protocol) in [("other", "responses"), ("p", "chat_completions")] {
            assert!(connection.execute(
                "INSERT INTO provider_model_image_probes(provider_id,provider_revision_id,upstream_model_id,wire_api,contract_version,state,checked_at)
                 VALUES (?1,'revision','vision-chat',?2,'image-v1','supported',1)", params![provider, protocol]
            ).is_err());
        }
    }
}
