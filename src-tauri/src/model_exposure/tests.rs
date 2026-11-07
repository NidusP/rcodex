use rusqlite::{params, TransactionBehavior};

use super::*;
use crate::database::Database;

fn fixture() -> (Database, ModelExposureStore) {
    let database = Database::in_memory();
    {
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        connection.execute_batch(
            "INSERT INTO api_key_providers(id,name,base_url,secret_ref,is_default,configured,created_at)
             VALUES ('p','Channel','https://example.invalid/v1','p-key',1,1,1);
             INSERT INTO api_key_provider_revisions(id,provider_id,secret_ref,base_url,wire_api,transport_mode,state,created_at)
             VALUES ('r','p','r-key','https://example.invalid/v1','responses','gateway_passthrough','active',1);
             UPDATE api_key_providers SET current_revision_id='r' WHERE id='p';
             INSERT INTO provider_model_catalog_revisions(id,provider_id,provider_revision_id,normalized_by,payload_sha256,state,discovered_at)
             VALUES ('catalog','p','r','test','sha256:test','ready',1);
             INSERT INTO provider_model_catalog_entries(catalog_revision_id,upstream_model_id,display_name,capabilities_json,position)
             VALUES ('catalog','vision-chat','Vision','{}',0),('catalog','text-chat','Text','{}',1),
               ('catalog','gpt-image-2','Image','{}',2),('catalog','codex-auto-review','Role','{}',3);
             INSERT INTO provider_connectivity_reports(provider_id,checked_at,status,model,models_check,text_check,wire_api,catalog_revision_id)
             VALUES ('p',1,'ready','vision-chat','ready','ready','responses','catalog');"
        ).unwrap();
    }
    let store = ModelExposureStore::from_database(database.clone());
    (database, store)
}

fn select_model(database: &Database, preset: &str, name: &str) {
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    connection.execute(
        "INSERT INTO launch_presets(id,name,credential_kind,api_key_provider_id,gateway_policy,profile_options,version,is_preferred,created_at,updated_at,entry_kind)
         VALUES (?1,?2,'api_key','p','gateway_passthrough','{}',1,0,1,1,'managed_app')", params![preset,name]
    ).unwrap();
    connection.execute(
        "INSERT INTO launch_preset_compositions(launch_preset_id,state,created_at,updated_at) VALUES (?1,'ready',1,1)", [preset]
    ).unwrap();
    connection.execute(
        "INSERT INTO launch_preset_api_providers(launch_preset_id,provider_id,position) VALUES (?1,'p',0)", [preset]
    ).unwrap();
    connection.execute(
        "INSERT OR IGNORE INTO model_route_identities(id,public_model_id,source_kind,api_key_provider_id,upstream_model_id,created_at)
         VALUES ('identity','vision-chat','api_provider','p','vision-chat',1)", []
    ).unwrap();
    connection.execute(
        "INSERT INTO launch_preset_model_selections(launch_preset_id,route_identity_id,position) VALUES (?1,'identity',0)", [preset]
    ).unwrap();
}

#[test]
fn ordinary_model_rules_match_product_exclusions() {
    for model in [
        "grok-4.6",
        "claude-sonnet",
        "vision-chat",
        "image-understanding",
        "myimagen-model",
    ] {
        assert!(is_selectable_model_id(model), "{model}");
    }
    for model in [
        "",
        "gpt-image-2",
        "grok-imagine-image-2.0",
        "dall-e-3",
        "dalle3",
        "imagen4",
        "foo/imagen",
        "image",
        "image-2.0",
        "codex-auto-review",
        "gpt-code-review",
        "gpt-next-auto_subagent",
    ] {
        assert!(!is_selectable_model_id(model), "{model}");
    }
}

#[test]
fn first_exposure_needs_probe_but_repeat_and_cached_failure_do_not() {
    let (_, store) = fixture();
    assert_eq!(store.list("p").unwrap().len(), 2);
    let change = store.set_exposure("p", "vision-chat", true).unwrap();
    assert!(change.summary.exposed);
    assert!(change.should_probe);
    assert!(
        !store
            .set_exposure("p", "vision-chat", true)
            .unwrap()
            .should_probe
    );
    let context = store.probe_context("p", "vision-chat").unwrap();
    assert!(store
        .save_probe_result(&context, "failed", Some("image_probe_timeout"))
        .unwrap());
    store.set_exposure("p", "vision-chat", false).unwrap();
    let reexposed = store.set_exposure("p", "vision-chat", true).unwrap();
    assert!(!reexposed.should_probe);
    assert_eq!(reexposed.summary.image_probe_state, "failed");
    assert!(reexposed.summary.image_probe_checked_at.is_some());
}

