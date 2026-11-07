//! Workspace deletion tests use disposable app-owned directories only.

use std::{path::PathBuf, sync::Arc};

use rusqlite::params;
use uuid::Uuid;

use super::deletion::EXPLICITLY_EMPTY_WORKSPACE_KEY;
use super::test_support::*;
use super::*;
use crate::database::Database;

struct Fixture {
    database: Database,
    store: ManagedLaunchStore,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let database = Database::in_memory();
        seed_provider(&database, "relay", "relay-r1", "responses");
        let root = std::env::temp_dir().join(format!("rcodex-workspace-delete-{}", Uuid::new_v4()));
        let store = ManagedLaunchStore::for_test(
            database.clone(),
            root.join("codex-instances"),
            Arc::new(MemoryInstanceVault),
        );
        Self {
            database,
            store,
            root,
        }
    }
    fn workspace(&self, new: bool) -> ManagedPresetSaved {
        let mut input = grok_preset("relay", None, None);
        input.create_new = new;
        self.store.create_exposed_test_preset(input).unwrap()
    }
    fn instance(&self, preset: &ManagedPresetSaved, target: &str) -> String {
        let id = Uuid::new_v4().to_string();
        let lifecycle = self.root.join("codex-instances").join(&id);
        let home = lifecycle.join("codex-home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(lifecycle.join("electron-data")).unwrap();
        std::fs::write(home.join("history.jsonl"), b"private fixture history").unwrap();
        std::fs::write(lifecycle.join("electron-data/session"), b"desktop fixture").unwrap();
        let config = format!("config-{id}");
        let connection = self.database.connection();
        let connection = connection.lock().unwrap();
        connection.execute(
            "INSERT INTO launch_configuration_revisions(id,launch_preset_id,launch_preset_version,credential_kind,profile_snapshot_hash,config_snapshot,codex_executable,codex_version,created_at)
             VALUES (?1,?2,?3,'api_key','sha256:test',?4,'fixture-codex','test',1)",
            params![config,preset.id,preset.version,serde_json::json!({"launchTarget":target}).to_string()]
        ).unwrap();
        connection.execute(
            "INSERT INTO codex_instances(id,launch_configuration_revision_id,credential_kind,api_key_provider_revision_id,managed_codex_home,lifecycle_dir,launch_nonce,state,exited_at)
             VALUES (?1,?2,'api_key','relay-r1',?3,?4,?5,'exited',2)",
            params![id,config,home.display().to_string(),lifecycle.display().to_string(),Uuid::new_v4().to_string()]
        ).unwrap();
        connection.execute("INSERT INTO launch_configuration_dependencies(launch_configuration_revision_id,resource_kind,resource_id) VALUES (?1,'launch_preset',?2)",params![config,preset.id]).unwrap();
        connection.execute("INSERT INTO runtime_configuration_locks(instance_id,launch_configuration_revision_id,acquired_at,released_at) VALUES (?1,?2,1,2)",params![id,config]).unwrap();
        id
    }
    fn count(&self, table: &str) -> i64 {
        self.database
            .connection()
            .lock()
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn deletes_only_target_workspace_and_selects_first_remaining_slot() {
    let fixture = Fixture::new();
    let a = fixture.workspace(false);
    let b = fixture.workspace(true);
    let aid = fixture.instance(&a, "managed_app");
    let bid = fixture.instance(&b, "managed_app");
    let legacy = fixture.root.join("managed-app-sqlite").join(&a.id);
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("state_5.sqlite"), b"legacy fixture").unwrap();
    let plan = fixture
        .store
        .prepare_workspace_deletion(&a.id, a.version)
        .unwrap();
    assert_eq!(plan.instance_ids, [aid.clone()]);
    fixture.store.delete_workspace_storage(&plan).unwrap();
    assert_eq!(
        fixture.store.finish_workspace_deletion(&plan).unwrap(),
        Some(b.id.clone())
    );
    assert!(!fixture.root.join("codex-instances").join(aid).exists());
    assert!(!legacy.exists());
    assert!(fixture
        .root
        .join("codex-instances")
        .join(bid)
        .join("codex-home/history.jsonl")
        .is_file());
    assert_eq!(fixture.count("launch_presets"), 1);
    assert_eq!(fixture.count("codex_instances"), 1);
    assert_eq!(fixture.count("api_key_providers"), 1);
    assert_eq!(fixture.count("provider_model_exposures"), 1);
    assert_eq!(fixture.count("model_route_identities"), 1);
    assert_eq!(fixture.count("resource_operation_leases"), 0);
    let connection = fixture.database.connection();
    let connection = connection.lock().unwrap();
    let violations: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
}

#[test]
fn rejects_every_active_state_and_unreleased_terminal_locks_before_any_deletion() {
    for state in ["launching", "running", "stopping", "orphaned", "degraded"] {
        let fixture = Fixture::new();
        let preset = fixture.workspace(false);
        let instance = fixture.instance(&preset, "cli");
        fixture
            .database
            .connection()
            .lock()
            .unwrap()
            .execute(
                "UPDATE codex_instances SET state=?1,exited_at=NULL WHERE id=?2",
                params![state, instance],
            )
            .unwrap();
        assert!(
            fixture
                .store
                .prepare_workspace_deletion(&preset.id, preset.version)
                .is_err(),
            "{state}"
        );
        assert!(fixture
            .root
            .join("codex-instances")
            .join(instance)
            .join("codex-home/history.jsonl")
            .is_file());
        assert_eq!(fixture.count("resource_operation_leases"), 0);
        let marker:i64=fixture.database.connection().lock().unwrap().query_row("SELECT COUNT(*) FROM provider_migration_state WHERE key LIKE 'managed_workspace_deleting:%'",[],|row|row.get(0)).unwrap();
        assert_eq!(marker, 0);
    }
    let fixture = Fixture::new();
    let preset = fixture.workspace(false);
    let instance = fixture.instance(&preset, "managed_app");
    fixture
        .database
        .connection()
        .lock()
        .unwrap()
        .execute(
            "UPDATE runtime_configuration_locks SET released_at=NULL WHERE instance_id=?1",
            [instance],
        )
        .unwrap();
    assert!(fixture
        .store
        .prepare_workspace_deletion(&preset.id, preset.version)
        .is_err());
    assert_eq!(fixture.count("resource_operation_leases"), 0);
}

#[test]
fn validates_all_instances_before_removing_the_first_and_rejects_version_conflicts() {
    let fixture = Fixture::new();
    let preset = fixture.workspace(false);
    let first = fixture.instance(&preset, "managed_app");
    let second = fixture.instance(&preset, "cli");
    assert!(fixture
        .store
        .prepare_workspace_deletion(&preset.id, preset.version + 1)
        .is_err());
    fixture
        .database
        .connection()
        .lock()
        .unwrap()
        .execute(
            "UPDATE codex_instances SET managed_codex_home=?1 WHERE id=?2",
            params![
                fixture.root.join("external-home").display().to_string(),
                second
            ],
        )
        .unwrap();
    assert!(fixture
        .store
        .prepare_workspace_deletion(&preset.id, preset.version)
        .is_err());
    assert!(fixture
        .root
        .join("codex-instances")
        .join(first)
        .join("codex-home/history.jsonl")
        .is_file());
    assert_eq!(fixture.count("resource_operation_leases"), 0);
}

#[cfg(unix)]
#[test]
fn rejects_external_home_symlinks_and_preserves_the_external_target() {
    let fixture = Fixture::new();
    let preset = fixture.workspace(false);
    let instance = fixture.instance(&preset, "managed_app");
    let home = fixture
        .root
        .join("codex-instances")
        .join(instance)
        .join("codex-home");
    let outside = fixture.root.join("external-home");
    std::fs::rename(&home, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &home).unwrap();
    assert!(fixture
        .store
        .prepare_workspace_deletion(&preset.id, preset.version)
        .is_err());
    assert!(outside.join("history.jsonl").is_file());
    assert_eq!(fixture.count("launch_presets"), 1);
    assert_eq!(fixture.count("resource_operation_leases"), 0);
}

#[cfg(unix)]
#[test]
fn removes_mac_short_homes_owned_aliases_and_cli_aliases_without_following_them() {
    let fixture = Fixture::new();
    let preset = fixture.workspace(false);
    let app = fixture.instance(&preset, "managed_app");
    let cli = fixture.instance(&preset, "cli");
    let short_root = fixture.root.join("r");
    std::fs::create_dir_all(&short_root).unwrap();
    let app_home = fixture
        .root
        .join("codex-instances")
        .join(&app)
        .join("codex-home");
    let app_short = short_root.join(&app.replace('-', "")[..12]);
    std::fs::rename(&app_home, &app_short).unwrap();
    std::os::unix::fs::symlink(&app_short, &app_home).unwrap();
    fixture
        .database
        .connection()
        .lock()
        .unwrap()
        .execute(
            "UPDATE codex_instances SET managed_codex_home=?1 WHERE id=?2",
            params![app_short.display().to_string(), app],
        )
        .unwrap();
    let cli_home = fixture
        .root
        .join("codex-instances")
        .join(&cli)
        .join("codex-home");
    let cli_alias = short_root.join(&cli.replace('-', "")[..12]);
    std::os::unix::fs::symlink(&cli_home, &cli_alias).unwrap();
    let plan = fixture
        .store
        .prepare_workspace_deletion(&preset.id, preset.version)
        .unwrap();
    fixture.store.delete_workspace_storage(&plan).unwrap();
    fixture.store.finish_workspace_deletion(&plan).unwrap();
    assert!(std::fs::symlink_metadata(app_home).is_err());
    assert!(!app_short.exists());
    assert!(std::fs::symlink_metadata(cli_alias).is_err());
}

#[test]
fn last_deletion_persists_empty_state_and_next_create_reuses_slot_a() {
    let fixture = Fixture::new();
    let preset = fixture.workspace(false);
    let legacy = fixture.root.join("managed-app-sqlite").join(&preset.id);
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("orphan.sqlite"), b"never launched fixture").unwrap();
    let plan = fixture
        .store
        .prepare_workspace_deletion(&preset.id, preset.version)
        .unwrap();
    assert!(plan.instance_ids.is_empty());
    fixture.store.delete_workspace_storage(&plan).unwrap();
    assert_eq!(
        fixture.store.finish_workspace_deletion(&plan).unwrap(),
        None
    );
    assert!(!legacy.exists());
    fixture.store.ensure_initial_workspace().unwrap();
    fixture.store.ensure_initial_workspace().unwrap();
    assert_eq!(fixture.count("launch_presets"), 0);
    let mark: bool = fixture
        .database
        .connection()
        .lock()
        .unwrap()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_migration_state WHERE key=?1)",
            [EXPLICITLY_EMPTY_WORKSPACE_KEY],
            |row| row.get(0),
        )
        .unwrap();
    assert!(mark);
    let recreated = fixture.workspace(true);
    let connection = fixture.database.connection();
    let connection = connection.lock().unwrap();
    let name: String = connection
        .query_row(
            "SELECT name FROM launch_presets WHERE id=?1",
            [recreated.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "隔离空间 A");
    let mark: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_migration_state WHERE key=?1)",
            [EXPLICITLY_EMPTY_WORKSPACE_KEY],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!mark);
}

