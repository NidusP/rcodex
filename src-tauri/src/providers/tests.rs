//! ProviderStore 行为测试。
//!
//! MemoryVault 和测试辅助留在本文件。不改断言。

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    },
};

use super::*;

#[derive(Default)]
struct MemoryVault {
    keys: Mutex<HashMap<String, String>>,
    load_count: AtomicUsize,
    save_count: AtomicUsize,
    clear_count: AtomicUsize,
    fail_clear: AtomicBool,
}

impl MemoryVault {
    fn load_count(&self) -> usize {
        self.load_count.load(Ordering::SeqCst)
    }

    fn reset_load_count(&self) {
        self.load_count.store(0, Ordering::SeqCst);
    }

    fn reset_write_counts(&self) {
        self.save_count.store(0, Ordering::SeqCst);
        self.clear_count.store(0, Ordering::SeqCst);
    }

    fn write_count(&self) -> usize {
        self.save_count.load(Ordering::SeqCst) + self.clear_count.load(Ordering::SeqCst)
    }

    fn fail_next_clear(&self) {
        self.fail_clear.store(true, Ordering::SeqCst);
    }
}

impl ApiKeyVault for MemoryVault {
    fn load(&self, secret_ref: &str) -> Result<Option<ApiKey>, String> {
        self.load_count.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .keys
            .lock()
            .unwrap()
            .get(secret_ref)
            .map(|key| ApiKey::for_test(key)))
    }
    fn save(&self, secret_ref: &str, api_key: &ApiKey) -> Result<(), String> {
        self.save_count.fetch_add(1, Ordering::SeqCst);
        self.keys
            .lock()
            .unwrap()
            .insert(secret_ref.to_string(), api_key.expose().to_string());
        Ok(())
    }
    fn clear(&self, secret_ref: &str) -> Result<(), String> {
        self.clear_count.fetch_add(1, Ordering::SeqCst);
        if self.fail_clear.swap(false, Ordering::SeqCst) {
            return Err("forced vault clear failure".to_string());
        }
        self.keys.lock().unwrap().remove(secret_ref);
        Ok(())
    }
}

fn input(name: &str, key: &str) -> CreateProviderInput {
    CreateProviderInput {
        name: name.to_string(),
        base_url: OPENAI_BASE_URL.to_string(),
        api_key: key.to_string(),
        transport_mode: TransportMode::GatewayPassthrough,
        gateway_timeout_settings: None,
    }
}

fn current_record(store: &ProviderStore, id: &str) -> ProviderRecord {
    store.record_by_id(id).unwrap().unwrap()
}

fn lock_provider(store: &ProviderStore, id: &str) {
    let record = current_record(store, id);
    let configuration_id = Uuid::new_v4().to_string();
    let instance_id = Uuid::new_v4().to_string();
    {
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO launch_configuration_revisions (
                    id, credential_kind, profile_snapshot_hash, config_snapshot,
                    codex_executable, codex_version, created_at
                 ) VALUES (?1, 'api_key', 'hash', '{}', '/usr/bin/true', 'test', 1)",
                [&configuration_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO codex_instances (
                    id, launch_configuration_revision_id, credential_kind,
                    api_key_provider_revision_id, managed_codex_home,
                    lifecycle_dir, launch_nonce, state
                 ) VALUES (?1, ?2, 'api_key', ?3, ?4, ?5, ?6, 'running')",
                params![
                    instance_id,
                    configuration_id,
                    record.current_revision_id,
                    format!("/tmp/{instance_id}/home"),
                    format!("/tmp/{instance_id}/lifecycle"),
                    format!("nonce-{instance_id}"),
                ],
            )
            .unwrap();
    }
    store
        .resource_guard
        .acquire_runtime_lock(
            &instance_id,
            &configuration_id,
            &provider_operation_resources(&record),
            1,
        )
        .unwrap();
}

