//! Database 连接、迁移和清理回归。

use super::*;
use uuid::Uuid;

#[test]
fn migration_is_idempotent_and_configures_connection() {
    let database = Database::in_memory();
    let connection = database.connection.lock().unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let foreign_keys: i64 = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    let balance_table: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'provider_upstream_balances'",
                [],
                |row| row.get(0),
            )
            .unwrap();
    let v5_table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN (
                    'api_key_provider_revisions', 'launch_presets',
                    'codex_accounts', 'official_credential_revisions',
                    'codex_authorization_attempts',
                    'launch_configuration_revisions',
                    'launch_configuration_dependencies', 'codex_instances',
                    'runtime_configuration_locks', 'resource_operation_leases',
                    'gateway_revisions', 'codex_quota_snapshots'
                 )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let v6_table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN (
                    'launch_preset_compositions', 'launch_preset_api_providers',
                    'launch_preset_model_selections', 'model_route_identities',
                    'provider_model_catalog_revisions', 'provider_model_catalog_entries',
                    'gateway_service_revisions', 'route_set_revisions',
                    'route_set_provider_revisions', 'model_routes',
                    'launch_configuration_route_sets', 'instance_gateway_bindings'
                 )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let v9_role_table: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'launch_preset_role_models'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let deleted_at_column: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('launch_presets')
                 WHERE name = 'deleted_at'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
    assert_eq!(foreign_keys, 1);
    assert_eq!(balance_table, 1);
    assert_eq!(v5_table_count, 12);
    assert_eq!(v6_table_count, 12);
    let provider_credentials_table: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'provider_credentials'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(provider_credentials_table, 1);
    let runtime_binding_table: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'launch_preset_runtime_bindings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(runtime_binding_table, 1);
    let compatibility_binding_table: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'compatibility_gateway_bindings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(compatibility_binding_table, 1);
    assert_eq!(v9_role_table, 1);
    assert_eq!(deleted_at_column, 1);
    drop(connection);
    let connection = database.connection.lock().unwrap();
    migrate(&connection).unwrap();
}

#[test]
fn migration_v21_drops_unique_entry_kind_index() {
    let database = Database::in_memory();
    let connection = database.connection.lock().unwrap();
    let exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='index' AND name='launch_presets_entry_kind'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(exists, 0);
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn migration_v20_renames_only_the_system_generated_preset() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE launch_presets (name TEXT NOT NULL);
                 INSERT INTO launch_presets VALUES ('3R Codex'), ('My 3R Codex setup');",
        )
        .unwrap();
    let transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Immediate).unwrap();
    migrate_to_v20(&transaction).unwrap();
    transaction.commit().unwrap();
    let names = connection
        .prepare("SELECT name FROM launch_presets ORDER BY rowid")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(names, vec!["R Codex", "My 3R Codex setup"]);
}

#[test]
fn migration_rejects_future_schema_versions() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch("PRAGMA user_version = 99;")
        .unwrap();
    let error = migrate(&connection).unwrap_err();
    assert!(error.contains("高于当前支持版本"));
}

#[test]
fn migration_v11_adds_stable_launch_preset_runtime_binding() {
    let database = Database::in_memory();
    let connection = database.connection.lock().unwrap();
    connection
        .execute_batch(
            "DROP TABLE launch_preset_runtime_bindings;
                 PRAGMA user_version = 10;",
        )
        .unwrap();
    migrate(&connection).unwrap();
    let exists: bool = connection
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master
                 WHERE type='table' AND name='launch_preset_runtime_bindings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(exists);
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn migration_v8_adds_instance_request_usage_columns() {
    let connection = Connection::open_in_memory().unwrap();
    configure_connection(&connection).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE model_route_identities (
                    id TEXT PRIMARY KEY
                );
                CREATE TABLE launch_preset_model_selections (
                    launch_preset_id TEXT NOT NULL,
                    route_identity_id TEXT NOT NULL
                );
                CREATE TABLE gateway_request_diagnostics (
                    id INTEGER PRIMARY KEY,
                    instance_id TEXT,
                    created_at INTEGER NOT NULL,
                    route TEXT NOT NULL,
                    stream INTEGER NOT NULL,
                    wire_api TEXT NOT NULL,
                    public_model_id TEXT NOT NULL,
                    upstream_model_id TEXT NOT NULL,
                    provider_id TEXT NOT NULL,
                    provider_revision_id TEXT NOT NULL,
                    outcome TEXT NOT NULL,
                    input_length INTEGER NOT NULL DEFAULT 0,
                    output_length INTEGER NOT NULL DEFAULT 0,
                    input_hash TEXT,
                    output_hash TEXT,
                    tool_call_types TEXT,
                    tool_result_hashes TEXT
                );
                PRAGMA user_version = 7;",
        )
        .unwrap();
    migrate(&connection).unwrap();
    for column in ["input_tokens", "output_tokens", "total_tokens"] {
        let exists: bool = connection
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('gateway_request_diagnostics') WHERE name = ?1",
                    [column],
                    |row| row.get(0),
                )
                .unwrap();
        assert!(exists, "missing {column}");
    }
}