#[test]
fn deletion_failure_keeps_records_and_intent_for_retry_and_blocks_edits() {
    let fixture = Fixture::new();
    let preset = fixture.workspace(false);
    fixture.instance(&preset, "managed_app");
    let plan = fixture
        .store
        .prepare_workspace_deletion(&preset.id, preset.version)
        .unwrap();
    assert!(
        fixture.store.finish_workspace_deletion(&plan).is_err(),
        "files must be removed before db commit"
    );
    fixture.store.release_workspace_deletion(&plan).unwrap();
    assert_eq!(fixture.count("resource_operation_leases"), 0);
    let mut edit = grok_preset("relay", None, None);
    edit.launch_preset_id = Some(preset.id.clone());
    assert!(fixture.store.create_preset(edit).is_err());
    assert!(fixture
        .store
        .set_managed_login_provider(&preset.id, preset.version, Some("relay"))
        .is_err());
    let mut copy = grok_preset("relay", None, None);
    copy.create_new = true;
    copy.source_preset_id = Some(preset.id.clone());
    assert!(fixture.store.create_preset(copy).is_err());
    let plan = fixture
        .store
        .prepare_workspace_deletion(&preset.id, preset.version)
        .unwrap();
    fixture.store.delete_workspace_storage(&plan).unwrap();
    fixture.database.connection().lock().unwrap().execute_batch("CREATE TRIGGER reject_workspace_delete BEFORE DELETE ON launch_presets BEGIN SELECT RAISE(ABORT,'injected delete failure'); END;").unwrap();
    let error = fixture
        .store
        .finish_workspace_deletion(&plan)
        .err()
        .unwrap();
    assert!(error.contains("workspace_delete_database_failed"));
    assert_eq!(fixture.count("launch_presets"), 1);
    assert_eq!(fixture.count("codex_instances"), 1);
    fixture.store.release_workspace_deletion(&plan).unwrap();
    fixture
        .database
        .connection()
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_workspace_delete;")
        .unwrap();
    let retry = fixture
        .store
        .prepare_workspace_deletion(&preset.id, preset.version)
        .unwrap();
    fixture.store.delete_workspace_storage(&retry).unwrap();
    assert_eq!(
        fixture.store.finish_workspace_deletion(&retry).unwrap(),
        None
    );
    assert_eq!(fixture.count("codex_instances"), 0);
}