#[test]
fn providers_have_isolated_keys_and_deleting_default_selects_a_replacement() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let first = store.create(input("First", "sk-first")).unwrap();
    let second = store.create(input("Second", "sk-second")).unwrap();
    assert!(first.is_default);
    assert!(!second.is_default);
    store.select_default(&second.id).unwrap();
    assert_eq!(
        store.default_upstream().unwrap().api_key.expose(),
        "sk-second"
    );
    store.delete(&second.id).unwrap();
    let providers = store.list().unwrap();
    assert_eq!(providers.len(), 1);
    assert!(providers[0].is_default);
    assert_eq!(
        store.default_upstream().unwrap().api_key.expose(),
        "sk-first"
    );
    assert_eq!(vault.keys.lock().unwrap().len(), 1);
    let connection = store.connection.lock().unwrap();
    let tombstone: (i64, Option<String>) = connection
        .query_row(
            "SELECT configured, current_revision_id FROM api_key_providers WHERE id = ?1",
            [&second.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(tombstone, (0, None));
    let retained_revisions: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM api_key_provider_revisions WHERE provider_id = ?1",
            [&second.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained_revisions, 1);
}

#[test]
fn create_uses_a_revision_scoped_secret_reference() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let provider = store.create(input("First", "sk-first")).unwrap();
    let record = current_record(&store, &provider.id);

    assert_eq!(
        record.secret_ref,
        revision_secret_ref(&provider.id, &record.current_revision_id)
    );
    assert_ne!(record.secret_ref, format!("provider:{}", provider.id));
    let stored: (String, String) = store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT id, secret_ref FROM api_key_provider_revisions
             WHERE provider_id = ?1",
            [&provider.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, (record.current_revision_id, record.secret_ref));
    assert_eq!(vault.keys.lock().unwrap().len(), 1);
}

#[test]
fn active_revision_requires_its_exact_credential_reference() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let provider = store.create(input("Migrated", "sk-legacy")).unwrap();
    let record = current_record(&store, &provider.id);
    let revision_secret_ref = record.secret_ref.clone();
    let legacy_secret_ref = format!("provider:{}", provider.id);

    vault.clear(&revision_secret_ref).unwrap();
    vault
        .save(&legacy_secret_ref, &ApiKey::for_test("sk-legacy"))
        .unwrap();

    let error = store
        .upstream_for_revision(&record.current_revision_id)
        .err()
        .expect("legacy credential must not satisfy the revision lookup");

    assert_eq!(error, "provider_credential_missing");
    assert!(vault.load(&revision_secret_ref).unwrap().is_none());
    assert_eq!(
        vault.load(&legacy_secret_ref).unwrap().unwrap().expose(),
        "sk-legacy"
    );
}

#[test]
fn exact_revision_lookup_never_drifts_to_the_new_current_revision() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault);
    let provider = store.create(input("Relay", "sk-first")).unwrap();
    let first = store.upstream_for_provider(&provider.id).unwrap();
    assert_eq!(
        store
            .upstream_for_revision(&first.provider_revision_id)
            .unwrap()
            .api_key
            .expose(),
        "sk-first"
    );

    store
        .update(
            &provider.id,
            UpdateProviderInput {
                name: "Relay".to_string(),
                base_url: OPENAI_BASE_URL.to_string(),
                api_key: Some("sk-second".to_string()),
                transport_mode: TransportMode::GatewayPassthrough,
                gateway_timeout_settings: None,
            },
        )
        .unwrap();
    let second = store.upstream_for_provider(&provider.id).unwrap();
    assert_ne!(first.provider_revision_id, second.provider_revision_id);
    assert_eq!(second.api_key.expose(), "sk-second");
    let error = match store.upstream_for_revision(&first.provider_revision_id) {
        Ok(_) => panic!("retired revision must not be resolved"),
        Err(error) => error,
    };
    assert_eq!(error, "provider_revision_not_active");
}

#[test]
fn listing_providers_uses_metadata_without_reading_the_vault() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    store.create(input("First", "sk-first")).unwrap();
    vault.reset_load_count();

    let providers = store.list().unwrap();

    assert_eq!(providers.len(), 1);
    assert!(providers[0].configured);
    assert_eq!(vault.load_count(), 0);
}

#[test]
fn missing_key_is_marked_unconfigured_after_one_explicit_access() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let provider = store.create(input("First", "sk-first")).unwrap();
    let secret_ref = current_record(&store, &provider.id).secret_ref;
    vault.clear(&secret_ref).unwrap();
    vault.reset_load_count();

    let error = match store.upstream_for_provider(&provider.id) {
        Ok(_) => panic!("missing credential should reject explicit Provider access"),
        Err(error) => error,
    };

    assert!(error.contains("API Key 已不存在"));
    assert_eq!(vault.load_count(), 1);
    assert!(!store.list().unwrap()[0].configured);
    assert_eq!(vault.load_count(), 1);
    assert!(store.upstream_for_provider(&provider.id).is_err());
    assert_eq!(vault.load_count(), 1);
}