#[test]
fn migration_adds_error_detail_even_when_user_version_is_already_15() {
    let connection = Connection::open_in_memory().unwrap();
    configure_connection(&connection).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE gateway_request_diagnostics (
                    id INTEGER PRIMARY KEY,
                    instance_id TEXT,
                    created_at INTEGER NOT NULL,
                    route TEXT NOT NULL,
                    stream INTEGER NOT NULL,
                    wire_api TEXT NOT NULL,
                    public_model_id TEXT NOT NULL,
                    upstream_model_id TEXT NOT NULL,
                    provider_id TEXT NOT NULL,
                    provider_revision_id TEXT NOT NULL,
                    outcome TEXT NOT NULL,
                    input_length INTEGER NOT NULL DEFAULT 0,
                    output_length INTEGER NOT NULL DEFAULT 0
                );
                PRAGMA user_version = 15;",
        )
        .unwrap();
    migrate(&connection).unwrap();
    let exists: bool = connection
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('gateway_request_diagnostics') WHERE name = 'error_detail'",
                [],
                |row| row.get(0),
            )
            .unwrap();
    assert!(exists);
}

#[test]
fn migration_v16_adds_nullable_selection_window_columns() {
    let connection = Connection::open_in_memory().unwrap();
    configure_connection(&connection).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE gateway_request_diagnostics (
                    id INTEGER PRIMARY KEY,
                    instance_id TEXT,
                    created_at INTEGER NOT NULL,
                    route TEXT NOT NULL,
                    stream INTEGER NOT NULL,
                    wire_api TEXT NOT NULL,
                    public_model_id TEXT NOT NULL,
                    upstream_model_id TEXT NOT NULL,
                    provider_id TEXT NOT NULL,
                    provider_revision_id TEXT NOT NULL,
                    outcome TEXT NOT NULL,
                    input_length INTEGER NOT NULL DEFAULT 0,
                    output_length INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE launch_preset_model_selections (
                    launch_preset_id TEXT NOT NULL,
                    route_identity_id TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    display_name_override TEXT
                );
                INSERT INTO launch_preset_model_selections (
                    launch_preset_id, route_identity_id, position, display_name_override
                ) VALUES ('preset-a', 'identity-a', 0, 'Grok');
                PRAGMA user_version = 15;",
        )
        .unwrap();
    migrate(&connection).unwrap();
    for column in ["context_window", "auto_compact_token_limit"] {
        let exists: bool = connection
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('launch_preset_model_selections') WHERE name = ?1",
                    [column],
                    |row| row.get(0),
                )
                .unwrap();
        assert!(exists, "missing {column}");
    }
    let (context_window, auto_compact_token_limit): (Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT context_window, auto_compact_token_limit
                 FROM launch_preset_model_selections
                 WHERE launch_preset_id='preset-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(context_window, None);
    assert_eq!(auto_compact_token_limit, None);
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn migration_v18_adds_nullable_reasoning_columns_and_preserves_old_rows() {
    let connection = Connection::open_in_memory().unwrap();
    configure_connection(&connection).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE gateway_request_diagnostics (
                    id INTEGER PRIMARY KEY,
                    instance_id TEXT,
                    created_at INTEGER NOT NULL,
                    route TEXT NOT NULL,
                    stream INTEGER NOT NULL,
                    wire_api TEXT NOT NULL,
                    public_model_id TEXT NOT NULL,
                    upstream_model_id TEXT NOT NULL,
                    provider_id TEXT NOT NULL,
                    provider_revision_id TEXT NOT NULL,
                    outcome TEXT NOT NULL,
                    input_length INTEGER NOT NULL DEFAULT 0,
                    output_length INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE launch_preset_model_selections (
                    launch_preset_id TEXT NOT NULL,
                    route_identity_id TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    display_name_override TEXT,
                    context_window INTEGER,
                    auto_compact_token_limit INTEGER
                );
                INSERT INTO launch_preset_model_selections (
                    launch_preset_id, route_identity_id, position, display_name_override
                ) VALUES ('preset-a', 'identity-a', 0, 'Legacy');
                PRAGMA user_version = 17;",
        )
        .unwrap();
    migrate(&connection).unwrap();
    for column in ["reasoning_levels_json", "default_reasoning_level"] {
        let exists: bool = connection
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('launch_preset_model_selections') WHERE name = ?1",
                    [column],
                    |row| row.get(0),
                )
                .unwrap();
        assert!(exists, "missing {column}");
    }
    let values: (Option<String>, Option<String>) = connection
        .query_row(
            "SELECT reasoning_levels_json, default_reasoning_level
                 FROM launch_preset_model_selections WHERE launch_preset_id='preset-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(values, (None, None));
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn migration_v17_retires_only_compatibility_instances_and_releases_their_locks() {
    let connection = Connection::open_in_memory().unwrap();
    configure_connection(&connection).unwrap();
    migrate(&connection).unwrap();
    connection.execute_batch(
            "INSERT INTO api_key_providers (id,name,base_url,secret_ref,is_default,configured,created_at)
             VALUES ('provider','Provider','https://example.com','key',0,1,1);
             INSERT INTO api_key_provider_revisions (id,provider_id,secret_ref,base_url,wire_api,transport_mode,state,created_at)
             VALUES ('revision','provider','key','https://example.com','responses','gateway_passthrough','active',1);"
        ).unwrap();
    for target in ["compat_app", "compat_cli", "managed_app", "cli"] {
        connection.execute(
                "INSERT INTO launch_configuration_revisions (id,credential_kind,profile_snapshot_hash,config_snapshot,codex_executable,codex_version,created_at)
                 VALUES (?1,'api_key','hash',?2,'/never-read','test',1)",
                rusqlite::params![target, serde_json::json!({"launchTarget":target}).to_string()],
            ).unwrap();
        connection.execute(
                "INSERT INTO codex_instances (id,launch_configuration_revision_id,credential_kind,api_key_provider_revision_id,managed_codex_home,lifecycle_dir,launch_nonce,state)
                 VALUES (?1,?1,'api_key','revision','/never-read',?1,?1,'running')", [target],
            ).unwrap();
        connection.execute(
                "INSERT INTO runtime_configuration_locks (instance_id,launch_configuration_revision_id,acquired_at) VALUES (?1,?1,1)", [target],
            ).unwrap();
    }
    connection.pragma_update(None, "user_version", 16).unwrap();
    migrate(&connection).unwrap();
    migrate(&connection).unwrap();
    for target in ["compat_app", "compat_cli", "managed_app", "cli"] {
        let (state, released): (String,bool) = connection.query_row(
                "SELECT instance.state, lock.released_at IS NOT NULL FROM codex_instances instance
                 JOIN runtime_configuration_locks lock ON lock.instance_id=instance.id WHERE instance.id=?1",
                [target], |row| Ok((row.get(0)?, row.get(1)?)),
            ).unwrap();
        let retired = target.starts_with("compat_");
        assert_eq!(state, if retired { "exited" } else { "running" });
        assert_eq!(released, retired);
    }
}

#[test]
fn migration_v9_adds_role_bindings_and_enforces_selected_model() {
    let database = Database::in_memory();
    let connection = database.connection.lock().unwrap();
    connection
        .execute_batch(
            "DROP TABLE launch_preset_role_models;
                 PRAGMA user_version = 8;",
        )
        .unwrap();
    migrate(&connection).unwrap();

    connection
        .execute_batch(
            "INSERT INTO api_key_providers (
                    id, name, base_url, secret_ref, is_default,
                    transport_mode, configured, created_at, updated_at
                 ) VALUES ('provider-role', 'Role', 'https://example.com/v1',
                    'provider:role', 0, 'gateway_passthrough', 1, 1, 1);
                 INSERT INTO launch_presets (
                    id, name, credential_kind, api_key_provider_id,
                    gateway_policy, profile_options, version, is_preferred,
                    created_at, updated_at
                 ) VALUES ('preset-role', 'Role', 'api_key', 'provider-role',
                    'gateway_passthrough', '{}', 1, 0, 1, 1);
                 INSERT INTO launch_preset_compositions (
                    launch_preset_id, state, created_at, updated_at
                 ) VALUES ('preset-role', 'ready', 1, 1);
                 INSERT INTO launch_preset_api_providers (
                    launch_preset_id, provider_id, position
                 ) VALUES ('preset-role', 'provider-role', 0);
                 INSERT INTO model_route_identities (
                    id, public_model_id, source_kind, api_key_provider_id,
                    upstream_model_id, created_at
                 ) VALUES ('identity-role', 'grok-review', 'api_provider',
                    'provider-role', 'grok-review', 1);",
        )
        .unwrap();
    let rejected = connection.execute(
        "INSERT INTO launch_preset_role_models (
                launch_preset_id, role, route_identity_id
             ) VALUES ('preset-role', 'auto_review', 'identity-role')",
        [],
    );
    assert!(rejected.is_err());
    connection
        .execute_batch(
            "INSERT INTO launch_preset_model_selections (
                    launch_preset_id, route_identity_id, position
                 ) VALUES ('preset-role', 'identity-role', 0);
                 INSERT INTO launch_preset_role_models (
                    launch_preset_id, role, route_identity_id
                 ) VALUES ('preset-role', 'auto_review', 'identity-role');",
        )
        .unwrap();
    assert!(connection
        .execute(
            "DELETE FROM launch_preset_model_selections
                 WHERE launch_preset_id='preset-role' AND route_identity_id='identity-role'",
            [],
        )
        .is_err());
}

