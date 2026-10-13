//! 工作区写入：渠道、模型勾选、登录渠道和 composition。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::params;
use url::Url;
use uuid::Uuid;

use crate::codex_control::EntryKind;
use crate::database::Database;
use crate::launcher_protocol::LaunchTarget;
use crate::managed_codex_config::INSTANCE_GATEWAY_HEADER;
use crate::model_family_profile::{GROK_AUTO_COMPACT_TOKEN_LIMIT, GROK_CONTEXT_WINDOW};
use crate::routing::{CompactMode, RouteKind};

use super::home_layout::{
    read_managed_home_layout, write_managed_home_layout, MANAGED_HOME_LAYOUT_FILE,
};
use super::preset_validation::validate_model;
use super::storage::{migrate_managed_app_sqlite_into_home, LEGACY_SQLITE_FILE_PREFIXES};
use super::test_support::*;
use super::workspace::{login_provider_id_from_profile_options, qualified_model_display_name};
use super::*;

#[test]
fn internal_role_model_selection_is_rejected() {
    let model = ManagedModelSelectionInput {
        source_kind: RouteKind::ApiProvider,
        source_id: "provider-a".to_string(),
        upstream_model_id: "codex-auto-review".to_string(),
        public_model_id: "codex-auto-review".to_string(),
        display_name: "Codex auto review".to_string(),
        ..Default::default()
    };
    assert_eq!(
        validate_model(&model).unwrap_err(),
        "reserved_internal_model"
    );
}

#[test]
fn model_window_overrides_reject_zero_million_and_compact_above_window() {
    let valid = ManagedModelSelectionInput {
        source_kind: RouteKind::ApiProvider,
        source_id: "provider-a".to_string(),
        upstream_model_id: "grok-4.6".to_string(),
        public_model_id: "grok-4.6".to_string(),
        display_name: "Grok".to_string(),
        context_window: Some(500_000),
        auto_compact_token_limit: Some(475_000),
        reasoning_levels: Vec::new(),
        default_reasoning_level: None,
    };
    assert!(validate_model(&valid).is_ok());
    let million = ManagedModelSelectionInput {
        context_window: Some(1_000_000),
        auto_compact_token_limit: Some(950_000),
        ..valid.clone()
    };
    assert!(validate_model(&million).is_ok());
    for (context_window, auto_compact_token_limit) in [
        (Some(0), None),
        (Some(1_000_001), None),
        (None, Some(0)),
        (None, Some(1_000_001)),
        (Some(100_000), Some(100_001)),
    ] {
        let model = ManagedModelSelectionInput {
            context_window,
            auto_compact_token_limit,
            ..valid.clone()
        };
        assert_eq!(
            validate_model(&model).unwrap_err(),
            "invalid_model_window",
            "{context_window:?} {auto_compact_token_limit:?}"
        );
    }
}

