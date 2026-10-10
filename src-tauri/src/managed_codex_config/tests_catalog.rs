//! 隔离目录生成、compact 合同和 catalog probe。

use std::{fs, path::Path};

use serde_json::Value as JsonValue;
use url::Url;
use uuid::Uuid;

use crate::routing::{CapabilitySet, CompactMode, ModelRoute, RouteSource, WireApi};

use super::*;

fn route() -> ModelRoute {
    ModelRoute {
        id: "route-1".to_string(),
        route_identity_id: "identity-1".to_string(),
        public_model_id: "relay-model".to_string(),
        display_name: "Relay/Model".to_string(),
        source: RouteSource::ApiProvider {
            provider_id: "provider-1".to_string(),
            provider_revision_id: "revision-1".to_string(),
        },
        upstream_model_id: "relay-model".to_string(),
        wire_api: WireApi::Responses,
        adapter_kind: "responses_passthrough".to_string(),
        adapter_version: "1".to_string(),
        capabilities: CapabilitySet::default(),
        position: 0,
    }
}

#[test]
fn isolated_catalog_contains_only_selected_routes() {
    let catalog = serde_json::json!({"models": [{"slug": "template", "display_name": "Template"}]});
    let generated = build_version_matched_catalog(&catalog, &[route()]).unwrap();
    assert_eq!(generated["models"].as_array().unwrap().len(), 1);
    assert_eq!(generated["models"][0]["slug"], "relay-model");
    assert_eq!(
        generated["models"][0]["supported_reasoning_levels"],
        serde_json::json!([
            {"effort": "low", "description": "Fast responses with lighter reasoning"},
            {"effort": "medium", "description": "Balances speed and reasoning depth for everyday tasks"},
            {"effort": "high", "description": "Greater reasoning depth for complex problems"},
            {"effort": "xhigh", "description": "Extra high reasoning depth for complex problems"}
        ])
    );
    assert_eq!(generated["models"][0]["priority"], 0);
    assert_eq!(generated["models"][0]["default_reasoning_level"], "high");
}

#[test]
fn isolated_catalog_sorts_by_provider_and_model_without_using_route_position() {
    let catalog = serde_json::json!({"models": [{"slug": "template", "display_name": "Template"}]});
    let mut later_provider = route();
    later_provider.public_model_id = "later-model".to_string();
    later_provider.display_name = "Zulu/Alpha".to_string();
    later_provider.position = 0;
    let mut earlier_provider = route();
    earlier_provider.public_model_id = "earlier-model".to_string();
    earlier_provider.display_name = "Alpha/Zulu".to_string();
    earlier_provider.position = 1;

    let generated =
        build_version_matched_catalog(&catalog, &[later_provider, earlier_provider]).unwrap();
    assert_eq!(generated["models"][0]["display_name"], "Alpha/Zulu");
    assert_eq!(generated["models"][0]["priority"], 0);
    assert_eq!(generated["models"][1]["display_name"], "Zulu/Alpha");
    assert_eq!(generated["models"][1]["priority"], 1);
}

#[test]
fn isolated_compact_contract_always_uses_local_client() {
    assert_eq!(
        CompactContract::NameGate.compact_mode_for_launch("managed_app"),
        CompactMode::LocalClient
    );
    assert_eq!(
        CompactContract::NameGate.compact_mode_for_launch("cli"),
        CompactMode::LocalClient
    );
}