#[test]
fn migration_adds_transport_mode_to_legacy_provider_table() {
    let path = std::env::temp_dir().join(format!("3rcodex-migration-{}.sqlite3", Uuid::new_v4()));
    {
        let connection = Connection::open(&path).unwrap();
        connection
                .execute_batch(
                    "CREATE TABLE api_key_providers (
                        id TEXT PRIMARY KEY,
                        name TEXT NOT NULL,
                        base_url TEXT NOT NULL,
                        secret_ref TEXT NOT NULL UNIQUE,
                        is_default INTEGER NOT NULL CHECK(is_default IN (0, 1)),
                        created_at INTEGER NOT NULL
                    );
                    INSERT INTO api_key_providers (id, name, base_url, secret_ref, is_default, created_at)
                    VALUES ('legacy', 'Legacy', 'https://example.com/v1', 'provider:legacy', 1, 1);",
                )
                .unwrap();
    }
    let database = Database::open_path(&path).unwrap();
    let connection = database.connection.lock().unwrap();
    let transport_mode: String = connection
        .query_row(
            "SELECT transport_mode FROM api_key_providers WHERE id = 'legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(transport_mode, "gateway_passthrough");
    let configured: bool = connection
        .query_row(
            "SELECT configured FROM api_key_providers WHERE id = 'legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!configured);
    let revision: (String, String, String) = connection
        .query_row(
            "SELECT providers.current_revision_id, revisions.secret_ref, revisions.state
                 FROM api_key_providers providers
                 JOIN api_key_provider_revisions revisions
                   ON revisions.id = providers.current_revision_id
                 WHERE providers.id = 'legacy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(revision.0, "v4-provider:legacy:revision:1");
    assert_eq!(revision.1, "provider:legacy");
    assert_eq!(revision.2, "credential_missing");
    let preset: (String, bool) = connection
        .query_row(
            "SELECT api_key_provider_id, is_preferred
                 FROM launch_presets WHERE id = 'v4-provider:legacy:preset'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(preset, ("legacy".to_string(), true));
    drop(connection);
    drop(database);
    let _ = std::fs::remove_file(path);
}

#[test]
fn reopening_a_tombstoned_provider_does_not_recreate_its_current_revision() {
    let path = std::env::temp_dir().join(format!(
        "3rcodex-tombstoned-provider-{}.sqlite3",
        Uuid::new_v4()
    ));
    {
        let database = Database::open_path(&path).unwrap();
        let connection = database.connection.lock().unwrap();
        connection
            .execute_batch(
                "INSERT INTO api_key_providers (
                        id, name, base_url, secret_ref, is_default,
                        transport_mode, configured, created_at, current_revision_id, updated_at
                     ) VALUES (
                        'retired-provider', 'Retired Provider', 'https://example.com/v1',
                        'provider:retired:revision:1', 0, 'gateway_passthrough', 0, 1, NULL, 2
                     );
                     INSERT INTO api_key_provider_revisions (
                        id, provider_id, secret_ref, base_url, wire_api, transport_mode,
                        state, created_at, retired_at
                     ) VALUES (
                        'retired-provider-revision', 'retired-provider',
                        'provider:retired:revision:1', 'https://example.com/v1', 'responses',
                        'gateway_passthrough', 'retired', 1, 2
                     );",
            )
            .unwrap();
    }

    let reopened = Database::open_path(&path).unwrap();
    let connection = reopened.connection.lock().unwrap();
    let current_revision: Option<String> = connection
        .query_row(
            "SELECT current_revision_id FROM api_key_providers WHERE id='retired-provider'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(current_revision, None);
    drop(connection);
    drop(reopened);
    let _ = std::fs::remove_file(path);
}

#[test]
fn migration_backfills_multiple_v4_providers_without_reading_secrets() {
    let path = std::env::temp_dir().join(format!("3rcodex-v4-multiple-{}.sqlite3", Uuid::new_v4()));
    {
        let connection = Connection::open(&path).unwrap();
        connection
                .execute_batch(
                    "CREATE TABLE api_key_providers (
                        id TEXT PRIMARY KEY,
                        name TEXT NOT NULL,
                        base_url TEXT NOT NULL,
                        secret_ref TEXT NOT NULL UNIQUE,
                        is_default INTEGER NOT NULL CHECK(is_default IN (0, 1)),
                        transport_mode TEXT NOT NULL DEFAULT 'gateway_passthrough',
                        configured INTEGER NOT NULL DEFAULT 1 CHECK(configured IN (0, 1)),
                        created_at INTEGER NOT NULL
                    );
                    CREATE UNIQUE INDEX one_default_provider
                        ON api_key_providers(is_default) WHERE is_default = 1;
                    INSERT INTO api_key_providers VALUES
                        ('a', 'Provider A', 'https://a.example/v1', 'provider:a', 1, 'direct_responses', 1, 10),
                        ('b', 'Provider B', 'https://b.example/v1', 'provider:b', 0, 'gateway_chat_completions', 0, 20);
                    PRAGMA user_version = 4;",
                )
                .unwrap();
    }

    let database = Database::open_path(&path).unwrap();
    let connection = database.connection.lock().unwrap();
    let revisions = connection
        .prepare(
            "SELECT provider_id, id, secret_ref, wire_api, state
                 FROM api_key_provider_revisions ORDER BY provider_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        revisions,
        vec![
            (
                "a".to_string(),
                "v4-provider:a:revision:1".to_string(),
                "provider:a".to_string(),
                "responses".to_string(),
                "credential_missing".to_string(),
            ),
            (
                "b".to_string(),
                "v4-provider:b:revision:1".to_string(),
                "provider:b".to_string(),
                "chat_completions".to_string(),
                "credential_missing".to_string(),
            ),
        ]
    );
    let preferred: String = connection
        .query_row(
            "SELECT api_key_provider_id FROM launch_presets WHERE is_preferred = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preferred, "a");
    let compositions = connection
        .prepare(
            "SELECT composition.launch_preset_id, composition.state, binding.provider_id
                 FROM launch_preset_compositions composition
                 JOIN launch_preset_api_providers binding
                   ON binding.launch_preset_id=composition.launch_preset_id
                 ORDER BY binding.provider_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        compositions,
        vec![
            (
                "v4-provider:a:preset".to_string(),
                "needs_model_selection".to_string(),
                "a".to_string(),
            ),
            (
                "v4-provider:b:preset".to_string(),
                "needs_model_selection".to_string(),
                "b".to_string(),
            ),
        ]
    );
    drop(connection);

    let reopened = Database::open_path(&path).unwrap();
    let connection = reopened.connection.lock().unwrap();
    let revision_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM api_key_provider_revisions",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let preset_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM launch_presets", [], |row| row.get(0))
        .unwrap();
    assert_eq!(revision_count, 2);
    assert_eq!(preset_count, 2);
    drop(connection);
    drop(reopened);
    drop(database);
    let _ = std::fs::remove_file(path);
}

#[test]
fn v5_migration_rolls_back_schema_and_version_on_backfill_failure() {
    let connection = Connection::open_in_memory().unwrap();
    configure_connection(&connection).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE api_key_providers (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    base_url TEXT NOT NULL,
                    is_default INTEGER NOT NULL,
                    created_at INTEGER NOT NULL
                );
                INSERT INTO api_key_providers VALUES
                    ('broken', 'Broken', 'https://example.com/v1', 1, 1);
                PRAGMA user_version = 4;",
        )
        .unwrap();

    assert!(migrate(&connection).is_err());
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let v5_table_exists: bool = connection
        .query_row(
            "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master
                    WHERE type = 'table' AND name = 'api_key_provider_revisions'
                 )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let transport_mode_exists: bool = connection
        .query_row(
            "SELECT EXISTS(
                    SELECT 1 FROM pragma_table_info('api_key_providers')
                    WHERE name = 'transport_mode'
                 )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 4);
    assert!(!v5_table_exists);
    assert!(!transport_mode_exists);
}