#[test]
fn model_reasoning_contract_is_validated_and_persisted_per_provider_model() {
    let mut model = grok_selection("relay", None, None);
    model.reasoning_levels = vec!["low".to_string(), "high".to_string()];
    model.default_reasoning_level = Some("high".to_string());
    assert!(validate_model(&model).is_ok());

    let mut invalid = model.clone();
    invalid.reasoning_levels = vec!["max".to_string()];
    assert_eq!(
        validate_model(&invalid).unwrap_err(),
        "invalid_reasoning_levels"
    );
    invalid.reasoning_levels = vec!["low".to_string()];
    invalid.default_reasoning_level = Some("high".to_string());
    assert_eq!(
        validate_model(&invalid).unwrap_err(),
        "invalid_default_reasoning_level"
    );

    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-reasoning-persist-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let mut preset = grok_preset("relay", None, None);
    preset.models[0] = model;
    let saved = store.create_preset(preset).unwrap();
    let composition = store
        .compositions()
        .unwrap()
        .into_iter()
        .find(|item| item.launch_preset_id == saved.id)
        .unwrap();
    assert_eq!(
        composition.models[0].reasoning_levels,
        vec!["low".to_string(), "high".to_string()]
    );
    assert_eq!(
        composition.models[0].default_reasoning_level.as_deref(),
        Some("high")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_preset_persists_nullable_window_overrides() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-window-persist-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(grok_preset("relay", Some(200_000), Some(180_000)))
        .unwrap();
    let composition = store
        .compositions()
        .unwrap()
        .into_iter()
        .find(|item| item.launch_preset_id == saved.id)
        .unwrap();
    assert_eq!(composition.models[0].context_window, Some(200_000));
    assert_eq!(
        composition.models[0].auto_compact_token_limit,
        Some(180_000)
    );
    let cleared = store
        .create_preset(grok_preset("relay", None, None))
        .unwrap();
    let composition = store
        .compositions()
        .unwrap()
        .into_iter()
        .find(|item| item.launch_preset_id == cleared.id)
        .unwrap();
    assert_eq!(composition.models[0].context_window, None);
    assert_eq!(composition.models[0].auto_compact_token_limit, None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_preset_rejects_invalid_window_overrides() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-window-reject-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    for (context_window, auto_compact_token_limit) in [
        (Some(0), None),
        (Some(1_000_001), None),
        (None, Some(0)),
        (None, Some(1_000_001)),
        (Some(100_000), Some(100_001)),
    ] {
        assert_eq!(
            store
                .create_preset(grok_preset(
                    "relay",
                    context_window,
                    auto_compact_token_limit
                ))
                .unwrap_err(),
            "invalid_model_window",
            "{context_window:?} {auto_compact_token_limit:?}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn initial_workspace_seed_is_idempotent_and_preserves_configured_workspace() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("rcodex-initial-workspace-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );

    store.ensure_initial_workspace().unwrap();
    let initial = store.compositions().unwrap();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].state, "needs_model_selection");
    assert!(initial[0].api_key_provider_ids.is_empty());
    assert!(initial[0].models.is_empty());

    let configured = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: Some(EntryKind::ManagedApp.as_wire().to_string()),
            name: "R Codex".to_string(),
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
            auto_review_public_model_id: None,
            transport_mode: Some("gateway_passthrough".to_string()),
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();
    assert_eq!(configured.id, initial[0].launch_preset_id);

    store.ensure_initial_workspace().unwrap();
    let after_repeat = store.compositions().unwrap();
    assert_eq!(after_repeat.len(), 1);
    assert_eq!(after_repeat[0].launch_preset_id, configured.id);
    assert_eq!(after_repeat[0].state, "ready");
    assert_eq!(after_repeat[0].api_key_provider_ids, vec!["relay"]);

    assert_eq!(after_repeat[0].models[0].public_model_id, "deepseek-chat");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unconfirmed_provider_cannot_join_workspace() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    database
        .connection()
        .lock()
        .unwrap()
        .execute(
            "UPDATE provider_connectivity_reports SET wire_api=NULL WHERE provider_id='relay'",
            [],
        )
        .unwrap();
    let root = std::env::temp_dir().join(format!("rcodex-unconfirmed-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let error = store
        .create_preset(single_model_preset("relay", "deepseek-chat", None))
        .unwrap_err();
    assert_eq!(error, "protocol_unconfirmed");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn official_account_can_be_combined_with_api_models() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    seed_authorized_official_account(&database);
    let root = std::env::temp_dir().join(format!("3r-hybrid-preset-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: None,
            name: "Official identity + API models".to_string(),
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
    let composition = store
        .compositions()
        .unwrap()
        .into_iter()
        .find(|item| item.launch_preset_id == saved.id)
        .unwrap();
    assert_eq!(
        composition.official_account_id.as_deref(),
        Some("account-official")
    );
    assert_eq!(composition.models[0].source_kind, RouteKind::ApiProvider);
    assert_eq!(
        store.launch_transport(&saved.id).unwrap(),
        ManagedLaunchTransport::Gateway
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn managed_preset_write_updates_the_single_primary_workspace() {
    let database = Database::in_memory();
    seed_provider(&database, "provider-a", "provider-a-r1", "responses");
    seed_provider(&database, "provider-b", "provider-b-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-workspace-update-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );

    let first = store
        .create_preset(single_model_preset("provider-a", "model-a", None))
        .unwrap();
    let mut second_input = single_model_preset("provider-b", "model-b", None);
    second_input.name = "主工作区".to_string();
    let second = store.create_preset(second_input).unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(second.version, 2);
    assert_eq!(store.compositions().unwrap().len(), 1);
    let compositions = store.compositions().unwrap();
    let composition = &compositions[0];
    assert_eq!(composition.api_key_provider_ids, vec!["provider-b"]);
    assert_eq!(composition.models[0].public_model_id, "model-b");
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM launch_presets WHERE deleted_at IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    drop(connection);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_new_managed_preset_inserts_an_additional_workspace() {
    let database = Database::in_memory();
    seed_provider(&database, "provider-a", "provider-a-r1", "responses");
    seed_provider(&database, "provider-b", "provider-b-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-workspace-create-new-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );

    let first = store
        .create_preset(single_model_preset("provider-a", "model-a", None))
        .unwrap();
    let mut second_input = single_model_preset("provider-b", "model-b", None);
    second_input.name = "实例 2".to_string();
    second_input.create_new = true;
    let second = store.create_preset(second_input).unwrap();

    assert_ne!(first.id, second.id);
    assert_eq!(second.version, 1);
    let compositions = store.compositions().unwrap();
    assert_eq!(compositions.len(), 2);
    let second_composition = compositions
        .iter()
        .find(|item| item.launch_preset_id == second.id)
        .unwrap();
    assert_eq!(second_composition.api_key_provider_ids, vec!["provider-b"]);
    let selected = store.select_preset(&first.id).unwrap();
    assert_eq!(selected.id, first.id);
    assert_eq!(selected.version, first.version);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn managed_preset_persists_multiple_providers_and_stable_models() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    seed_provider(&database, "grok", "grok-r1", "chat_completions");
    let root = std::env::temp_dir().join(format!("3r-managed-launch-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: None,
            name: "DeepSeek + Grok".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec!["relay".to_string(), "grok".to_string()],
            models: vec![
                ManagedModelSelectionInput {
                    source_kind: RouteKind::ApiProvider,
                    source_id: "relay".to_string(),
                    upstream_model_id: "deepseek-chat".to_string(),
                    public_model_id: "deepseek-chat".to_string(),
                    display_name: "DeepSeek Chat".to_string(),
                    ..Default::default()
                },
                ManagedModelSelectionInput {
                    source_kind: RouteKind::ApiProvider,
                    source_id: "grok".to_string(),
                    upstream_model_id: "grok-4".to_string(),
                    public_model_id: "grok-4".to_string(),
                    display_name: "Grok 4".to_string(),
                    ..Default::default()
                },
            ],
            auto_review_public_model_id: None,
            transport_mode: None,
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();
    let composition = store
        .compositions()
        .unwrap()
        .into_iter()
        .find(|item| item.launch_preset_id == saved.id)
        .unwrap();
    assert_eq!(composition.api_key_provider_ids, vec!["relay", "grok"]);
    assert_eq!(
        composition
            .models
            .iter()
            .map(|model| model.public_model_id.as_str())
            .collect::<Vec<_>>(),
        vec!["deepseek-chat", "grok-4"]
    );
    assert!(store.is_ready_managed_preset(&saved.id).unwrap());

    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let projection: (String, String) = connection
        .query_row(
            "SELECT credential_kind, api_key_provider_id
             FROM launch_presets WHERE id=?1 AND deleted_at IS NULL",
            [&saved.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(projection, ("api_key".to_string(), "relay".to_string()));
    let gateway_policy: String = connection
        .query_row(
            "SELECT gateway_policy FROM launch_presets
             WHERE id=?1 AND deleted_at IS NULL",
            [&saved.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(gateway_policy, "gateway_passthrough");
    drop(connection);
    assert_eq!(
        store.launch_transport(&saved.id).unwrap(),
        ManagedLaunchTransport::Gateway
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workspace_edits_apply_on_next_launch_while_instance_is_running() {
    let database = Database::in_memory();
    seed_provider(&database, "first", "first-r1", "responses");
    seed_provider(&database, "second", "second-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-running-edit-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let mut input = single_model_preset("first", "deepseek-chat", None);
    input.api_key_provider_ids.push("second".to_string());
    let saved = store.create_preset(input).unwrap();
    {
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO launch_configuration_revisions (
                    id, launch_preset_id, launch_preset_version, credential_kind,
                    profile_snapshot_hash, config_snapshot, codex_executable,
                    codex_version, created_at
                 ) VALUES ('config-running', ?1, 1, 'api_key', 'hash', '{}', '/bin/codex', 'test', 1)",
                [&saved.id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO launch_configuration_dependencies (
                    launch_configuration_revision_id, resource_kind, resource_id
                 ) VALUES ('config-running', 'launch_preset', ?1)",
                [&saved.id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO codex_instances (
                    id, launch_configuration_revision_id, credential_kind,
                    api_key_provider_revision_id, managed_codex_home, lifecycle_dir,
                    launch_nonce, state, launched_at
                 ) VALUES ('instance-running', 'config-running', 'api_key',
                    'first-r1', ?1, ?2, 'nonce-running', 'running', 1)",
                rusqlite::params![
                    root.join("home").display().to_string(),
                    root.join("lifecycle").display().to_string(),
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO runtime_configuration_locks (
                    instance_id, launch_configuration_revision_id, acquired_at
                 ) VALUES ('instance-running', 'config-running', 1)",
                [],
            )
            .unwrap();
    }
    let mut next = single_model_preset("second", "deepseek-chat", None);
    next.launch_preset_id = Some(saved.id.clone());
    let updated = store.create_preset(next).unwrap();
    assert_eq!(updated.id, saved.id);
    assert!(updated.version > saved.version);
    let login = store
        .set_managed_login_provider(&updated.id, updated.version, Some("second"))
        .unwrap();
    assert!(login.version > updated.version);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn login_selection_is_versioned_and_cleared_when_channel_is_removed() {
    let database = Database::in_memory();
    seed_provider(&database, "first", "first-r1", "responses");
    seed_provider(&database, "second", "second-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-login-selection-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let mut input = single_model_preset("first", "deepseek-chat", None);
    input.api_key_provider_ids.push("second".to_string());
    let saved = store.create_preset(input).unwrap();
    assert_eq!(
        store
            .set_managed_login_provider(&saved.id, saved.version, Some("outside"))
            .unwrap_err(),
        "login_provider_not_in_workspace"
    );
    let selected = store
        .set_managed_login_provider(&saved.id, saved.version, Some("second"))
        .unwrap();
    assert_eq!(
        store
            .set_managed_login_provider(&saved.id, saved.version, Some("first"))
            .unwrap_err(),
        "version_conflict"
    );
    let read_selection = || {
        let connection = database.connection();
        let raw: String = connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT profile_options FROM launch_presets WHERE id=?1",
                [&saved.id],
                |row| row.get(0),
            )
            .unwrap();
        login_provider_id_from_profile_options(&raw)
    };
    assert_eq!(read_selection().as_deref(), Some("second"));
    let removed = store
        .create_preset(single_model_preset("first", "deepseek-chat", None))
        .unwrap();
    assert_eq!(removed.id, saved.id);
    assert!(removed.version > selected.version);
    assert_eq!(read_selection(), None);
    database.connection().lock().unwrap().execute(
        "INSERT INTO resource_operation_leases (resource_kind,resource_id,operation_id,operation_kind,state,acquired_at)
         VALUES ('launch_preset',?1,'import','session_import','active',1)", [&saved.id],
    ).unwrap();
    assert_eq!(
        store
            .set_managed_login_provider(&saved.id, removed.version, Some("first"))
            .unwrap_err(),
        "resource_operation_in_progress"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn adding_a_channel_keeps_the_existing_login_and_default_model() {
    let database = Database::in_memory();
    seed_provider(&database, "first", "first-r1", "responses");
    seed_provider(&database, "second", "second-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-add-channel-keep-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(single_model_preset("first", "deepseek-chat", None))
        .unwrap();
    let mut added = single_model_preset("first", "deepseek-chat", None);
    added.launch_preset_id = Some(saved.id.clone());
    added.api_key_provider_ids.insert(0, "second".to_string());
    let updated = store.create_preset(added).unwrap();
    assert_eq!(updated.id, saved.id);
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let login: String = connection
        .query_row(
            "SELECT profile_options FROM launch_presets WHERE id=?1",
            [&saved.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        login_provider_id_from_profile_options(&login).as_deref(),
        Some("first")
    );
    let providers: Vec<String> = connection
        .prepare(
            "SELECT provider_id FROM launch_preset_api_providers
             WHERE launch_preset_id=?1 ORDER BY position",
        )
        .unwrap()
        .query_map([&saved.id], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(providers, vec!["second", "first"]);
    let model: String = connection
        .query_row(
            "SELECT identity.public_model_id
             FROM launch_preset_model_selections selection
             JOIN model_route_identities identity
               ON identity.id = selection.route_identity_id
             WHERE selection.launch_preset_id=?1
             ORDER BY selection.position
             LIMIT 1",
            [&saved.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(model, "deepseek-chat");
    drop(connection);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn managed_preset_qualifies_public_id_when_another_provider_owns_it() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    seed_provider(&database, "grok", "grok-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-public-id-collision-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );

    store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: None,
            name: "Relay workspace".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec!["relay".to_string()],
            models: vec![ManagedModelSelectionInput {
                source_kind: RouteKind::ApiProvider,
                source_id: "relay".to_string(),
                upstream_model_id: "shared-model".to_string(),
                public_model_id: "shared-model".to_string(),
                display_name: "Shared model".to_string(),
                ..Default::default()
            }],
            auto_review_public_model_id: None,
            transport_mode: Some("gateway_passthrough".to_string()),
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();

    let saved = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: None,
            name: "Grok workspace".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec!["grok".to_string()],
            models: vec![ManagedModelSelectionInput {
                source_kind: RouteKind::ApiProvider,
                source_id: "grok".to_string(),
                upstream_model_id: "shared-model".to_string(),
                public_model_id: "shared-model".to_string(),
                display_name: "Shared model".to_string(),
                ..Default::default()
            }],
            auto_review_public_model_id: None,
            transport_mode: Some("gateway_passthrough".to_string()),
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();
    let composition = store
        .compositions()
        .unwrap()
        .into_iter()
        .find(|item| item.launch_preset_id == saved.id)
        .unwrap();
    assert_eq!(composition.models[0].public_model_id, "shared-model-grok");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn managed_preset_can_remove_last_provider_without_invalid_credentials_error() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-empty-composition-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let initial = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: Some(EntryKind::ManagedApp.as_wire().to_string()),
            name: "R Codex".to_string(),
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
            auto_review_public_model_id: None,
            transport_mode: None,
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();

    let updated = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: Some(EntryKind::ManagedApp.as_wire().to_string()),
            name: "R Codex".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec![],
            models: vec![],
            auto_review_public_model_id: None,
            transport_mode: None,
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();

    assert_eq!(updated.id, initial.id);
    let composition = store
        .compositions()
        .unwrap()
        .into_iter()
        .find(|item| item.launch_preset_id == updated.id)
        .unwrap();
    assert_eq!(composition.state, "needs_model_selection");
    assert!(composition.api_key_provider_ids.is_empty());
    assert!(composition.models.is_empty());
    assert!(!store.is_ready_managed_preset(&updated.id).unwrap());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn managed_preset_can_start_with_provider_and_no_model_selection() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-empty-workspace-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );

    let saved = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: Some(EntryKind::ManagedApp.as_wire().to_string()),
            name: "R Codex".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec!["relay".to_string()],
            models: vec![],
            auto_review_public_model_id: None,
            transport_mode: Some("gateway_passthrough".to_string()),
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();

    let composition = store
        .compositions()
        .unwrap()
        .into_iter()
        .find(|item| item.launch_preset_id == saved.id)
        .unwrap();
    assert_eq!(composition.state, "needs_model_selection");
    assert_eq!(composition.api_key_provider_ids, vec!["relay"]);
    assert!(composition.models.is_empty());
    assert!(!store.is_ready_managed_preset(&saved.id).unwrap());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn auto_review_role_persists_and_forces_gateway_for_direct_provider() {
    let database = Database::in_memory();
    seed_direct_provider(&database, "grok", "grok-r1");
    let root = std::env::temp_dir().join(format!("3r-managed-review-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(single_model_preset(
            "grok",
            "grok-4.6-latest",
            Some("grok-4.6-latest"),
        ))
        .unwrap();
    let composition = store
        .compositions()
        .unwrap()
        .into_iter()
        .find(|item| item.launch_preset_id == saved.id)
        .unwrap();
    assert_eq!(
        composition.auto_review_public_model_id.as_deref(),
        Some("grok-4.6-latest")
    );
    assert_eq!(
        store.launch_transport(&saved.id).unwrap(),
        ManagedLaunchTransport::Gateway
    );
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let policy: String = connection
        .query_row(
            "SELECT gateway_policy FROM launch_presets WHERE id=?1",
            [&saved.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(policy, "gateway_passthrough");
    drop(connection);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn auto_review_role_rejects_unselected_and_chat_only_models() {
    let database = Database::in_memory();
    seed_provider(&database, "responses", "responses-r1", "responses");
    seed_provider(&database, "chat", "chat-r1", "chat_completions");
    let root = std::env::temp_dir().join(format!("3r-managed-review-invalid-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    assert_eq!(
        store
            .create_preset(single_model_preset(
                "responses",
                "grok-4.6-latest",
                Some("other-model"),
            ))
            .unwrap_err(),
        "auto_review_model_not_selected"
    );
    assert_eq!(
        store
            .create_preset(single_model_preset(
                "chat",
                "grok-4.6-latest",
                Some("grok-4.6-latest"),
            ))
            .unwrap_err(),
        "auto_review_model_requires_responses"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn multiple_direct_providers_always_use_gateway_transport() {
    let database = Database::in_memory();
    seed_direct_provider(&database, "direct-a", "direct-a-r1");
    seed_direct_provider(&database, "direct-b", "direct-b-r1");
    let root = std::env::temp_dir().join(format!("3r-managed-multi-direct-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: None,
            name: "Multiple Direct Providers".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec!["direct-a".to_string(), "direct-b".to_string()],
            models: vec![
                ManagedModelSelectionInput {
                    source_kind: RouteKind::ApiProvider,
                    source_id: "direct-a".to_string(),
                    upstream_model_id: "model-a".to_string(),
                    public_model_id: "model-a".to_string(),
                    display_name: "Model A".to_string(),
                    ..Default::default()
                },
                ManagedModelSelectionInput {
                    source_kind: RouteKind::ApiProvider,
                    source_id: "direct-b".to_string(),
                    upstream_model_id: "model-b".to_string(),
                    public_model_id: "model-b".to_string(),
                    display_name: "Model B".to_string(),
                    ..Default::default()
                },
            ],
            auto_review_public_model_id: None,
            transport_mode: None,
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();
    assert_eq!(
        store.launch_transport(&saved.id).unwrap(),
        ManagedLaunchTransport::Gateway
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn same_upstream_model_across_providers_requires_distinct_public_routes() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    seed_provider(&database, "grok", "grok-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-managed-duplicate-model-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: None,
            name: "同名模型分流".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec!["relay".to_string(), "grok".to_string()],
            models: vec![
                ManagedModelSelectionInput {
                    source_kind: RouteKind::ApiProvider,
                    source_id: "relay".to_string(),
                    upstream_model_id: "gpt-4o".to_string(),
                    public_model_id: "relay-gpt-4o".to_string(),
                    display_name: "Relay / GPT-4o".to_string(),
                    ..Default::default()
                },
                ManagedModelSelectionInput {
                    source_kind: RouteKind::ApiProvider,
                    source_id: "grok".to_string(),
                    upstream_model_id: "gpt-4o".to_string(),
                    public_model_id: "grok-gpt-4o".to_string(),
                    display_name: "Grok / GPT-4o".to_string(),
                    ..Default::default()
                },
            ],
            auto_review_public_model_id: None,
            transport_mode: None,
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap();
    let composition = store
        .compositions()
        .unwrap()
        .into_iter()
        .find(|item| item.launch_preset_id == saved.id)
        .unwrap();
    assert_eq!(
        composition
            .models
            .iter()
            .map(|model| (model.public_model_id.as_str(), model.display_name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("relay-gpt-4o", "Relay / GPT-4o"),
            ("grok-gpt-4o", "Grok / GPT-4o"),
        ]
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn qualified_model_display_name_is_idempotent() {
    assert_eq!(
        qualified_model_display_name("Relay", "GPT-4o"),
        "Relay/GPT-4o"
    );
    assert_eq!(
        qualified_model_display_name("Relay", "Relay/GPT-4o"),
        "Relay/GPT-4o"
    );
    assert_eq!(
        qualified_model_display_name("Relay", "Relay / GPT-4o"),
        "Relay/GPT-4o"
    );
    assert_eq!(
        qualified_model_display_name("Relay", "Relay/Relay/GPT-4o"),
        "Relay/GPT-4o"
    );
}

#[test]
fn model_source_must_belong_to_the_preset() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-managed-launch-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let error = store
        .create_preset(CreateManagedLaunchPresetInput {
            entry_kind: None,
            name: "Invalid".to_string(),
            official_account_id: None,
            api_key_provider_ids: vec!["relay".to_string()],
            models: vec![ManagedModelSelectionInput {
                source_kind: RouteKind::ApiProvider,
                source_id: "other".to_string(),
                upstream_model_id: "model".to_string(),
                public_model_id: "model".to_string(),
                display_name: "Model".to_string(),
                ..Default::default()
            }],
            auto_review_public_model_id: None,
            transport_mode: None,
            default_workspace_path: None,

            create_new: false,
            launch_preset_id: None,
        })
        .unwrap_err();
    assert_eq!(error, "route_source_not_selected");
    let _ = std::fs::remove_dir_all(root);
}