#[test]
fn deleting_provider_purges_local_usage_and_diagnostics() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault);
    let provider = store.create(input("First", "sk-first")).unwrap();
    {
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO provider_usage_events (provider_id, created_at, succeeded) VALUES (?1, 1, 1)",
                [&provider.id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO gateway_diagnostic_events (provider_id, created_at, source, route, transport_mode, stream, outcome) VALUES (?1, 1, 'gateway', '/v1/responses', 'gateway_passthrough', 0, 'success')",
                [&provider.id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO provider_connectivity_reports (provider_id, checked_at, status, models_check, text_check) VALUES (?1, 1, 'ready', 'ready', 'ready')",
                [&provider.id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO gateway_timeout_settings (provider_id, connect_seconds, first_byte_seconds, stream_idle_seconds, non_stream_total_seconds, updated_at) VALUES (?1, 5, 20, 60, 180, 1)",
                [&provider.id],
            )
            .unwrap();
    }
    store.delete(&provider.id).unwrap();
    let connection = store.connection.lock().unwrap();
    for table in [
        "provider_usage_events",
        "gateway_diagnostic_events",
        "provider_connectivity_reports",
        "gateway_timeout_settings",
    ] {
        let count: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE provider_id = ?1"),
                [&provider.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "records remain in {table}");
    }
}

#[test]
fn updating_provider_persists_metadata_and_optionally_replaces_key() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let provider = store.create(input("First", "sk-first")).unwrap();
    let updated = store
        .update(
            &provider.id,
            UpdateProviderInput {
                name: "Renamed".to_string(),
                base_url: "https://example.com/v1/".to_string(),
                api_key: None,
                transport_mode: TransportMode::GatewayPassthrough,
                gateway_timeout_settings: None,
            },
        )
        .unwrap();
    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.base_url, "https://example.com/v1");
    assert_eq!(updated.transport_mode, TransportMode::GatewayPassthrough);
    let copied_revision = current_record(&store, &provider.id);
    assert_eq!(
        vault
            .load(&copied_revision.secret_ref)
            .unwrap()
            .unwrap()
            .expose(),
        "sk-first"
    );

    store
        .update(
            &provider.id,
            UpdateProviderInput {
                name: "Renamed".to_string(),
                base_url: "https://example.com/v1".to_string(),
                api_key: Some("sk-replaced".to_string()),
                transport_mode: TransportMode::GatewayPassthrough,
                gateway_timeout_settings: None,
            },
        )
        .unwrap();
    let replacement_revision = current_record(&store, &provider.id);
    assert_eq!(
        vault
            .load(&replacement_revision.secret_ref)
            .unwrap()
            .unwrap()
            .expose(),
        "sk-replaced"
    );
    let revisions: Vec<(String, String)> = {
        let connection = store.connection.lock().unwrap();
        let mut statement = connection
            .prepare(
                "SELECT secret_ref, state FROM api_key_provider_revisions
                 WHERE provider_id = ?1 ORDER BY created_at, id",
            )
            .unwrap();
        statement
            .query_map([&provider.id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(revisions.len(), 3);
    assert_eq!(
        revisions
            .iter()
            .filter(|(_, state)| state == "active")
            .count(),
        1
    );
    assert!(revisions
        .iter()
        .filter(|(_, state)| state == "retired")
        .all(|(secret_ref, _)| !vault.keys.lock().unwrap().contains_key(secret_ref)));
    assert_eq!(vault.keys.lock().unwrap().len(), 1);
}

#[test]
fn provider_create_commits_gateway_settings_in_the_same_database_transaction() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let provider = store
        .create(CreateProviderInput {
            name: "Atomic".to_string(),
            base_url: OPENAI_BASE_URL.to_string(),
            api_key: "sk-atomic".to_string(),
            transport_mode: TransportMode::GatewayPassthrough,
            gateway_timeout_settings: Some(ProviderGatewayTimeoutInput {
                connect_seconds: 5,
                first_byte_seconds: 20,
                stream_idle_seconds: 60,
                non_stream_total_seconds: 180,
            }),
        })
        .unwrap();
    let values: (u64, u64, u64, u64) = store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT connect_seconds, first_byte_seconds,
                    stream_idle_seconds, non_stream_total_seconds
             FROM gateway_timeout_settings WHERE provider_id=?1",
            [&provider.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(values, (5, 20, 60, 180));

    vault.reset_write_counts();
    let error = store
        .update(
            &provider.id,
            UpdateProviderInput {
                name: "Invalid".to_string(),
                base_url: OPENAI_BASE_URL.to_string(),
                api_key: Some("sk-must-not-write".to_string()),
                transport_mode: TransportMode::GatewayPassthrough,
                gateway_timeout_settings: Some(ProviderGatewayTimeoutInput {
                    connect_seconds: 0,
                    first_byte_seconds: 20,
                    stream_idle_seconds: 60,
                    non_stream_total_seconds: 180,
                }),
            },
        )
        .unwrap_err();
    assert!(error.contains("连接超时"));
    assert_eq!(vault.write_count(), 0);
}

#[test]
fn name_only_update_keeps_the_current_revision() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let provider = store.create(input("First", "sk-first")).unwrap();
    let original = current_record(&store, &provider.id);
    vault.reset_write_counts();

    store
        .update(
            &provider.id,
            UpdateProviderInput {
                name: "Renamed".to_string(),
                base_url: provider.base_url,
                api_key: None,
                transport_mode: provider.transport_mode,
                gateway_timeout_settings: None,
            },
        )
        .unwrap();

    let updated = current_record(&store, &provider.id);
    assert_eq!(updated.current_revision_id, original.current_revision_id);
    assert_eq!(updated.secret_ref, original.secret_ref);
    assert_eq!(vault.write_count(), 0);
}

#[test]
fn rename_is_allowed_while_a_runtime_lock_blocks_key_changes() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault);
    let provider = store.create(input("Locked", "sk-locked")).unwrap();
    let original = current_record(&store, &provider.id);
    lock_provider(&store, &provider.id);

    let renamed = store
        .rename(&provider.id, "  新名称  ".to_string())
        .unwrap();
    assert_eq!(renamed.name, "新名称");
    let updated = current_record(&store, &provider.id);
    assert_eq!(updated.name, "新名称");
    assert_eq!(updated.current_revision_id, original.current_revision_id);
    assert_eq!(updated.secret_ref, original.secret_ref);

    let blocked = store
        .update(
            &provider.id,
            UpdateProviderInput {
                name: "Blocked".to_string(),
                base_url: provider.base_url,
                api_key: Some("sk-new".to_string()),
                transport_mode: provider.transport_mode,
                gateway_timeout_settings: None,
            },
        )
        .unwrap_err();
    assert!(blocked.contains("configuration_in_use"));
}

