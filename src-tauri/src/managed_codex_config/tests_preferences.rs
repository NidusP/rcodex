//! 偏好播种、onboarding 和权限选择器。

use std::fs;

use serde_json::Value as JsonValue;
use uuid::Uuid;

use super::*;

#[test]
fn seed_copies_preferences_and_onboarding_once_without_routing_or_history() {
    let root = std::env::temp_dir().join(format!("3r-preference-seed-{}", Uuid::new_v4()));
    let source = root.join("source");
    let managed = root.join("managed");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&managed).unwrap();
    fs::create_dir_all(source.join("rules")).unwrap();
    fs::create_dir_all(source.join("skills/personal")).unwrap();
    fs::create_dir_all(managed.join("skills/.system")).unwrap();
    fs::write(source.join("AGENTS.md"), "global guidance").unwrap();
    fs::write(source.join("rules/default.rules"), "allow pattern").unwrap();
    fs::write(source.join("skills/personal/SKILL.md"), "personal skill").unwrap();
    fs::write(
        managed.join("skills/.system/SKILL.md"),
        "bundled system skill",
    )
    .unwrap();
    fs::write(
        source.join("config.toml"),
        r#"model_provider = "OpenAI"
notify = ["bell"]

[desktop]
localeOverride = "en-US"

[mcp_servers.docs]
command = "secret-tool"

[plugins."visualize@openai-bundled"]
enabled = true
extra = "skip"

[projects."/work/app"]
trust_level = "trusted"
"#,
    )
    .unwrap();
    fs::write(
        managed.join("config.toml"),
        "model_provider = \"rcodex\"\nmodel = \"relay-model\"\n",
    )
    .unwrap();
    fs::write(source.join(".codex-global-state.json"), r#"{"thread-project-assignments":{"thread":"secret"},"electron-persisted-atom-state":{"electron:onboarding-welcome-pending":false,"home-composer-mode-v1":"codex","composer-model-picker-menu-view-v1":"advanced","sidebar-width":280,"composer-prompt-drafts-v1":{"draft":"secret"}}}"#).unwrap();
    fs::write(managed.join(".codex-global-state.json"), r#"{"selected-project":"kept","electron-persisted-atom-state":{"electron:onboarding-welcome-v2-role-state":{"workMode":"non_coding"}}}"#).unwrap();
    seed_default_codex_preferences(&managed, &source).unwrap();
    let config: toml::Table = fs::read_to_string(managed.join("config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(config["model_provider"].as_str(), Some("rcodex"));
    assert_eq!(config["model"].as_str(), Some("relay-model"));
    assert!(config.get("mcp_servers").is_none());
    assert_eq!(config["desktop"]["localeOverride"].as_str(), Some("en-US"));
    assert_eq!(
        config["plugins"]["visualize@openai-bundled"]["enabled"].as_bool(),
        Some(true)
    );
    assert!(config["plugins"]["visualize@openai-bundled"]
        .get("extra")
        .is_none());
    assert_eq!(
        config["projects"]["/work/app"]["trust_level"].as_str(),
        Some("trusted")
    );
    assert_eq!(
        fs::read_to_string(managed.join("AGENTS.md")).unwrap(),
        "global guidance"
    );
    assert_eq!(
        fs::read_to_string(managed.join("rules/default.rules")).unwrap(),
        "allow pattern"
    );
    assert_eq!(
        fs::read_to_string(managed.join("skills/personal/SKILL.md")).unwrap(),
        "personal skill"
    );
    assert_eq!(
        fs::read_to_string(managed.join("skills/.system/SKILL.md")).unwrap(),
        "bundled system skill"
    );
    let state: JsonValue =
        serde_json::from_slice(&fs::read(managed.join(".codex-global-state.json")).unwrap())
            .unwrap();
    assert_eq!(state["selected-project"], "kept");
    assert!(state.get("thread-project-assignments").is_none());
    let atoms = &state["electron-persisted-atom-state"];
    assert_eq!(atoms["electron:onboarding-welcome-pending"], false);
    assert_eq!(atoms["home-composer-mode-v1"], "codex");
    assert_eq!(atoms["composer-model-picker-menu-view-v1"], "advanced");
    assert_eq!(atoms["sidebar-width"], 280);
    assert_eq!(atoms["electron:onboarding-projectless-completed"], true);
    assert!(atoms
        .get("electron:onboarding-welcome-v2-role-state")
        .is_none());
    assert!(atoms.get("composer-prompt-drafts-v1").is_none());
    fs::write(managed.join("config.toml"), "model_provider = \"rcodex\"\n").unwrap();
    seed_default_codex_preferences(&managed, &source).unwrap();
    let again = fs::read_to_string(managed.join("config.toml")).unwrap();
    assert!(!again.contains("localeOverride"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_sync_only_applies_selected_safe_categories() {
    let root = std::env::temp_dir().join(format!("3r-preference-sync-{}", Uuid::new_v4()));
    let source = root.join("source");
    let managed = root.join("managed");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&managed).unwrap();
    fs::write(
        source.join("config.toml"),
        "notify = [\"bell\"]\n[plugins.demo]\nenabled = true\n",
    )
    .unwrap();
    fs::write(managed.join("config.toml"), "model_provider = \"rcodex\"\n").unwrap();
    let applied =
        sync_default_codex_preferences(&managed, &source, &["plugins".to_string()]).unwrap();
    let config: toml::Table = fs::read_to_string(managed.join("config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(applied, vec!["plugins"]);
    assert!(config.get("notify").is_none());
    assert_eq!(config["plugins"]["demo"]["enabled"].as_bool(), Some(true));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn onboarding_sync_marks_used_default_home_without_role_or_project_roots() {
    let root = std::env::temp_dir().join(format!("3r-projectless-onboarding-{}", Uuid::new_v4()));
    let source = root.join("source");
    let managed = root.join("managed");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&managed).unwrap();
    fs::write(
        source.join("config.toml"),
        "[projects.\"/work/app\"]\ntrust_level = \"trusted\"\n",
    )
    .unwrap();
    fs::write(
        source.join(".codex-global-state.json"),
        r#"{"electron-persisted-atom-state":{"home-composer-mode-v1":"work","sidebar-project-expanded-v1-codex:/work/app":true}}"#,
    )
    .unwrap();
    fs::write(
        managed.join(".codex-global-state.json"),
        r#"{"electron-persisted-atom-state":{"electron:onboarding-welcome-v2-role-state":{"roles":["something_else"],"workMode":"non_coding"}}}"#,
    )
    .unwrap();
    let applied =
        sync_default_codex_preferences(&managed, &source, &["onboarding".to_string()]).unwrap();
    assert_eq!(applied, vec!["onboarding"]);
    let state: JsonValue =
        serde_json::from_slice(&fs::read(managed.join(".codex-global-state.json")).unwrap())
            .unwrap();
    let atoms = &state["electron-persisted-atom-state"];
    assert_eq!(atoms["electron:onboarding-projectless-completed"], true);
    assert_eq!(atoms["home-composer-mode-v1"], "work");
    assert!(atoms
        .get("electron:onboarding-welcome-v2-role-state")
        .is_none());
    assert!(atoms
        .get("sidebar-project-expanded-v1-codex:/work/app")
        .is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_trust_alone_marks_projectless_onboarding_complete() {
    let root = std::env::temp_dir().join(format!("3r-project-trust-onboarding-{}", Uuid::new_v4()));
    let source = root.join("source");
    let managed = root.join("managed");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&managed).unwrap();
    fs::write(
        source.join("config.toml"),
        "[projects.\"/work/app\"]\ntrust_level = \"trusted\"\n",
    )
    .unwrap();
    seed_default_codex_preferences(&managed, &source).unwrap();
    let state: JsonValue =
        serde_json::from_slice(&fs::read(managed.join(".codex-global-state.json")).unwrap())
            .unwrap();
    let atoms = &state["electron-persisted-atom-state"];
    assert_eq!(atoms["electron:onboarding-projectless-completed"], true);
    assert!(atoms
        .get("electron:onboarding-welcome-v2-role-state")
        .is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_projectless_flag_and_unused_home_are_not_rewritten() {
    let root = std::env::temp_dir().join(format!("3r-projectless-explicit-{}", Uuid::new_v4()));
    let explicit_source = root.join("explicit");
    let explicit_managed = root.join("explicit-managed");
    fs::create_dir_all(&explicit_source).unwrap();
    fs::create_dir_all(&explicit_managed).unwrap();
    fs::write(
        explicit_source.join(".codex-global-state.json"),
        r#"{"electron-persisted-atom-state":{"electron:onboarding-welcome-pending":false,"electron:onboarding-projectless-completed":false,"home-composer-mode-v1":"work"}}"#,
    )
    .unwrap();
    seed_default_codex_preferences(&explicit_managed, &explicit_source).unwrap();
    let explicit_state: JsonValue = serde_json::from_slice(
        &fs::read(explicit_managed.join(".codex-global-state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        explicit_state["electron-persisted-atom-state"]
            ["electron:onboarding-projectless-completed"],
        false
    );

    let unused_source = root.join("unused");
    let unused_managed = root.join("unused-managed");
    fs::create_dir_all(&unused_source).unwrap();
    fs::create_dir_all(&unused_managed).unwrap();
    fs::write(
        unused_source.join(".codex-global-state.json"),
        r#"{"electron-persisted-atom-state":{"sidebar-width":240}}"#,
    )
    .unwrap();
    seed_default_codex_preferences(&unused_managed, &unused_source).unwrap();
    let unused_state: JsonValue =
        serde_json::from_slice(&fs::read(unused_managed.join(".codex-global-state.json")).unwrap())
            .unwrap();
    assert!(unused_state["electron-persisted-atom-state"]
        .get("electron:onboarding-projectless-completed")
        .is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn managed_home_keeps_permission_selector_visible_without_creating_state() {
    let root = std::env::temp_dir().join(format!("3r-permission-selector-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    keep_managed_permission_selector_available(&root).unwrap();
    assert!(!root.join(".codex-global-state.json").exists());

    let state = serde_json::json!({
        "selected-project": "kept",
        "electron-persisted-atom-state": {
            "composer-permission-mode-visibility": false,
            "home-composer-mode-v1": "work"
        }
    });
    fs::write(
        root.join(".codex-global-state.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
    keep_managed_permission_selector_available(&root).unwrap();
    let saved: JsonValue =
        serde_json::from_slice(&fs::read(root.join(".codex-global-state.json")).unwrap()).unwrap();
    assert_eq!(saved["selected-project"], "kept");
    assert_eq!(
        saved["electron-persisted-atom-state"]["home-composer-mode-v1"],
        "work"
    );
    assert_eq!(
        saved["electron-persisted-atom-state"]["composer-permission-mode-visibility"],
        true
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn managed_home_adds_hidden_catalog_efforts_without_creating_state() {
    let root = std::env::temp_dir().join(format!("3r-reasoning-allowlist-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    keep_catalog_reasoning_efforts_selectable(&root).unwrap();
    assert!(!root.join(".codex-global-state.json").exists());

    let state = serde_json::json!({
        "selected-project": "kept",
        "electron-persisted-atom-state": {
            "enabled-reasoning-efforts": ["low", "high"],
            "home-composer-mode-v1": "work"
        }
    });
    fs::write(
        root.join(".codex-global-state.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
    keep_catalog_reasoning_efforts_selectable(&root).unwrap();
    let saved: JsonValue =
        serde_json::from_slice(&fs::read(root.join(".codex-global-state.json")).unwrap()).unwrap();
    assert_eq!(saved["selected-project"], "kept");
    assert_eq!(
        saved["electron-persisted-atom-state"]["home-composer-mode-v1"],
        "work"
    );
    assert_eq!(
        saved["electron-persisted-atom-state"]["enabled-reasoning-efforts"],
        serde_json::json!(["low", "high", "none", "minimal", "max"])
    );

    let missing = serde_json::json!({"electron-persisted-atom-state": {}});
    fs::write(
        root.join(".codex-global-state.json"),
        serde_json::to_vec_pretty(&missing).unwrap(),
    )
    .unwrap();
    keep_catalog_reasoning_efforts_selectable(&root).unwrap();
    let filled: JsonValue =
        serde_json::from_slice(&fs::read(root.join(".codex-global-state.json")).unwrap()).unwrap();
    assert_eq!(
        filled["electron-persisted-atom-state"]["enabled-reasoning-efforts"],
        serde_json::json!([
            "low",
            "medium",
            "high",
            "xhigh",
            "ultra",
            "persistent",
            "none",
            "minimal",
            "max"
        ])
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn launch_replaces_collaboration_table_with_boolean() {
    let root = std::env::temp_dir().join(format!("3r-collab-feature-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("config.toml"),
        "[features]\ngoals = true\n\n[features.multi_agent_v2]\nnon_code_mode_only = false\n",
    )
    .unwrap();
    let profile = root.join("profile.toml");
    fs::write(&profile, "model_provider = \"rcodex\"\n").unwrap();
    materialize_managed_app_config(&root, &profile).unwrap();
    let config: toml::Table = fs::read_to_string(root.join("config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(config["features"]["goals"].as_bool(), Some(true));
    assert_eq!(config["features"]["multi_agent_v2"].as_bool(), Some(true));
    assert!(config["features"]["multi_agent_v2"].as_table().is_none());
    assert!(!fs::read_to_string(root.join("config.toml"))
        .unwrap()
        .contains("non_code_mode_only"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn isolated_config_rejects_link_to_user_file_without_reading_it() {
    let root = std::env::temp_dir().join(format!("3r-config-link-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let source = root.join("user-config");
    fs::write(&source, "sentinel").unwrap();
    std::os::unix::fs::symlink(&source, root.join("config.toml")).unwrap();
    let profile = root.join("profile.toml");
    fs::write(&profile, "model_provider='rcodex'").unwrap();
    assert_eq!(
        materialize_managed_app_config(&root, &profile).unwrap_err(),
        "managed_file_symlink_rejected"
    );
    assert_eq!(fs::read_to_string(source).unwrap(), "sentinel");
    fs::remove_dir_all(root).unwrap();
}