#[test]
fn v5_presets_backfill_optional_account_and_provider_without_guessing_models() {
    let database = Database::in_memory();
    let connection = database.connection.lock().unwrap();
    connection
        .execute_batch(
            "INSERT INTO codex_accounts (
                    id, masked_identity, workspace_label, plan_label,
                    current_credential_revision_id, created_at, updated_at
                 ) VALUES ('account-a', 'a***@example.com', NULL, 'Plus', NULL, 1, 1);
                 INSERT INTO api_key_providers (
                    id, name, base_url, secret_ref, is_default,
                    transport_mode, configured, created_at, current_revision_id, updated_at
                 ) VALUES (
                    'provider-a', 'Provider A', 'https://example.com/v1',
                    'provider:a', 1, 'gateway_passthrough', 1, 1, NULL, 1
                 );
                 INSERT INTO api_key_provider_revisions (
                    id, provider_id, secret_ref, base_url, wire_api,
                    transport_mode, state, created_at
                 ) VALUES (
                    'provider-revision-a', 'provider-a', 'provider:a',
                    'https://example.com/v1', 'responses',
                    'gateway_passthrough', 'active', 1
                 );
                 UPDATE api_key_providers
                    SET current_revision_id='provider-revision-a'
                    WHERE id='provider-a';
                 INSERT INTO launch_presets (
                    id, name, credential_kind, api_key_provider_id,
                    codex_account_id, gateway_policy, profile_options,
                    default_workspace_path, version, is_preferred,
                    created_at, updated_at
                 ) VALUES
                    ('preset-api', 'API', 'api_key', 'provider-a', NULL,
                     'gateway_passthrough', '{}', NULL, 1, 1, 1, 1),
                    ('preset-official', 'Official', 'official', NULL, 'account-a',
                     NULL, '{}', NULL, 1, 0, 1, 1);
                 PRAGMA user_version = 5;",
        )
        .unwrap();
    migrate(&connection).unwrap();

    let compositions = connection
        .prepare(
            "SELECT launch_preset_id, official_account_id, state
                 FROM launch_preset_compositions ORDER BY launch_preset_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        compositions,
        vec![
            (
                "preset-api".to_string(),
                None,
                "needs_model_selection".to_string(),
            ),
            (
                "preset-official".to_string(),
                Some("account-a".to_string()),
                "needs_model_selection".to_string(),
            ),
        ]
    );
    let provider_binding: (String, i64) = connection
        .query_row(
            "SELECT provider_id, position FROM launch_preset_api_providers
                 WHERE launch_preset_id='preset-api'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(provider_binding, ("provider-a".to_string(), 0));
    let selection_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM launch_preset_model_selections",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(selection_count, 0);
}