#[test]
fn runtime_lock_blocks_provider_writes_but_not_an_unrelated_provider() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let locked = store.create(input("Locked", "sk-locked")).unwrap();
    let unrelated = store.create(input("Unrelated", "sk-unrelated")).unwrap();
    lock_provider(&store, &locked.id);
    vault.reset_write_counts();

    let update_error = match store.update(
        &locked.id,
        UpdateProviderInput {
            name: "Blocked".to_string(),
            base_url: locked.base_url.clone(),
            api_key: Some("sk-blocked".to_string()),
            transport_mode: locked.transport_mode,
            gateway_timeout_settings: None,
        },
    ) {
        Ok(_) => panic!("runtime lock should block provider update"),
        Err(error) => error,
    };
    assert!(update_error.contains("configuration_in_use"));
    assert_eq!(vault.write_count(), 0);

    let delete_error = store.delete(&locked.id).unwrap_err();
    assert!(delete_error.contains("configuration_in_use"));
    assert_eq!(vault.write_count(), 0);

    store
        .update(
            &unrelated.id,
            UpdateProviderInput {
                name: "Changed".to_string(),
                base_url: unrelated.base_url,
                api_key: Some("sk-unrelated-new".to_string()),
                transport_mode: unrelated.transport_mode,
                gateway_timeout_settings: None,
            },
        )
        .unwrap();
    assert_eq!(
        store
            .upstream_for_provider(&unrelated.id)
            .unwrap()
            .api_key
            .expose(),
        "sk-unrelated-new"
    );
}