#[test]
fn relaunch_refreshes_owned_config_and_keeps_isolated_preferences() {
    let root = std::env::temp_dir().join(format!("3r-config-test-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let catalog = serde_json::json!({"models": [{"slug": "relay-model"}]});
    let gateway = Url::parse("http://127.0.0.1:43123/v1").unwrap();
    let first =
        materialize_managed_codex_config(&root, "first", &gateway, false, &catalog).unwrap();
    materialize_managed_app_config(&root, &first.profile_path).unwrap();
    let mut config: toml::Table = fs::read_to_string(root.join("config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    config
        .get_mut("features")
        .unwrap()
        .as_table_mut()
        .unwrap()
        .insert("goals".to_string(), toml::Value::Boolean(true));
    config.insert(
        "approvals_reviewer".to_string(),
        toml::Value::String("auto_review".to_string()),
    );
    fs::write(root.join("config.toml"), toml::to_string(&config).unwrap()).unwrap();
    let second =
        materialize_managed_codex_config(&root, "second", &gateway, false, &catalog).unwrap();
    materialize_managed_app_config(&root, &second.profile_path).unwrap();
    let config = fs::read_to_string(root.join("config.toml"))
        .unwrap()
        .parse::<toml::Table>()
        .unwrap();
    assert_eq!(config["model_provider"].as_str(), Some("rcodex"));
    assert_eq!(config["model"].as_str(), Some("relay-model"));
    assert_eq!(config["cli_auth_credentials_store"].as_str(), Some("file"));
    assert!(config["model_catalog_json"]
        .as_str()
        .unwrap()
        .ends_with("second.models.json"));
    assert_eq!(config["features"]["goals"].as_bool(), Some(true));
    assert_eq!(config["features"]["multi_agent_v2"].as_bool(), Some(true));
    assert!(config["features"]["multi_agent_v2"].as_table().is_none());
    assert_eq!(config["allow_symlinked_codex_home"].as_bool(), Some(true));
    assert!(config.get("approvals_reviewer").is_none());
    assert_eq!(config["approval_policy"].as_str(), Some("on-request"));
    assert_eq!(
        config["model_providers"]["rcodex"]["name"].as_str(),
        Some(MANAGED_GATEWAY_DISPLAY_NAME)
    );
    assert!(!root.join("config.toml.3r-compat-marker").exists());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn generated_catalog_keeps_schema_fields_but_removes_unverified_capabilities() {
    let template = serde_json::json!({"models":[{
        "slug":"template", "unknown_additive_field":{"value":1},
        "context_window":900000, "max_context_window":900000,
        "auto_compact_token_limit":850000, "input_modalities":["text","image"],
        "supports_image_detail_original":true, "web_search_tool_type":"web_search",
        "experimental_supported_tools":["image_gen"]
    }]});
    let mut selected = route();
    selected.capabilities.input_modalities = vec!["text".to_string()];
    let catalog = build_version_matched_catalog(&template, &[selected.clone()]).unwrap();
    let model = &catalog["models"][0];
    assert_eq!(model["unknown_additive_field"]["value"], 1);
    assert!(model.get("context_window").is_none());
    assert!(model.get("supports_image_detail_original").is_none());
    assert!(model.get("web_search_tool_type").is_none());
    assert_eq!(model["input_modalities"], serde_json::json!(["text"]));
    selected.capabilities.context_window = Some(500000);
    selected.capabilities.auto_compact_token_limit = Some(475000);
    let catalog = build_version_matched_catalog(&template, &[selected]).unwrap();
    assert_eq!(catalog["models"][0]["context_window"], 500000);
    assert_eq!(catalog["models"][0]["max_context_window"], 500000);
    assert_eq!(catalog["models"][0]["auto_compact_token_limit"], 475000);
}

#[test]
fn probe_summary_redacts_credentials_and_urls() {
    let summary = sanitize_probe_summary(
        b"failed to parse https://user:secret@example.test/v1?key=secret Authorization: Bearer secret-value",
    );
    assert!(summary.contains("[url-redacted]"));
    assert!(summary.contains("[redacted]"));
    assert!(!summary.contains("example.test"));
    assert!(!summary.contains("secret"));
}

#[cfg(unix)]
#[test]
fn catalog_probe_logs_a_redacted_nonzero_exit() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!("3r-catalog-probe-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let executable = root.join("codex-fixture");
    fs::write(
        &executable,
        "#!/bin/sh\nprintf 'catalog parse error at https://secret@example.test/v1 Authorization: Bearer secret-value' >&2\nexit 7\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let catalog = root.join("models.json");
    fs::write(&catalog, "{\"models\":[]}").unwrap();
    let log = root.join("startup.log");

    assert_eq!(
        verify_catalog_models_with_codex(
            &executable,
            &catalog,
            &["relay-model".to_string()],
            &log,
        )
        .unwrap_err(),
        "codex_catalog_validation_failed"
    );
    let content = fs::read_to_string(&log).unwrap();
    assert!(content.contains("catalog_probe result=failed stage=command_exit status=exit_7"));
    assert!(content.contains("stderr_sha256=sha256:"));
    assert!(content.contains("[url-redacted]"));
    assert!(content.contains("[redacted]"));
    assert!(!content.contains("example.test"));
    assert!(!content.contains("secret-value"));
    fs::remove_dir_all(root).unwrap();
}
