//! Codex 控制面：预设校验、官方启动锁和快照。
//!
//! 不覆盖 Gateway 转发或隔离配置投影。

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::database::Database;

use super::*;

#[test]
fn startup_log_redacts_credentials_before_ui_exposure() {
    let safe = sanitize_startup_log(
        "codex_spawned pid=7\nAuthorization: Bearer secret-value\nTHREER_INSTANCE_KEY=instance-secret",
    );
    assert!(safe.contains("codex_spawned pid=7"));
    assert!(!safe.contains("secret-value"));
    assert!(!safe.contains("instance-secret"));
    assert!(safe.contains("[redacted]"));
}

fn seeded_store() -> (CodexControlStore, PathBuf) {
    let database = Database::in_memory();
    let root = std::env::temp_dir().join(format!("3r-control-{}", Uuid::new_v4()));
    {
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO api_key_providers (
                    id, name, base_url, secret_ref, is_default,
                    transport_mode, configured, created_at, updated_at
                 ) VALUES ('provider-a','Provider A','https://example.test/v1',
                           'provider:a',1,'direct_responses',1,1,1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO api_key_provider_revisions (
                    id, provider_id, secret_ref, base_url, wire_api,
                    transport_mode, state, created_at
                 ) VALUES ('revision-a','provider-a','provider:a',
                           'https://example.test/v1','responses',
                           'direct_responses','active',1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE api_key_providers SET current_revision_id='revision-a'
                 WHERE id='provider-a'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO launch_presets (
                    id,name,credential_kind,api_key_provider_id,gateway_policy,
                    profile_options,version,is_preferred,created_at,updated_at
                 ) VALUES ('preset-a','Preset A','api_key','provider-a','direct',
                           '{}',1,1,1,1)",
                [],
            )
            .unwrap();
    }
    (CodexControlStore::for_test(database, root.clone()), root)
}

fn seeded_official_store() -> (CodexControlStore, PathBuf) {
    let database = Database::in_memory();
    let root = std::env::temp_dir().join(format!("3r-official-control-{}", Uuid::new_v4()));
    let managed_home = root
        .join("managed-official-codex-homes")
        .join("account-a")
        .join("official-revision-a");
    std::fs::create_dir_all(&managed_home).unwrap();
    {
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO codex_accounts (
                    id, masked_identity, workspace_label, plan_label,
                    current_credential_revision_id, created_at, updated_at
                 ) VALUES ('account-a','u***@example.test',NULL,'Plus',NULL,1,1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO official_credential_revisions (
                    id, account_id, managed_codex_home, auth_state,
                    auth_checked_at, auth_error_code, codex_version,
                    schema_fingerprint, created_at, retired_at
                 ) VALUES ('official-revision-a','account-a',?1,'authorized',
                           1,NULL,'test-codex','test-schema',1,NULL)",
                [managed_home.display().to_string()],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE codex_accounts
                 SET current_credential_revision_id='official-revision-a'
                 WHERE id='account-a'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO launch_presets (
                    id,name,credential_kind,codex_account_id,profile_options,
                    version,is_preferred,created_at,updated_at
                 ) VALUES ('official-preset-a','Official Preset','official','account-a',
                           '{}',1,1,1,1)",
                [],
            )
            .unwrap();
    }
    (CodexControlStore::for_test(database, root.clone()), root)
}

#[test]
fn preset_validation_keeps_credentials_mutually_exclusive() {
    assert!(validate_preset_input("api_key", Some("p"), None, Some("direct")).is_ok());
    assert!(validate_preset_input("official", None, Some("a"), None).is_ok());
    assert!(validate_preset_input("official", Some("p"), Some("a"), None).is_err());
    assert!(validate_preset_input("api_key", Some("p"), None, None).is_err());
}

#[test]
fn deleting_preset_tombstones_it_without_removing_history() {
    let (store, _root) = seeded_store();

    store.delete_preset("preset-a").unwrap();

    let snapshot = store.snapshot().unwrap();
    assert!(snapshot
        .presets
        .iter()
        .all(|preset| preset.id != "preset-a"));

    let connection = store.connection.lock().unwrap();
    let deleted_at: Option<i64> = connection
        .query_row(
            "SELECT deleted_at FROM launch_presets WHERE id='preset-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(deleted_at.is_some());
}

#[test]
fn renaming_preset_changes_only_the_name() {
    let (store, root) = seeded_store();

    let renamed = store.rename_preset("preset-a", "  工作区  ", 1).unwrap();
    assert_eq!(renamed.name, "工作区");
    assert!(renamed.is_preferred);
    assert_eq!(renamed.version, 2);

    let unchanged = store.rename_preset("preset-a", "工作区", 2).unwrap();
    assert_eq!(unchanged.version, 2);
    assert_eq!(
        store.rename_preset("preset-a", " ", 2).unwrap_err(),
        "invalid_launch_preset_name"
    );
    assert_eq!(
        store.rename_preset("preset-a", "其他", 1).unwrap_err(),
        "version_conflict"
    );
    let snapshot = store.snapshot().unwrap();
    let preset = snapshot
        .presets
        .iter()
        .find(|preset| preset.id == "preset-a")
        .unwrap();
    assert_eq!(preset.name, "工作区");
    assert!(preset.is_preferred);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn workspace_resolution_rejects_files() {
    let directory = std::env::temp_dir().join(format!("3r-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("file");
    std::fs::write(&file, "x").unwrap();
    assert!(resolve_workspace(file.to_str()).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn official_launch_locks_account_and_enforces_one_active_instance_per_revision() {
    let (store, root) = seeded_official_store();
    let input = LaunchCodexInput {
        launch_preset_id: "official-preset-a".to_string(),
        expected_version: 1,
        workspace_path: None,
        launch_target: None,
    };
    assert_eq!(store.preset_credential_kind(&input).unwrap(), "official");
    let prepared = store
        .prepare_official_launch(&input, Path::new("/bin/sh"), "test-codex")
        .unwrap();
    assert!(store
        .assert_resource_mutable("official_account", "account-a")
        .unwrap_err()
        .starts_with("configuration_in_use:"));
    assert_eq!(
        store
            .prepare_official_launch(&input, Path::new("/bin/sh"), "test-codex")
            .unwrap_err(),
        "official_credential_in_use"
    );

    store
        .mark_launch_failed(&prepared.instance_id, true)
        .unwrap();
    assert!(store
        .assert_resource_mutable("official_account", "account-a")
        .is_ok());
    assert!(store
        .prepare_official_launch(&input, Path::new("/bin/sh"), "test-codex")
        .is_ok());
    let snapshot = store.snapshot().unwrap();
    assert!(!snapshot.compatibility.official_accounts_available);
    assert!(!snapshot.compatibility.managed_app_available);
    assert!(!snapshot.compatibility.official_hybrid_available);
    assert!(snapshot.providers.is_empty());
    let account = &snapshot.accounts[0];
    assert_eq!(account.auth_checked_at, Some(1));
    assert_eq!(account.codex_version.as_deref(), Some("test-codex"));
    assert_eq!(account.schema_fingerprint.as_deref(), Some("test-schema"));
    assert!(account.quota.is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn finished_instance_does_not_keep_workspace_running_lock() {
    let (store, root) = seeded_official_store();
    let prepared = store
        .prepare_official_launch(
            &LaunchCodexInput {
                launch_preset_id: "official-preset-a".to_string(),
                expected_version: 1,
                workspace_path: None,
                launch_target: None,
            },
            Path::new("/bin/sh"),
            "test-codex",
        )
        .unwrap();
    {
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "UPDATE codex_instances SET state='exited', exited_at=2 WHERE id=?1",
                [&prepared.instance_id],
            )
            .unwrap();
        let released: Option<i64> = connection
            .query_row(
                "SELECT released_at FROM runtime_configuration_locks WHERE instance_id=?1",
                [&prepared.instance_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(released.is_none());
    }

    assert!(store
        .assert_resource_mutable("official_account", "account-a")
        .is_ok());
    let snapshot = store.snapshot().unwrap();
    assert!(snapshot.accounts[0].locked_by_instances.is_empty());
    {
        let connection = store.connection.lock().unwrap();
        let released: Option<i64> = connection
            .query_row(
                "SELECT released_at FROM runtime_configuration_locks WHERE instance_id=?1",
                [&prepared.instance_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(released.is_some());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn official_snapshot_returns_latest_quota_and_instance_credential_revision() {
    let (store, root) = seeded_official_store();
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO codex_quota_snapshots (
                account_id, official_credential_revision_id, source,
                primary_used_percent, primary_window_minutes, primary_resets_at,
                secondary_used_percent, secondary_window_minutes, secondary_resets_at,
                refreshed_at, expires_at, error_code
             ) VALUES ('account-a','official-revision-a','official_app_server',
                       31,300,500,62,10080,900,10,20,NULL)",
            [],
        )
        .unwrap();
    let prepared = store
        .prepare_official_launch(
            &LaunchCodexInput {
                launch_preset_id: "official-preset-a".to_string(),
                expected_version: 1,
                workspace_path: None,
                launch_target: None,
            },
            Path::new("/bin/sh"),
            "test-codex",
        )
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    let quota = snapshot.accounts[0].quota.as_ref().unwrap();
    assert_eq!(quota.primary_used_percent, Some(31.0));
    assert_eq!(quota.secondary_window_minutes, Some(10080));
    let instance = snapshot
        .instances
        .iter()
        .find(|instance| instance.id == prepared.instance_id)
        .unwrap();
    assert_eq!(
        instance.credential_revision_id.as_deref(),
        Some("official-revision-a")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn official_launch_rejects_a_managed_home_outside_its_owned_revision_path() {
    let (store, root) = seeded_official_store();
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE official_credential_revisions
             SET managed_codex_home='/tmp/not-owned-by-rcodex'
             WHERE id='official-revision-a'",
            [],
        )
        .unwrap();
    let error = store
        .prepare_official_launch(
            &LaunchCodexInput {
                launch_preset_id: "official-preset-a".to_string(),
                expected_version: 1,
                workspace_path: None,
                launch_target: None,
            },
            Path::new("/bin/sh"),
            "test-codex",
        )
        .unwrap_err();
    assert_eq!(error, "managed_home_invalid");
    std::fs::remove_dir_all(root).unwrap();
}