#[test]
fn updating_provider_restores_the_previous_key_when_sqlite_rejects_metadata() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let provider = store.create(input("First", "sk-first")).unwrap();
    let original = current_record(&store, &provider.id);
    vault.reset_write_counts();
    store
        .connection
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_provider_update BEFORE UPDATE ON api_key_providers BEGIN SELECT RAISE(FAIL, 'forced update failure'); END;",
        )
        .unwrap();

    let error = match store.update(
        &provider.id,
        UpdateProviderInput {
            name: "Renamed".to_string(),
            base_url: OPENAI_BASE_URL.to_string(),
            api_key: Some("sk-replaced".to_string()),
            transport_mode: TransportMode::GatewayPassthrough,
            gateway_timeout_settings: None,
        },
    ) {
        Ok(_) => panic!("SQLite trigger should reject the Provider update"),
        Err(error) => error,
    };

    assert!(error.contains("新 API Key 已回收"));
    assert_eq!(
        vault.load(&original.secret_ref).unwrap().unwrap().expose(),
        "sk-first"
    );
    assert_eq!(vault.write_count(), 2);
    assert_eq!(vault.keys.lock().unwrap().len(), 1);
    let connection = store.connection.lock().unwrap();
    let revision_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM api_key_provider_revisions WHERE provider_id = ?1",
            [&provider.id],
            |row| row.get(0),
        )
        .unwrap();
    let lease_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM resource_operation_leases",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(revision_count, 1);
    assert_eq!(lease_count, 0);
}

#[test]
fn failed_secret_compensation_marks_the_operation_for_recovery() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let provider = store.create(input("First", "sk-first")).unwrap();
    store
        .connection
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_provider_recovery_test BEFORE UPDATE ON api_key_providers BEGIN SELECT RAISE(FAIL, 'forced update failure'); END;",
        )
        .unwrap();
    vault.fail_next_clear();

    let error = match store.update(
        &provider.id,
        UpdateProviderInput {
            name: "Renamed".to_string(),
            base_url: provider.base_url,
            api_key: Some("sk-replaced".to_string()),
            transport_mode: TransportMode::GatewayPassthrough,
            gateway_timeout_settings: None,
        },
    ) {
        Ok(_) => panic!("SQLite trigger should reject the Provider update"),
        Err(error) => error,
    };

    assert!(error.contains("无法回收新 API Key"));
    let connection = store.connection.lock().unwrap();
    let leases: (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), SUM(state = 'recovering')
             FROM resource_operation_leases",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(leases, (3, 3));
    assert_eq!(vault.keys.lock().unwrap().len(), 2);
}

#[test]
fn startup_recovery_clears_unpublished_revision_secret_and_releases_lease() {
    let vault = Arc::new(MemoryVault::default());
    let database = Database::in_memory();
    let store = ProviderStore::from_database(database.clone(), vault.clone()).unwrap();
    let provider = store.create(input("First", "sk-first")).unwrap();
    let record = current_record(&store, &provider.id);
    let orphan_ref = format!("provider:{}:revision:orphan", provider.id);
    vault
        .save(&orphan_ref, &ApiKey::for_test("sk-orphan"))
        .unwrap();
    store
        .resource_guard
        .acquire_operation_leases(
            "operation-orphan-update",
            "update_api_key_provider",
            Some(std::process::id() as i64),
            Some(&orphan_ref),
            &provider_operation_resources(&record),
            unix_now_seconds(),
        )
        .unwrap();

    let recovered = ProviderStore::from_database(database, vault.clone()).unwrap();

    assert!(vault.load(&orphan_ref).unwrap().is_none());
    assert!(recovered
        .resource_guard
        .recoverable_leases()
        .unwrap()
        .is_empty());
    assert_eq!(
        recovered
            .upstream_for_provider(&provider.id)
            .unwrap()
            .api_key
            .expose(),
        "sk-first"
    );
}

#[test]
fn startup_recovery_finishes_secret_cleanup_after_committed_tombstone() {
    let vault = Arc::new(MemoryVault::default());
    let database = Database::in_memory();
    let store = ProviderStore::from_database(database.clone(), vault.clone()).unwrap();
    let provider = store.create(input("First", "sk-first")).unwrap();
    let record = current_record(&store, &provider.id);
    store
        .resource_guard
        .acquire_operation_leases(
            "operation-crashed-delete",
            "delete_api_key_provider",
            Some(std::process::id() as i64),
            Some(&record.secret_ref),
            &provider_operation_resources(&record),
            unix_now_seconds(),
        )
        .unwrap();
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE api_key_providers
             SET configured=0, is_default=0, current_revision_id=NULL
             WHERE id=?1",
            [&provider.id],
        )
        .unwrap();

    let recovered = ProviderStore::from_database(database, vault.clone()).unwrap();

    assert!(vault.load(&record.secret_ref).unwrap().is_none());
    assert!(recovered.list().unwrap().is_empty());
    assert!(recovered
        .resource_guard
        .recoverable_leases()
        .unwrap()
        .is_empty());
}