#[test]
fn v6_migration_rolls_back_schema_and_version_on_overlay_failure() {
    let connection = Connection::open_in_memory().unwrap();
    configure_connection(&connection).unwrap();
    migrate(&connection).unwrap();
    connection
        .execute_batch(
            "DROP TABLE instance_gateway_bindings;
                 DROP TABLE launch_configuration_route_sets;
                 DROP TABLE model_routes;
                 DROP TABLE route_set_provider_revisions;
                 DROP TABLE route_set_revisions;
                 DROP TABLE gateway_service_revisions;
                 DROP TABLE provider_model_catalog_entries;
                 DROP TABLE provider_model_catalog_revisions;
                 DROP TABLE launch_preset_model_selections;
                 DROP TABLE model_route_identities;
                 DROP TABLE launch_preset_api_providers;
                 DROP TABLE launch_preset_compositions;
                 CREATE TABLE launch_preset_compositions (broken TEXT);
                 PRAGMA user_version = 5;",
        )
        .unwrap();

    assert!(migrate(&connection).is_err());
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let route_set_table_exists: bool = connection
        .query_row(
            "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master
                    WHERE type='table' AND name='route_set_revisions'
                 )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 5);
    assert!(!route_set_table_exists);
}

#[test]
fn v6_resource_tables_accept_new_route_resource_kinds() {
    let database = Database::in_memory();
    let connection = database.connection.lock().unwrap();
    for (index, kind) in [
        "route_set_revision",
        "model_route",
        "model_route_identity",
        "gateway_service_revision",
        "model_catalog_revision",
        "instance_gateway_binding",
    ]
    .into_iter()
    .enumerate()
    {
        connection
            .execute(
                "INSERT INTO resource_operation_leases (
                        resource_kind, resource_id, operation_id,
                        operation_kind, state, acquired_at
                     ) VALUES (?1, ?2, ?3, 'test', 'active', 1)",
                rusqlite::params![kind, format!("resource-{index}"), format!("op-{index}")],
            )
            .unwrap();
    }
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM resource_operation_leases",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 6);
}