#[test]
fn revision_changes_preserve_choice_and_do_not_reuse_old_evidence() {
    let (database, store) = fixture();
    store.set_exposure("p", "vision-chat", true).unwrap();
    let context = store.probe_context("p", "vision-chat").unwrap();
    store
        .save_probe_result(&context, "supported", None)
        .unwrap();
    {
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        assert!(image_supported(&connection, "p", "r", "vision-chat", "responses").unwrap());
        connection.execute_batch(
            "INSERT INTO api_key_provider_revisions(id,provider_id,secret_ref,base_url,wire_api,transport_mode,state,created_at)
             VALUES ('r2','p','r2-key','https://new.example.invalid/v1','chat_completions','gateway_chat_completions','active',2);
             UPDATE api_key_providers SET current_revision_id='r2' WHERE id='p';"
        ).unwrap();
        assert!(!image_supported(&connection, "p", "r", "vision-chat", "responses").unwrap());
        assert!(
            !image_supported(&connection, "p", "r2", "vision-chat", "chat_completions").unwrap()
        );
    }
    let summary = store.summary("p", "vision-chat").unwrap();
    assert!(summary.exposed);
    assert_eq!(summary.image_probe_state, "unknown");
    assert!(
        !store
            .set_exposure("p", "vision-chat", true)
            .unwrap()
            .should_probe
    );
    assert!(!store
        .save_probe_result(&context, "supported", None)
        .unwrap());
    assert!(store
        .probe_context("p", "vision-chat")
        .unwrap_err()
        .contains("protocol_unconfirmed"));
}

#[test]
fn cannot_cancel_a_model_used_by_any_undeleted_space() {
    let (database, store) = fixture();
    store.set_exposure("p", "vision-chat", true).unwrap();
    select_model(&database, "a", "隔离空间 A");
    select_model(&database, "b", "隔离空间 B");
    let error = store.set_exposure("p", "vision-chat", false).err().unwrap();
    assert!(error.starts_with("model_exposure_in_use:"));
    assert!(error.contains("隔离空间 A") && error.contains("隔离空间 B"));
    assert!(store.summary("p", "vision-chat").unwrap().exposed);
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    connection
        .execute(
            "DELETE FROM launch_preset_model_selections WHERE launch_preset_id='a'",
            [],
        )
        .unwrap();
    connection
        .execute("UPDATE launch_presets SET deleted_at=2 WHERE id='b'", [])
        .unwrap();
    drop(connection);
    assert!(
        !store
            .set_exposure("p", "vision-chat", false)
            .unwrap()
            .summary
            .exposed
    );
}

#[test]
fn rejects_roles_generators_unknown_models_and_unconfirmed_protocol() {
    let (database, store) = fixture();
    for id in ["gpt-image-2", "codex-auto-review", " vision-chat", ""] {
        assert!(store.set_exposure("p", id, true).is_err());
    }
    assert!(store.set_exposure("p", "invented-model", true).is_err());
    assert!(store.list("missing-provider").is_err());
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    connection
        .execute(
            "UPDATE provider_connectivity_reports SET wire_api=NULL WHERE provider_id='p'",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(store.set_exposure("p", "vision-chat", true).is_err());
    assert!(!store.summary("p", "vision-chat").unwrap().exposed);
}

#[test]
fn evidence_is_model_protocol_contract_and_exposure_scoped() {
    let (database, store) = fixture();
    store.set_exposure("p", "vision-chat", true).unwrap();
    store.set_exposure("p", "text-chat", true).unwrap();
    let context = store.probe_context("p", "vision-chat").unwrap();
    assert!(store
        .save_probe_result(&context, "supported", None)
        .unwrap());
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    assert!(!image_supported(&connection, "p", "r", "text-chat", "responses").unwrap());
    assert!(!image_supported(&connection, "p", "r", "vision-chat", "chat_completions").unwrap());
    connection
        .execute(
            "UPDATE provider_model_image_probes SET contract_version='old'",
            [],
        )
        .unwrap();
    assert!(!image_supported(&connection, "p", "r", "vision-chat", "responses").unwrap());
    drop(connection);
    assert_eq!(
        store.summary("p", "vision-chat").unwrap().image_probe_state,
        "unknown"
    );
    store.set_exposure("p", "vision-chat", false).unwrap();
    assert!(!store
        .save_probe_result(&context, "supported", None)
        .unwrap());
}

#[test]
fn evidence_rejects_response_text_and_invalid_state_pairs() {
    let (_, store) = fixture();
    store.set_exposure("p", "vision-chat", true).unwrap();
    let context = store.probe_context("p", "vision-chat").unwrap();
    for (state, code) in [
        ("failed", Some("key leaked in error")),
        ("supported", Some("error")),
        ("failed", None),
        ("unknown", None),
    ] {
        assert!(store.save_probe_result(&context, state, code).is_err());
    }
    assert_eq!(
        store.summary("p", "vision-chat").unwrap().image_probe_state,
        "unknown"
    );
}

#[test]
fn workspace_exposure_check_uses_existing_transaction() {
    let (database, store) = fixture();
    assert!(store.probe_context("p", "vision-chat").is_err());
    store.set_exposure("p", "vision-chat", true).unwrap();
    let connection = database.connection();
    let mut connection = connection.lock().unwrap();
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    assert_model_exposed(&transaction, "p", "vision-chat").unwrap();
    assert!(assert_model_exposed(&transaction, "p", "text-chat").is_err());
    transaction
        .execute("UPDATE provider_model_exposures SET exposed=0", [])
        .unwrap();
    assert!(assert_model_exposed(&transaction, "p", "vision-chat").is_err());
    transaction.rollback().unwrap();
    drop(connection);
    assert!(store.summary("p", "vision-chat").unwrap().exposed);
}