#[test]
fn startup_recovery_reclaims_retired_secret_after_committed_update() {
    let vault = Arc::new(MemoryVault::default());
    let database = Database::in_memory();
    let store = ProviderStore::from_database(database.clone(), vault.clone()).unwrap();
    let provider = store.create(input("First", "sk-first")).unwrap();
    let original = current_record(&store, &provider.id);
    vault.fail_next_clear();

    let error = store
        .update(
            &provider.id,
            UpdateProviderInput {
                name: "Updated".to_string(),
                base_url: provider.base_url,
                api_key: Some("sk-second".to_string()),
                transport_mode: provider.transport_mode,
                gateway_timeout_settings: None,
            },
        )
        .unwrap_err();
    assert!(error.contains("Provider 已更新，但无法回收旧 API Key"));
    let current = current_record(&store, &provider.id);
    assert_ne!(current.current_revision_id, original.current_revision_id);
    assert!(vault.load(&original.secret_ref).unwrap().is_some());

    let recovered = ProviderStore::from_database(database, vault.clone()).unwrap();

    assert!(vault.load(&original.secret_ref).unwrap().is_none());
    assert_eq!(
        recovered
            .upstream_for_provider(&provider.id)
            .unwrap()
            .api_key
            .expose(),
        "sk-second"
    );
    assert!(recovered
        .resource_guard
        .recoverable_leases()
        .unwrap()
        .is_empty());
}

#[test]
fn deleting_provider_keeps_the_current_key_when_sqlite_rejects_tombstone() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault.clone());
    let provider = store.create(input("First", "sk-first")).unwrap();
    let original = current_record(&store, &provider.id);
    vault.reset_write_counts();
    store
        .connection
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_provider_delete BEFORE UPDATE OF current_revision_id ON api_key_providers BEGIN SELECT RAISE(FAIL, 'forced delete failure'); END;",
        )
        .unwrap();

    let error = store.delete(&provider.id).unwrap_err();

    assert!(error.contains("无法删除 Provider"));
    assert!(store.record_by_id(&provider.id).unwrap().is_some());
    assert_eq!(
        vault.load(&original.secret_ref).unwrap().unwrap().expose(),
        "sk-first"
    );
    assert_eq!(vault.write_count(), 0);
}

#[test]
fn api_base_url_normalizes_root_without_changing_custom_prefixes() {
    for (input, expected) in [
        ("https://api.example", "https://api.example"),
        (" https://api.example/ ", "https://api.example"),
        ("https://api.example/v1/", "https://api.example/v1"),
        (
            "https://api.example/proxy/v1/",
            "https://api.example/proxy/v1",
        ),
        ("https://api.example/custom/", "https://api.example/custom"),
        ("http://127.0.0.1:8080", "http://127.0.0.1:8080"),
        ("http://[::1]:8080/", "http://[::1]:8080"),
    ] {
        let normalized = normalize_api_base_url(input).unwrap();
        assert_eq!(normalized, expected);
        assert_eq!(normalize_api_base_url(&normalized).unwrap(), normalized);
    }
    for input in [
        "",
        "file:///tmp/config",
        "https://api.example?key=secret",
        "https://api.example/#fragment",
        "https://user:secret@api.example",
        "https://api.example/v1/responses",
        "https://api.example/chat/completions",
    ] {
        assert_eq!(
            normalize_api_base_url(input).unwrap_err(),
            "upstream_base_url_invalid"
        );
    }
}

#[test]
fn rejects_non_https_non_loopback_provider_urls() {
    assert!(validate_base_url("http://example.com/v1".to_string()).is_err());
    assert!(validate_base_url("http://127.0.0.1:8080/v1".to_string()).is_ok());
}

#[test]
fn transport_mode_is_persisted_with_each_provider() {
    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::in_memory(vault);
    let provider = store
        .create(CreateProviderInput {
            name: "Chat-only".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "sk-chat".to_string(),
            transport_mode: TransportMode::GatewayChatCompletions,
            gateway_timeout_settings: None,
        })
        .unwrap();
    assert_eq!(
        provider.transport_mode,
        TransportMode::GatewayChatCompletions
    );
    assert_eq!(
        store.default_upstream().unwrap().transport_mode,
        TransportMode::GatewayChatCompletions
    );
}