#[test]
fn retention_prunes_old_diagnostics_and_usage() {
    let database = Database::in_memory();
    let connection = database.connection.lock().unwrap();
    connection
            .execute("INSERT INTO provider_usage_events (provider_id, created_at, succeeded) VALUES ('p', 1, 1)", [])
            .unwrap();
    connection
            .execute("INSERT INTO gateway_diagnostic_events (provider_id, created_at, source, route, transport_mode, stream, outcome) VALUES ('p', 1, 'gateway', '/v1/responses', 'gateway_passthrough', 0, 'success')", [])
            .unwrap();
    prune_expired(&connection).unwrap();
    let usage_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM provider_usage_events", [], |row| {
            row.get(0)
        })
        .unwrap();
    let diagnostic_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM gateway_diagnostic_events",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(usage_count, 0);
    assert_eq!(diagnostic_count, 0);
}

#[cfg(unix)]
#[test]
fn open_path_restricts_database_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!("rcodex-db-permissions-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join(DATABASE_FILENAME);

    let database = Database::open_path(&path).unwrap();
    drop(database);

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn open_path_ignores_legacy_app_data_and_runtime_sockets() {
    use std::os::unix::net::UnixListener;

    let root = std::env::temp_dir().join(format!("rcodex-fresh-start-{}", Uuid::new_v4()));
    let old_dir = root.join("io.3r.codex");
    let new_dir = root.join("io.rcodex");
    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::create_dir_all(&new_dir).unwrap();
    std::fs::write(old_dir.join("3rcodex.sqlite3"), b"legacy-db").unwrap();
    let socket_path =
        std::path::PathBuf::from(format!("/tmp/rcodex-socket-{}", Uuid::new_v4().simple()));
    {
        let _socket = UnixListener::bind(&socket_path).unwrap();
        std::os::unix::fs::symlink(&socket_path, old_dir.join("S")).unwrap();

        let database = Database::open_path(new_dir.join(DATABASE_FILENAME)).unwrap();

        assert!(database.path().is_file());
        assert_eq!(
            std::fs::read(old_dir.join("3rcodex.sqlite3")).unwrap(),
            b"legacy-db"
        );
        assert!(!new_dir.join(".rcodex-migration-complete").exists());
    }
    let _ = std::fs::remove_file(socket_path);
    std::fs::remove_dir_all(root).unwrap();
}