#[test]
fn new_workspace_reuses_deleted_slot_without_renumbering_the_survivor() {
    let fixture = Fixture::new();
    let a = fixture.workspace(false);
    let b = fixture.workspace(true);
    let plan = fixture
        .store
        .prepare_workspace_deletion(&a.id, a.version)
        .unwrap();
    fixture.store.delete_workspace_storage(&plan).unwrap();
    fixture.store.finish_workspace_deletion(&plan).unwrap();
    let fresh = fixture.workspace(true);
    let connection = fixture.database.connection();
    let connection = connection.lock().unwrap();
    let names: Vec<String> = connection
        .prepare("SELECT name FROM launch_presets WHERE deleted_at IS NULL ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(names, ["隔离空间 A", "隔离空间 B"]);
    assert_ne!(fresh.id, a.id);
    let still_b: String = connection
        .query_row(
            "SELECT name FROM launch_presets WHERE id=?1",
            [b.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(still_b, "隔离空间 B");
}

#[cfg(unix)]
#[test]
fn deletes_actual_prepared_route_binding_configuration_and_role_graph_atomically() {
    use url::Url;
    let fixture = Fixture::new();
    let preset = fixture.workspace(false);
    fixture.database.connection().lock().unwrap().execute(
        "INSERT INTO launch_preset_role_models(launch_preset_id,role,route_identity_id)
         SELECT launch_preset_id,'auto_review',route_identity_id FROM launch_preset_model_selections WHERE launch_preset_id=?1 LIMIT 1",
        [&preset.id],
    ).unwrap();
    let codex = write_fake_codex(&fixture.root, &["template", "grok-4.6"]);
    let prepared = fixture
        .store
        .prepare_launch_with_target(
            &preset.id,
            preset.version,
            None,
            &codex,
            "codex-test",
            Some(&Url::parse("http://127.0.0.1:43123/v1").unwrap()),
            &fixture.root,
            "managed_app",
        )
        .unwrap();
    assert_eq!(fixture.count("route_set_revisions"), 1);
    assert_eq!(fixture.count("model_routes"), 1);
    assert_eq!(fixture.count("launch_configuration_route_sets"), 1);
    assert_eq!(fixture.count("instance_gateway_bindings"), 1);
    {
        let connection = fixture.database.connection();
        let connection = connection.lock().unwrap();
        connection
            .execute(
                "UPDATE codex_instances SET state='exited',exited_at=2 WHERE id=?1",
                [&prepared.instance_id],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE runtime_configuration_locks SET released_at=2 WHERE instance_id=?1",
                [&prepared.instance_id],
            )
            .unwrap();
    }
    let plan = fixture
        .store
        .prepare_workspace_deletion(&preset.id, preset.version)
        .unwrap();
    fixture
        .store
        .revoke_binding_for_delete(&prepared.instance_id)
        .unwrap();
    fixture.store.delete_workspace_storage(&plan).unwrap();
    fixture.store.finish_workspace_deletion(&plan).unwrap();
    for table in [
        "launch_presets",
        "launch_preset_role_models",
        "codex_instances",
        "runtime_configuration_locks",
        "instance_gateway_bindings",
        "launch_preset_runtime_bindings",
        "launch_configuration_route_sets",
        "launch_configuration_dependencies",
        "launch_configuration_revisions",
        "model_routes",
        "route_set_provider_revisions",
        "route_set_revisions",
        "gateway_service_revisions",
    ] {
        assert_eq!(fixture.count(table), 0, "{table}");
    }
    assert_eq!(fixture.count("api_key_providers"), 1);
    assert_eq!(fixture.count("model_route_identities"), 1);
    assert_eq!(fixture.count("provider_model_exposures"), 1);
    let violations: i64 = fixture
        .database
        .connection()
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
    assert!(!prepared.managed_codex_home.exists());
    assert!(!prepared.status_file.parent().unwrap().exists());
}

#[test]
fn deleting_an_unselected_workspace_keeps_the_current_selection() {
    let fixture = Fixture::new();
    let a = fixture.workspace(false);
    let b = fixture.workspace(true);
    fixture.store.select_preset(&b.id).unwrap();
    let plan = fixture
        .store
        .prepare_workspace_deletion(&a.id, a.version)
        .unwrap();
    fixture.store.delete_workspace_storage(&plan).unwrap();
    assert_eq!(
        fixture.store.finish_workspace_deletion(&plan).unwrap(),
        Some(b.id.clone())
    );
    let preferred: String = fixture
        .database
        .connection()
        .lock()
        .unwrap()
        .query_row(
            "SELECT id FROM launch_presets WHERE entry_kind='managed_app' AND deleted_at IS NULL AND is_preferred=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preferred, b.id);
}
