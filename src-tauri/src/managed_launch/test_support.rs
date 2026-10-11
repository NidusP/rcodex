//! managed_launch 测试夹具。不覆盖产品行为，只给各行为面测试造 Database / Store。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::params;
use url::Url;
use uuid::Uuid;

use crate::codex_control::EntryKind;
use crate::database::Database;
use crate::launcher_protocol::LaunchTarget;
use crate::routing::RouteKind;

use super::*;

#[derive(Default)]
pub(super) struct MemoryInstanceVault;

pub(super) fn grok_selection(
    provider_id: &str,
    context_window: Option<u64>,
    auto_compact_token_limit: Option<u64>,
) -> ManagedModelSelectionInput {
    ManagedModelSelectionInput {
        source_kind: RouteKind::ApiProvider,
        source_id: provider_id.to_string(),
        upstream_model_id: "grok-4.6".to_string(),
        public_model_id: "grok-4.6".to_string(),
        display_name: "Grok 4.6".to_string(),
        context_window,
        auto_compact_token_limit,
        reasoning_levels: Vec::new(),
        default_reasoning_level: None,
    }
}

pub(super) fn grok_preset(
    provider_id: &str,
    context_window: Option<u64>,
    auto_compact_token_limit: Option<u64>,
) -> CreateManagedLaunchPresetInput {
    CreateManagedLaunchPresetInput {
        entry_kind: Some(EntryKind::ManagedApp.as_wire().to_string()),
        name: "Isolated".to_string(),
        official_account_id: None,
        api_key_provider_ids: vec![provider_id.to_string()],
        models: vec![grok_selection(
            provider_id,
            context_window,
            auto_compact_token_limit,
        )],
        auto_review_public_model_id: None,
        transport_mode: None,
        default_workspace_path: None,
        create_new: false,
        launch_preset_id: None,
    }
}

#[cfg(unix)]
pub(super) fn write_fake_codex(root: &Path, slugs: &[&str]) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let models = slugs
        .iter()
        .map(|slug| format!(r#"{{"slug":"{slug}","display_name":"{slug}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let fake_codex = root.join("fake-codex");
    std::fs::write(
        &fake_codex,
        format!(
            "#!/bin/sh\ncase \" $* \" in *\" debug \"*) printf '%s' '{{\"models\":[{models}]}}' ;; *) printf '%s' '{{}}' ;; esac\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake_codex, std::fs::Permissions::from_mode(0o700)).unwrap();
    fake_codex
}

pub(super) fn catalog_model(catalog_path: &Path, slug: &str) -> serde_json::Value {
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(catalog_path).unwrap()).unwrap();
    catalog["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["slug"] == slug)
        .cloned()
        .unwrap()
}

#[cfg(unix)]
pub(super) fn prepare_grok_launch(
    context_window: Option<u64>,
    auto_compact_token_limit: Option<u64>,
) -> (PreparedManagedLaunch, PathBuf) {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("3r-grok-window-launch-{}", Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database,
        root.clone(),
        Arc::new(MemoryInstanceVault::default()),
    );
    let saved = store
        .create_preset(grok_preset(
            "relay",
            context_window,
            auto_compact_token_limit,
        ))
        .unwrap();
    let fake_codex = write_fake_codex(&root, &["template", "grok-4.6"]);
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
    (prepared, root)
}

pub(super) fn seed_provider(database: &Database, id: &str, revision: &str, wire_api: &str) {
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    connection
        .execute(
            "INSERT INTO api_key_providers (
                id, name, base_url, secret_ref, is_default,
                transport_mode, configured, created_at, current_revision_id, updated_at
             ) VALUES (?1, ?2, 'https://example.com/v1', ?3, 0,
                       'gateway_passthrough', 1, 1, NULL, 1)",
            params![id, format!("Provider {id}"), format!("provider:{id}")],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO api_key_provider_revisions (
                id, provider_id, secret_ref, base_url, wire_api,
                transport_mode, state, created_at
             ) VALUES (?1, ?2, ?3, 'https://example.com/v1', ?4,
                       'gateway_passthrough', 'active', 1)",
            params![revision, id, format!("provider:{id}"), wire_api],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE api_key_providers SET current_revision_id=?2 WHERE id=?1",
            params![id, revision],
        )
        .unwrap();
}

pub(super) fn seed_direct_provider(database: &Database, id: &str, revision: &str) {
    seed_provider(database, id, revision, "responses");
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    connection
        .execute(
            "UPDATE api_key_providers SET transport_mode='gateway_passthrough' WHERE id=?1",
            [id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE api_key_provider_revisions SET transport_mode='gateway_passthrough' WHERE id=?1",
            [revision],
        )
        .unwrap();
}

pub(super) fn seed_authorized_official_account(database: &Database) {
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    connection
        .execute_batch(
            "INSERT INTO codex_accounts (
                id, masked_identity, workspace_label, plan_label,
                current_credential_revision_id, created_at, updated_at
             ) VALUES ('account-official', 'a***@example.com', NULL, 'Pro',
                NULL, 1, 1);
             INSERT INTO official_credential_revisions (
                id, account_id, managed_codex_home, auth_state,
                auth_checked_at, auth_error_code, codex_version,
                schema_fingerprint, created_at, retired_at
             ) VALUES ('credential-official', 'account-official',
                '/managed/account-official', 'authorized', 1, NULL,
                'test', 'schema', 1, NULL);
             UPDATE codex_accounts
                SET current_credential_revision_id='credential-official'
              WHERE id='account-official';",
        )
        .unwrap();
}

pub(super) fn single_model_preset(
    provider_id: &str,
    public_model_id: &str,
    auto_review: Option<&str>,
) -> CreateManagedLaunchPresetInput {
    CreateManagedLaunchPresetInput {
        entry_kind: None,
        name: "Review role".to_string(),
        official_account_id: None,
        api_key_provider_ids: vec![provider_id.to_string()],
        models: vec![ManagedModelSelectionInput {
            source_kind: RouteKind::ApiProvider,
            source_id: provider_id.to_string(),
            upstream_model_id: public_model_id.to_string(),
            public_model_id: public_model_id.to_string(),
            display_name: "Review model".to_string(),
            ..Default::default()
        }],
        auto_review_public_model_id: auto_review.map(str::to_string),
        transport_mode: None,
        default_workspace_path: None,
        create_new: false,
        launch_preset_id: None,
    }
}
