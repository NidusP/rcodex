//! 隔离 home、Electron 数据、活动对话库迁移与删除。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{params, Connection};
use url::Url;
use uuid::Uuid;

use crate::database::Database;
use crate::launcher_protocol::LaunchTarget;
use crate::managed_codex_config::INSTANCE_GATEWAY_HEADER;
use crate::model_family_profile::{GROK_AUTO_COMPACT_TOKEN_LIMIT, GROK_CONTEXT_WINDOW};
use crate::routing::{CompactMode, RouteKind};

use super::home_layout::{
    read_managed_home_layout, write_managed_home_layout, ManagedHomeLayoutPhase,
    ManagedHomeLayoutRecord, MANAGED_HOME_LAYOUT_FILE,
};
use super::preset_validation::validate_model;
use super::storage::{migrate_managed_app_sqlite_into_home, LEGACY_SQLITE_FILE_PREFIXES};
use super::test_support::*;
use super::workspace::{login_provider_id_from_profile_options, qualified_model_display_name};
use super::*;

#[test]
fn managed_app_electron_data_is_per_lifecycle_and_adopts_legacy_root_once() {
    let database = Database::in_memory();
    let root = std::env::temp_dir().join(format!("3r-electron-{}", Uuid::new_v4()));
    let instances_root = root.join("instances");
    let store = ManagedLaunchStore::for_test(
        database,
        instances_root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let legacy = root.join("managed-app-data");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("preferences"), "shared").unwrap();
    let first = instances_root.join("first");
    std::fs::create_dir_all(&first).unwrap();
    let first_electron = store.managed_app_electron_data_dir(&first).unwrap();
    assert_eq!(first_electron, first.join("electron-data"));
    assert_eq!(
        std::fs::read_to_string(first_electron.join("preferences")).unwrap(),
        "shared"
    );
    assert!(!legacy.exists());
    let second = instances_root.join("second");
    std::fs::create_dir_all(&second).unwrap();
    let second_electron = store.managed_app_electron_data_dir(&second).unwrap();
    assert_ne!(first_electron, second_electron);
    assert!(!second_electron.join("preferences").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn managed_app_sqlite_path_is_stable_per_launch_preset() {
    let database = Database::in_memory();
    let root = std::env::temp_dir().join(format!("3r-managed-sqlite-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let preset_id = format!("preset-{}", Uuid::new_v4());
    let first = store.managed_app_sqlite_path(&preset_id).unwrap();
    let second = store.managed_app_sqlite_path(&preset_id).unwrap();
    assert_eq!(first, second);
    assert!(!first.exists());
    assert!(store.managed_app_sqlite_path("../escape").is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn migrates_legacy_managed_app_sqlite_into_codex_home() {
    let root = std::env::temp_dir().join(format!("3r-managed-migrate-{}", Uuid::new_v4()));
    let old = root.join("managed-app-sqlite");
    let dest = root.join("codex-home");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    for suffix in ["", "-wal", "-shm"] {
        std::fs::write(old.join(format!("state_5.sqlite{suffix}")), suffix).unwrap();
    }
    std::fs::write(old.join("logs_5.sqlite"), "logs").unwrap();

    migrate_managed_app_sqlite_into_home(&old, &dest).unwrap();

    assert!(!old.exists());
    for suffix in ["", "-wal", "-shm"] {
        assert_eq!(
            std::fs::read_to_string(dest.join(format!("state_5.sqlite{suffix}"))).unwrap(),
            suffix
        );
    }
    assert_eq!(
        std::fs::read_to_string(dest.join("logs_5.sqlite")).unwrap(),
        "logs"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn refuses_sqlite_layout_conflict_without_changing_either_side() {
    let root = std::env::temp_dir().join(format!("3r-managed-conflict-{}", Uuid::new_v4()));
    let old = root.join("managed-app-sqlite");
    let dest = root.join("codex-home");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(old.join("state_5.sqlite"), "old").unwrap();
    std::fs::write(old.join("state_5.sqlite-wal"), "old-wal").unwrap();
    std::fs::write(dest.join("state_5.sqlite"), "new").unwrap();

    assert_eq!(
        migrate_managed_app_sqlite_into_home(&old, &dest).unwrap_err(),
        "managed_sqlite_layout_conflict"
    );
    assert_eq!(
        std::fs::read_to_string(old.join("state_5.sqlite")).unwrap(),
        "old"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("state_5.sqlite")).unwrap(),
        "new"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn skips_sqlite_migration_when_home_already_has_catalog() {
    let root = std::env::temp_dir().join(format!("3r-managed-skip-{}", Uuid::new_v4()));
    let old = root.join("managed-app-sqlite");
    let dest = root.join("codex-home");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("state_5.sqlite"), "new").unwrap();

    migrate_managed_app_sqlite_into_home(&old, &dest).unwrap();

    assert_eq!(
        std::fs::read_to_string(dest.join("state_5.sqlite")).unwrap(),
        "new"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn deleting_managed_app_storage_removes_home_sqlite_and_electron_data() {
    let database = Database::in_memory();
    seed_provider(&database, "provider", "provider-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-managed-delete-{}", Uuid::new_v4()));
    let instances_root = root.join("instances");
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        instances_root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let preset = store
        .create_preset(single_model_preset("provider", "deepseek-chat", None))
        .unwrap();
    let lifecycle = instances_root.join("instance-delete");
    let home = lifecycle.join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("auth.json"), "private").unwrap();
    let sqlite = home.join("state_5.sqlite");
    std::fs::write(&sqlite, "conversation").unwrap();
    let legacy_sqlite = store.managed_app_sqlite_path(&preset.id).unwrap();
    std::fs::create_dir_all(&legacy_sqlite).unwrap();
    std::fs::write(legacy_sqlite.join("state_5.sqlite"), "legacy").unwrap();
    let electron = lifecycle.join("electron-data");
    std::fs::create_dir_all(&electron).unwrap();
    std::fs::write(electron.join("preferences"), "private").unwrap();

    let connection = database.connection();
    let connection = connection.lock().unwrap();
    connection
        .execute(
            "INSERT INTO launch_configuration_revisions (
                id, launch_preset_id, launch_preset_version, credential_kind,
                profile_snapshot_hash, config_snapshot, codex_executable,
                codex_version, created_at
             ) VALUES ('config-delete', ?1, 1, 'api_key', 'hash',
                '{\"launchTarget\":\"managed_app\"}', '/bin/codex', 'test', 1)",
            [&preset.id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO codex_instances (
                id, launch_configuration_revision_id, credential_kind,
                api_key_provider_revision_id, managed_codex_home, lifecycle_dir,
                launch_nonce, state, launched_at, exited_at
             ) VALUES ('instance-delete', 'config-delete', 'api_key',
                'provider-r1', ?1, ?2, 'nonce-delete', 'exited', 1, 2)",
            rusqlite::params![home.display().to_string(), lifecycle.display().to_string()],
        )
        .unwrap();
    drop(connection);

    store.delete_instance_storage("instance-delete").unwrap();
    assert!(!lifecycle.exists());
    assert!(!sqlite.exists());
    assert!(!legacy_sqlite.exists());
    assert!(!electron.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn deleting_one_managed_app_keeps_shared_electron_data_while_another_record_exists() {
    let database = Database::in_memory();
    seed_provider(&database, "provider", "provider-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-managed-delete-shared-{}", Uuid::new_v4()));
    let instances_root = root.join("instances");
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        instances_root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let ended = store
        .create_preset(single_model_preset("provider", "deepseek-chat", None))
        .unwrap();
    let mut running_input = single_model_preset("provider", "deepseek-chat", None);
    running_input.name = "实例 4".to_string();
    running_input.create_new = true;
    let running = store.create_preset(running_input).unwrap();
    let ended_lifecycle = instances_root.join("instance-ended");
    let ended_home = ended_lifecycle.join("codex-home");
    let running_lifecycle = instances_root.join("instance-running");
    let running_home = running_lifecycle.join("codex-home");
    std::fs::create_dir_all(&ended_home).unwrap();
    std::fs::create_dir_all(&running_home).unwrap();
    std::fs::write(ended_home.join("state_5.sqlite"), "ended").unwrap();
    std::fs::write(running_home.join("state_5.sqlite"), "running").unwrap();
    let ended_legacy = store.managed_app_sqlite_path(&ended.id).unwrap();
    let running_legacy = store.managed_app_sqlite_path(&running.id).unwrap();
    std::fs::create_dir_all(&ended_legacy).unwrap();
    std::fs::create_dir_all(&running_legacy).unwrap();
    std::fs::write(ended_legacy.join("state_5.sqlite"), "ended-legacy").unwrap();
    std::fs::write(running_legacy.join("state_5.sqlite"), "running-legacy").unwrap();
    let ended_electron = ended_lifecycle.join("electron-data");
    let running_electron = running_lifecycle.join("electron-data");
    std::fs::create_dir_all(&ended_electron).unwrap();
    std::fs::create_dir_all(&running_electron).unwrap();
    std::fs::write(ended_electron.join("preferences"), "ended").unwrap();
    std::fs::write(running_electron.join("preferences"), "running").unwrap();

    let connection = database.connection();
    let connection = connection.lock().unwrap();
    for (config_id, preset_id, instance_id, home, lifecycle, state, exited_at) in [
        (
            "config-ended",
            ended.id.as_str(),
            "instance-ended",
            ended_home.display().to_string(),
            ended_lifecycle.display().to_string(),
            "exited",
            Some(2i64),
        ),
        (
            "config-running",
            running.id.as_str(),
            "instance-running",
            running_home.display().to_string(),
            running_lifecycle.display().to_string(),
            "running",
            None,
        ),
    ] {
        connection
            .execute(
                "INSERT INTO launch_configuration_revisions (
                    id, launch_preset_id, launch_preset_version, credential_kind,
                    profile_snapshot_hash, config_snapshot, codex_executable,
                    codex_version, created_at
                 ) VALUES (?1, ?2, 1, 'api_key', 'hash',
                    '{\"launchTarget\":\"managed_app\"}', '/bin/codex', 'test', 1)",
                rusqlite::params![config_id, preset_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO codex_instances (
                    id, launch_configuration_revision_id, credential_kind,
                    api_key_provider_revision_id, managed_codex_home, lifecycle_dir,
                    launch_nonce, state, launched_at, exited_at
                 ) VALUES (?1, ?2, 'api_key',
                    'provider-r1', ?3, ?4, ?1, ?5, 1, ?6)",
                rusqlite::params![instance_id, config_id, home, lifecycle, state, exited_at],
            )
            .unwrap();
    }
    drop(connection);

    store.delete_instance_storage("instance-ended").unwrap();
    assert!(!ended_lifecycle.exists());
    assert!(!ended_legacy.exists());
    assert_eq!(
        std::fs::read_to_string(running_home.join("state_5.sqlite")).unwrap(),
        "running"
    );
    assert_eq!(
        std::fs::read_to_string(running_legacy.join("state_5.sqlite")).unwrap(),
        "running-legacy"
    );
    assert_eq!(
        std::fs::read_to_string(running_electron.join("preferences")).unwrap(),
        "running"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(target_os = "macos")]
#[test]
fn managed_app_migrates_legacy_home_and_rewrites_rollout_paths() {
    use std::os::unix::fs::symlink;

    let database = Database::in_memory();
    seed_provider(&database, "provider", "provider-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-home-layout-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(single_model_preset("provider", "deepseek-chat", None))
        .unwrap();
    let fake_codex = write_fake_codex(&root, &["template", "deepseek-chat"]);
    let gateway = Url::parse("http://127.0.0.1:43123/v1").unwrap();
    let first = store
        .prepare_launch_with_target(
            &saved.id,
            saved.version,
            None,
            &fake_codex,
            "codex-test",
            Some(&gateway),
            &root,
            "managed_app",
        )
        .unwrap();
    let instance_id = first.instance_id.clone();
    let lifecycle = store.instances_root.join(&instance_id);
    let long_home = lifecycle.join("codex-home");
    let short_home = first.managed_codex_home.clone();

    store.revoke_binding(&instance_id).unwrap();
    {
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        connection
            .execute(
                "UPDATE codex_instances
                 SET managed_codex_home=?2, state='exited', exited_at=2
                 WHERE id=?1",
                params![&instance_id, long_home.display().to_string()],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE runtime_configuration_locks SET released_at=2 WHERE instance_id=?1",
                [&instance_id],
            )
            .unwrap();
    }
    std::fs::remove_file(&long_home).unwrap();
    std::fs::rename(&short_home, &long_home).unwrap();
    symlink(&long_home, &short_home).unwrap();
    std::fs::create_dir_all(long_home.join("sessions")).unwrap();
    let rollout = long_home.join("sessions/rollout.jsonl");
    std::fs::write(&rollout, "{\"type\":\"session_meta\"}\n").unwrap();
    let index = long_home.join("state_5.sqlite");
    let connection = Connection::open(&index).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE threads(id TEXT PRIMARY KEY, rollout_path TEXT, archived INTEGER);
             CREATE TABLE rollout_migration_skipped_rollouts(rollout_path TEXT);
             CREATE TABLE unrelated_paths(rollout_path TEXT);",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO threads VALUES('active', ?1, 0), ('archived', ?1, 1)",
            [rollout.display().to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO rollout_migration_skipped_rollouts VALUES(?1)",
            [rollout.display().to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO unrelated_paths VALUES(?1)",
            [format!(
                "{}-other/sessions/rollout.jsonl",
                long_home.display()
            )],
        )
        .unwrap();
    drop(connection);

    let second = store
        .prepare_launch_with_target(
            &saved.id,
            saved.version,
            None,
            &fake_codex,
            "codex-test",
            Some(&gateway),
            &root,
            "managed_app",
        )
        .unwrap();
    assert_eq!(second.instance_id, instance_id);
    assert_eq!(second.managed_codex_home, short_home);
    assert!(std::fs::symlink_metadata(&short_home).unwrap().is_dir());
    assert!(!std::fs::symlink_metadata(&short_home)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        long_home.canonicalize().unwrap(),
        short_home.canonicalize().unwrap()
    );
    let connection = Connection::open(short_home.join("state_5.sqlite")).unwrap();
    let rewritten: Vec<String> = connection
        .prepare("SELECT rollout_path FROM threads ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rewritten,
        vec![
            short_home
                .join("sessions/rollout.jsonl")
                .display()
                .to_string(),
            short_home
                .join("sessions/rollout.jsonl")
                .display()
                .to_string(),
        ]
    );
    let skipped: String = connection
        .query_row(
            "SELECT rollout_path FROM rollout_migration_skipped_rollouts",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        skipped,
        short_home
            .join("sessions/rollout.jsonl")
            .display()
            .to_string()
    );
    let untouched: String = connection
        .query_row("SELECT rollout_path FROM unrelated_paths", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        untouched,
        format!("{}-other/sessions/rollout.jsonl", long_home.display())
    );
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let persisted_home: String = connection
        .query_row(
            "SELECT managed_codex_home FROM codex_instances WHERE id=?1",
            [&instance_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted_home, short_home.display().to_string());
    drop(connection);
    store.delete_instance_storage(&instance_id).unwrap();
    assert!(!short_home.exists());
    assert!(!long_home.exists());
    assert!(!lifecycle.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(target_os = "macos")]
#[test]
fn managed_home_layout_resumes_after_rename_and_rejects_two_real_homes() {
    let database = Database::in_memory();
    let root = std::env::temp_dir().join(format!("3r-home-resume-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let instance_id = "12345678-1234-1234-1234-123456789abc";
    let lifecycle = store.instances_root.join(instance_id);
    let long_home = lifecycle.join("codex-home");
    let short_home = store
        .instances_root
        .parent()
        .unwrap()
        .join("r")
        .join("123456781234");
    std::fs::create_dir_all(&lifecycle).unwrap();
    std::fs::create_dir_all(&short_home).unwrap();
    let marker = lifecycle.join(MANAGED_HOME_LAYOUT_FILE);
    write_managed_home_layout(
        &marker,
        &ManagedHomeLayoutRecord {
            instance_id: instance_id.to_string(),
            long_home: long_home.display().to_string(),
            short_home: short_home.display().to_string(),
            phase: ManagedHomeLayoutPhase::Renaming,
        },
    )
    .unwrap();
    let (home, launch, pending_marker) = store
        .resume_macos_managed_app_home_layout(
            &marker,
            read_managed_home_layout(&marker).unwrap().unwrap(),
        )
        .unwrap();
    assert_eq!(home, short_home);
    assert_eq!(launch, short_home);
    assert_eq!(pending_marker, Some(marker.clone()));
    assert_eq!(
        long_home.canonicalize().unwrap(),
        short_home.canonicalize().unwrap()
    );

    std::fs::remove_file(&long_home).unwrap();
    std::fs::create_dir_all(&long_home).unwrap();
    let error = store.resume_macos_managed_app_home_layout(
        &marker,
        read_managed_home_layout(&marker).unwrap().unwrap(),
    );
    assert_eq!(error.unwrap_err(), "managed_home_layout_conflict");
    assert!(short_home.is_dir());
    assert!(long_home.is_dir());
    let _ = std::fs::remove_dir_all(root);
}
