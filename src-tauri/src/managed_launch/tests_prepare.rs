//! 准备启动：RouteSet 固化、推理档位、官方账户绑定。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::params;
use url::Url;
use uuid::Uuid;

use crate::codex_control::EntryKind;
use crate::database::Database;
use crate::launcher_protocol::LaunchTarget;
use crate::managed_codex_config::INSTANCE_GATEWAY_HEADER;
use crate::model_family_profile::{
    default_auto_compact_token_limit, GROK_AUTO_COMPACT_TOKEN_LIMIT, GROK_CONTEXT_WINDOW,
};
use crate::routing::{CompactMode, CompactionSupport, RouteKind};

use super::home_layout::{
    read_managed_home_layout, write_managed_home_layout, MANAGED_HOME_LAYOUT_FILE,
};
use super::preset_validation::validate_model;
use super::storage::{migrate_managed_app_sqlite_into_home, LEGACY_SQLITE_FILE_PREFIXES};
use super::test_support::*;
use super::workspace::{login_provider_id_from_profile_options, qualified_model_display_name};
use super::*;

#[cfg(unix)]
#[test]
fn grok_prepare_launch_uses_family_windows_when_selection_is_empty() {
    let (prepared, root) = prepare_grok_launch(None, None);
    let capabilities = &prepared.route_set.routes[0].capabilities;
    assert_eq!(capabilities.context_window, Some(GROK_CONTEXT_WINDOW));
    assert_eq!(
        capabilities.auto_compact_token_limit,
        Some(GROK_AUTO_COMPACT_TOKEN_LIMIT)
    );
    assert!(capabilities.supports_images);
    assert_eq!(
        capabilities.input_modalities,
        vec!["text".to_string(), "image".to_string()]
    );
    let grok = catalog_model(Path::new(&prepared.route_set.catalog_path), "grok-4.6");
    assert_eq!(grok["input_modalities"][0], "text");
    assert_eq!(grok["input_modalities"][1], "image");
    assert_eq!(grok["context_window"], GROK_CONTEXT_WINDOW);
    assert_eq!(grok["max_context_window"], GROK_CONTEXT_WINDOW);
    assert_eq!(
        grok["auto_compact_token_limit"],
        GROK_AUTO_COMPACT_TOKEN_LIMIT
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn grok_prepare_launch_prefers_ui_window_overrides() {
    let (prepared, root) = prepare_grok_launch(Some(200_000), Some(150_000));
    let capabilities = &prepared.route_set.routes[0].capabilities;
    assert_eq!(capabilities.context_window, Some(200_000));
    assert_eq!(capabilities.auto_compact_token_limit, Some(150_000));
    let grok = catalog_model(Path::new(&prepared.route_set.catalog_path), "grok-4.6");
    assert_eq!(grok["context_window"], 200_000);
    assert_eq!(grok["max_context_window"], 200_000);
    assert_eq!(grok["auto_compact_token_limit"], 150_000);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn grok_prepare_launch_defaults_compact_when_only_window_is_overridden() {
    let (prepared, root) = prepare_grok_launch(Some(200_000), None);
    let capabilities = &prepared.route_set.routes[0].capabilities;
    assert_eq!(capabilities.context_window, Some(200_000));
    assert_eq!(
        capabilities.auto_compact_token_limit,
        Some(default_auto_compact_token_limit(200_000))
    );
    let grok = catalog_model(Path::new(&prepared.route_set.catalog_path), "grok-4.6");
    assert_eq!(grok["auto_compact_token_limit"], 190_000);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn unknown_family_prepare_launch_strips_cloned_gpt_windows() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-unknown-window-launch-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: Some(EntryKind::ManagedApp.as_wire().to_string()),
            name: "Isolated".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec!["relay".to_string()],
            models: vec![ManagedModelSelectionInput {
                source_kind: RouteKind::ApiProvider,
                source_id: "relay".to_string(),
                upstream_model_id: "mystery-model".to_string(),
                public_model_id: "mystery-model".to_string(),
                display_name: "Mystery".to_string(),
                ..Default::default()
            }],
            auto_review_public_model_id: None,
            transport_mode: None,
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();
    let fake_codex = write_fake_codex(&root, &["template", "mystery-model"]);
    let prepared = store
        .prepare_launch_with_target(
            &saved.id,
            saved.version,
            None,
            &fake_codex,
            "codex-test",
            Some(&Url::parse("http://127.0.0.1:43123/v1").unwrap()),
            &root,
            "managed_app",
        )
        .unwrap();
    let capabilities = &prepared.route_set.routes[0].capabilities;
    assert_eq!(capabilities.context_window, None);
    assert_eq!(capabilities.auto_compact_token_limit, None);
    let model = catalog_model(Path::new(&prepared.route_set.catalog_path), "mystery-model");
    assert!(model.get("context_window").is_none());
    assert!(model.get("max_context_window").is_none());
    assert!(model.get("auto_compact_token_limit").is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn official_account_api_model_launch_binds_official_revision_in_route_set() {
    use std::os::unix::fs::PermissionsExt;

    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    seed_authorized_official_account(&database);
    let root = std::env::temp_dir().join(format!("3r-hybrid-launch-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: None,
            name: "Hybrid launch".to_string(),
            official_account_id: Some("account-official".to_string()),
            api_key_provider_ids: vec!["relay".to_string()],
            models: vec![ManagedModelSelectionInput {
                source_kind: RouteKind::ApiProvider,
                source_id: "relay".to_string(),
                upstream_model_id: "deepseek-chat".to_string(),
                public_model_id: "deepseek-chat".to_string(),
                display_name: "DeepSeek Chat".to_string(),
                ..Default::default()
            }],
            auto_review_public_model_id: None,
            transport_mode: None,
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();
    let fake_codex = root.join("fake-codex");
    std::fs::write(
        &fake_codex,
        "#!/bin/sh\ncase \" $* \" in *\" debug \"*) printf '%s' '{\"models\":[{\"slug\":\"template\",\"display_name\":\"Template\"},{\"slug\":\"deepseek-chat\",\"display_name\":\"DeepSeek Chat\"}]}' ;; *) printf '%s' '{}' ;; esac\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake_codex, std::fs::Permissions::from_mode(0o700)).unwrap();
    let prepared = store
        .prepare_launch_with_target(
            &saved.id,
            saved.version,
            None,
            &fake_codex,
            "codex-test",
            Some(&Url::parse("http://127.0.0.1:43123/v1").unwrap()),
            &root,
            "managed_app",
        )
        .unwrap();
    assert_eq!(
        prepared.route_set.official_account_id.as_deref(),
        Some("account-official")
    );
    assert_eq!(
        prepared
            .route_set
            .official_credential_revision_id
            .as_deref(),
        Some("credential-official")
    );
    assert!(prepared.route_set.requires_openai_auth);
    assert_eq!(
        prepared.route_set.routes[0].source.kind(),
        RouteKind::ApiProvider
    );
    assert_eq!(
        CompactMode::from_gateway_snapshot(&prepared.route_set.gateway_service.config_snapshot),
        CompactMode::LocalClient
    );
    assert_eq!(
        prepared.route_set.routes[0].capabilities.compaction_support,
        CompactionSupport::Unsupported
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn managed_app_reopens_the_same_runtime_binding_after_exit() {
    use std::os::unix::fs::PermissionsExt;

    let database = Database::in_memory();
    seed_provider(&database, "provider", "provider-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-managed-reopen-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(single_model_preset("provider", "deepseek-chat", None))
        .unwrap();
    let legacy_sqlite = store.managed_app_sqlite_path(&saved.id).unwrap();
    std::fs::create_dir_all(&legacy_sqlite).unwrap();
    std::fs::write(legacy_sqlite.join("state_5.sqlite"), "legacy").unwrap();
    std::fs::write(legacy_sqlite.join("state_5.sqlite-wal"), "legacy-wal").unwrap();
    let fake_codex = root.join("fake-codex");
    std::fs::write(
        &fake_codex,
        "#!/bin/sh\ncase \" $* \" in *\" debug \"*) printf '%s' '{\"models\":[{\"slug\":\"template\",\"display_name\":\"Template\"},{\"slug\":\"deepseek-chat\",\"display_name\":\"DeepSeek Chat\"}]}' ;; *) printf '%s' '{}' ;; esac\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake_codex, std::fs::Permissions::from_mode(0o700)).unwrap();
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
    let first_id = first.instance_id.clone();
    let first_home = first.launch_codex_home.clone();
    let first_sqlite = first.sqlite_home.clone();
    assert_eq!(first_sqlite, Some(first_home.clone()));
    assert!(!legacy_sqlite.exists());
    assert_eq!(
        std::fs::read_to_string(first.managed_codex_home.join("state_5.sqlite")).unwrap(),
        "legacy"
    );
    assert_eq!(
        std::fs::read_to_string(first.managed_codex_home.join("state_5.sqlite-wal")).unwrap(),
        "legacy-wal"
    );
    std::fs::create_dir_all(&legacy_sqlite).unwrap();
    std::fs::write(legacy_sqlite.join("state_5.sqlite"), "active-old").unwrap();
    std::fs::write(legacy_sqlite.join("state_5.sqlite-wal"), "active-old-wal").unwrap();
    let active_launch = store.prepare_launch_with_target(
        &saved.id,
        saved.version,
        None,
        &fake_codex,
        "codex-test",
        Some(&gateway),
        &root,
        "managed_app",
    );
    assert_eq!(active_launch.err().as_deref(), Some("managed_app_in_use"));
    assert!(legacy_sqlite.join("state_5.sqlite-wal").exists());
    std::fs::remove_dir_all(&legacy_sqlite).unwrap();
    std::fs::write(&first.status_file, "stale receipt").unwrap();
    store.revoke_binding(&first_id).unwrap();
    {
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        connection
            .execute(
                "UPDATE codex_instances SET state='exited', exited_at=2 WHERE id=?1",
                [&first_id],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE runtime_configuration_locks SET released_at=2 WHERE instance_id=?1",
                [&first_id],
            )
            .unwrap();
    }
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
    assert_eq!(second.instance_id, first_id);
    assert_eq!(second.launch_codex_home, first_home);
    assert_eq!(second.sqlite_home, first_sqlite);
    assert_eq!(second.sqlite_home, Some(second.launch_codex_home.clone()));
    assert!(!second.status_file.exists());
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let instance_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM codex_instances WHERE id=?1",
            [&second.instance_id],
            |row| row.get(0),
        )
        .unwrap();
    let configuration_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM launch_configuration_revisions WHERE launch_preset_id=?1",
            [&saved.id],
            |row| row.get(0),
        )
        .unwrap();
    let binding_state: String = connection
        .query_row(
            "SELECT state FROM instance_gateway_bindings WHERE instance_id=?1",
            [&second.instance_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(instance_count, 1);
    assert_eq!(configuration_count, 2);
    assert_eq!(binding_state, "active");
    drop(connection);
    store.revoke_binding(&second.instance_id).unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn prepare_launch_publishes_route_set_binding_and_instance_atomically() {
    use std::os::unix::fs::PermissionsExt;

    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-managed-launch-{}", Uuid::new_v4()));
    let vault = Arc::new(MemoryInstanceVault::default());
    let store = ManagedLaunchStore::for_test(database.clone(), root.clone(), vault.clone());
    let saved = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: None,
            name: "DeepSeek".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec!["relay".to_string()],
            models: vec![ManagedModelSelectionInput {
                source_kind: RouteKind::ApiProvider,
                source_id: "relay".to_string(),
                upstream_model_id: "deepseek-chat".to_string(),
                public_model_id: "deepseek-chat".to_string(),
                display_name: "DeepSeek Chat".to_string(),
                ..Default::default()
            }],
            auto_review_public_model_id: Some("deepseek-chat".to_string()),
            transport_mode: None,
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let gateway_policy: String = connection
        .query_row(
            "SELECT gateway_policy FROM launch_presets WHERE id=?1",
            [&saved.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(gateway_policy, "gateway_passthrough");
    drop(connection);
    let fake_codex = root.join("fake-codex");
    std::fs::write(
        &fake_codex,
        "#!/bin/sh\ncase \" $* \" in *\" debug \"*) printf '%s' '{\"models\":[{\"slug\":\"template\",\"display_name\":\"Template\"},{\"slug\":\"deepseek-chat\",\"display_name\":\"DeepSeek Chat\"}]}' ;; *) printf '%s' '{}' ;; esac\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake_codex, std::fs::Permissions::from_mode(0o700)).unwrap();
    let prepared = store
        .prepare_launch(
            &saved.id,
            saved.version,
            None,
            &fake_codex,
            "codex-test",
            Some(&Url::parse("http://127.0.0.1:43123/v1").unwrap()),
            &root,
        )
        .unwrap();
    assert_eq!(prepared.route_set.providers.len(), 1);
    assert_eq!(
        prepared.route_set.routes[0].public_model_id,
        "deepseek-chat"
    );
    assert_eq!(
        CompactMode::from_gateway_snapshot(&prepared.route_set.gateway_service.config_snapshot),
        CompactMode::LocalClient
    );
    assert_eq!(
        prepared.route_set.routes[0].capabilities.compaction_support,
        CompactionSupport::Unsupported
    );
    let managed_home = std::fs::canonicalize(&prepared.launch_codex_home).unwrap();
    assert!(managed_home
        .join(format!("{}.config.toml", prepared.profile_name))
        .is_file());
    assert!(prepared.launch_codex_home.exists());
    assert!(prepared.launch_codex_home.to_string_lossy().len() < 110);
    assert_ne!(prepared.launch_codex_home, managed_home);
    let profile = std::fs::read_to_string(
        prepared
            .launch_codex_home
            .join(format!("{}.config.toml", prepared.profile_name)),
    )
    .unwrap();
    assert!(!profile.contains("approvals_reviewer"));
    assert!(profile.contains("approval_policy = \"on-request\""));
    assert!(profile.contains("allow_symlinked_codex_home = true"));
    let catalog: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            prepared
                .launch_codex_home
                .join(format!("{}.models.json", prepared.profile_name)),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        catalog["models"][0]["auto_review_model_override"],
        "deepseek-chat"
    );
    assert!(prepared.instance_key.is_some());
    let home_config = std::fs::read_to_string(managed_home.join("config.toml")).unwrap();
    assert!(home_config.contains("model_provider = \"rcodex\""));
    assert!(home_config.contains("approval_policy = \"on-request\""));
    assert!(!home_config.contains("approvals_reviewer"));
    assert!(managed_home.join(".rcodex-preference-seed").is_file());

    let prepared_app = store
        .prepare_launch_with_target(
            &saved.id,
            saved.version,
            Some("/path/that/does/not/exist"),
            &fake_codex,
            "codex-test",
            Some(&Url::parse("http://127.0.0.1:43123/v1").unwrap()),
            &root,
            "managed_app",
        )
        .unwrap();
    assert_eq!(prepared_app.workspace_path, None);
    assert_eq!(
        CompactMode::from_gateway_snapshot(&prepared_app.route_set.gateway_service.config_snapshot),
        CompactMode::LocalClient
    );
    assert_eq!(
        prepared_app.route_set.routes[0]
            .capabilities
            .compaction_support,
        CompactionSupport::Unsupported
    );
    assert!(prepared_app.instance_key.is_some());
    assert!(prepared_app.launch_codex_home.exists());
    assert!(std::fs::symlink_metadata(&prepared_app.launch_codex_home)
        .unwrap()
        .is_dir());
    assert!(!std::fs::symlink_metadata(&prepared_app.launch_codex_home)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        prepared_app.managed_codex_home,
        prepared_app.launch_codex_home
    );
    assert!(prepared_app
        .sqlite_home
        .as_deref()
        .is_some_and(Path::is_dir));
    assert_eq!(
        prepared_app
            .electron_user_data_dir
            .as_deref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str()),
        Some("electron-data")
    );
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let persisted_workspace: Option<String> = connection
        .query_row(
            "SELECT workspace_path FROM codex_instances WHERE id=?1",
            [&prepared_app.instance_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted_workspace, None);
    drop(connection);
    store.revoke_binding(&prepared_app.instance_id).unwrap();
    assert!(prepared_app.launch_codex_home.is_dir());

    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let state: (String, String, String) = connection
        .query_row(
            "SELECT instance.state, binding.state, route_set.state
             FROM codex_instances instance
             JOIN instance_gateway_bindings binding ON binding.instance_id=instance.id
             JOIN route_set_revisions route_set ON route_set.id=binding.route_set_revision_id
             WHERE instance.id=?1",
            [&prepared.instance_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        state,
        (
            "launching".to_string(),
            "active".to_string(),
            "ready".to_string()
        )
    );
    drop(connection);
    store.revoke_binding(&prepared.instance_id).unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn prepare_launch_uses_gateway_when_auto_review_is_enabled() {
    use std::os::unix::fs::PermissionsExt;

    let database = Database::in_memory();
    seed_direct_provider(&database, "direct", "direct-r1");
    let root = std::env::temp_dir().join(format!("3r-managed-direct-{}", Uuid::new_v4()));
    let vault = Arc::new(MemoryInstanceVault::default());
    let store = ManagedLaunchStore::for_test(database.clone(), root.clone(), vault.clone());
    let saved = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: None,
            name: "Direct".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec!["direct".to_string()],
            models: vec![ManagedModelSelectionInput {
                source_kind: RouteKind::ApiProvider,
                source_id: "direct".to_string(),
                upstream_model_id: "deepseek-chat".to_string(),
                public_model_id: "deepseek-chat".to_string(),
                display_name: "DeepSeek Chat".to_string(),
                ..Default::default()
            }],
            auto_review_public_model_id: None,
            transport_mode: None,
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let gateway_policy: String = connection
        .query_row(
            "SELECT gateway_policy FROM launch_presets WHERE id=?1",
            [&saved.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(gateway_policy, "direct");
    drop(connection);
    assert_eq!(
        store.launch_transport(&saved.id).unwrap(),
        ManagedLaunchTransport::Gateway
    );
    let fake_codex = root.join("fake-codex");
    std::fs::write(
        &fake_codex,
        "#!/bin/sh\ncase \" $* \" in *\" debug \"*) printf '%s' '{\"models\":[{\"slug\":\"template\",\"display_name\":\"Template\"},{\"slug\":\"deepseek-chat\",\"display_name\":\"DeepSeek Chat\"}]}' ;; *) printf '%s' '{}' ;; esac\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake_codex, std::fs::Permissions::from_mode(0o700)).unwrap();
    let prepared = store
        .prepare_launch(
            &saved.id,
            saved.version,
            None,
            &fake_codex,
            "codex-test",
            Some(&Url::parse("http://127.0.0.1:43123/v1").unwrap()),
            &root,
        )
        .unwrap();
    assert_eq!(prepared.transport_mode, ManagedLaunchTransport::Gateway);
    assert!(prepared.instance_key_ref.is_some());
    assert!(prepared.instance_key_hash.is_some());
    let profile = std::fs::read_to_string(
        prepared
            .launch_codex_home
            .join(format!("{}.config.toml", prepared.profile_name)),
    )
    .unwrap();
    assert!(profile.contains("model_provider = \"rcodex\""));
    assert!(!profile.contains("approvals_reviewer"));
    assert!(profile.contains("approval_policy = \"on-request\""));
    assert!(profile.contains("allow_symlinked_codex_home = true"));
    assert!(profile.contains("http://127.0.0.1:43123/v1"));
    assert!(profile.contains(INSTANCE_GATEWAY_HEADER));
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let binding_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM instance_gateway_bindings WHERE instance_id=?1",
            [&prepared.instance_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(binding_count, 1);
    drop(connection);
    let _ = std::fs::remove_dir_all(root);
}
