//! The production selection and launch paths enforce global model exposure and
//! read only the current model/protocol's positive image-probe evidence.

use std::sync::Arc;

use super::test_support::*;
use super::*;
use crate::database::Database;
use crate::diagnostics::{
    CheckStatus, ConnectivityStatus, DiagnosticsStore, ProbeSnapshot, ProviderModelOption,
};
use crate::model_exposure::ModelExposureStore;

#[test]
fn saving_an_unexposed_model_rolls_back_the_entire_workspace() {
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("rcodex-exposure-save-{}", uuid::Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault),
    );
    let result = store.create_preset(grok_preset("relay", None, None));
    assert!(result.is_err());
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM launch_presets", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM model_route_identities", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    drop(connection);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn only_current_positive_image_evidence_enables_both_protocol_catalogs() {
    use std::path::Path;
    use url::Url;
    for wire in ["responses", "chat_completions"] {
        for verdict in ["unknown", "supported", "unsupported", "failed"] {
            let database = Database::in_memory();
            seed_provider(&database, "relay", "relay-r1", wire);
            DiagnosticsStore::from_database(database.clone())
                .store_channel_probe(
                    "relay",
                    "relay-r1",
                    &ProbeSnapshot {
                        status: ConnectivityStatus::Ready,
                        wire_api: Some(wire.to_string()),
                        confirmed_base_url: Some("https://example.com/v1".to_string()),
                        model: Some("grok-4.6".to_string()),
                        models: vec![ProviderModelOption {
                            upstream_model_id: "grok-4.6".to_string(),
                            display_name: "Grok 4.6".to_string(),
                            owned_by: None,
                        }],
                        models_check: CheckStatus::Ready,
                        text_check: CheckStatus::Ready,
                        error_code: None,
                    },
                )
                .unwrap();
            let exposures = ModelExposureStore::from_database(database.clone());
            assert!(
                exposures
                    .set_exposure("relay", "grok-4.6", true)
                    .unwrap()
                    .should_probe
            );
            if verdict != "unknown" {
                let context = exposures.probe_context("relay", "grok-4.6").unwrap();
                exposures
                    .save_probe_result(
                        &context,
                        verdict,
                        if verdict == "supported" {
                            None
                        } else {
                            Some("fixture_error")
                        },
                    )
                    .unwrap();
            }
            let root = std::env::temp_dir()
                .join(format!("rcodex-exposure-launch-{}", uuid::Uuid::new_v4()));
            let store =
                ManagedLaunchStore::for_test(database, root.clone(), Arc::new(MemoryInstanceVault));
            let saved = store
                .create_preset(grok_preset("relay", None, None))
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
            let supports = verdict == "supported";
            let route = &prepared.route_set.routes[0];
            assert_eq!(
                route.capabilities.supports_images, supports,
                "{wire}/{verdict}"
            );
            let catalog = catalog_model(Path::new(&prepared.route_set.catalog_path), "grok-4.6");
            assert_eq!(
                catalog["input_modalities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value == "image"),
                supports,
                "{wire}/{verdict}"
            );
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

#[cfg(unix)]
#[test]
fn launch_rechecks_exposure_even_when_selection_already_exists() {
    use url::Url;
    let database = Database::in_memory();
    seed_provider(&database, "relay", "relay-r1", "responses");
    let root = std::env::temp_dir().join(format!("rcodex-exposure-guard-{}", uuid::Uuid::new_v4()));
    let store = ManagedLaunchStore::for_test(
        database.clone(),
        root.clone(),
        Arc::new(MemoryInstanceVault),
    );
    let saved = store
        .create_exposed_test_preset(grok_preset("relay", None, None))
        .unwrap();
    database
        .connection()
        .lock()
        .unwrap()
        .execute(
            "UPDATE provider_model_exposures SET exposed=0 WHERE provider_id='relay'",
            [],
        )
        .unwrap();
    let fake_codex = write_fake_codex(&root, &["template", "grok-4.6"]);
    let result = store.prepare_launch_with_target(
        &saved.id,
        saved.version,
        None,
        &fake_codex,
        "codex-test",
        Some(&Url::parse("http://127.0.0.1:43123/v1").unwrap()),
        &root,
        "managed_app",
    );
    assert!(result.is_err());
    let count: i64 = database
        .connection()
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM codex_instances", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let _ = std::fs::remove_dir_all(root);
}
