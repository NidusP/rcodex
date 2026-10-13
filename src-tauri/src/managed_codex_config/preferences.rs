//! 隔离 home 的偏好播种、同步、onboarding 和权限选择器。
//!
//! 不拷贝凭据、会话、MCP 或浏览数据。显式同步只覆盖用户勾选的安全类别。

use std::{fs, path::Path};

use serde_json::{Map as JsonMap, Value as JsonValue};

use super::io::{read_regular_toml, reject_link, write_private_atomically_overwrite};

const MAX_GUIDANCE_SEED_BYTES: u64 = 16 * 1024 * 1024;

/// A non-coding first-run choice hides every permission except "Ask for
/// approval", and Codex then disables Change permissions. Restore the selector
/// without creating the state file or changing the chosen role.
pub fn keep_managed_permission_selector_available(codex_home: &Path) -> Result<(), String> {
    let path = codex_home.join(".codex-global-state.json");
    if !path.exists() {
        return Ok(());
    }
    reject_link(&path)?;
    let mut value: JsonValue = serde_json::from_slice(
        &fs::read(&path)
            .map_err(|error| format!("managed_permission_selector_read_failed:{error}"))?,
    )
    .map_err(|_| "managed_permission_selector_state_invalid".to_string())?;
    let Some(root) = value.as_object_mut() else {
        return Ok(());
    };
    let state = root
        .entry("electron-persisted-atom-state")
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    let Some(state) = state.as_object_mut() else {
        return Ok(());
    };
    state.insert(
        "composer-permission-mode-visibility".to_string(),
        JsonValue::Bool(true),
    );
    let encoded = serde_json::to_vec_pretty(&value)
        .map_err(|error| format!("managed_permission_selector_encode_failed:{error}"))?;
    write_private_atomically_overwrite(&path, &encoded, "managed_permission_selector")
}

const DESKTOP_REASONING_EFFORT_ALLOWLIST: &[&str] =
    &["low", "medium", "high", "xhigh", "ultra", "persistent"];
const CATALOG_REASONING_EFFORTS_HIDDEN_BY_DEFAULT: &[&str] = &["none", "minimal", "max"];

/// Keep family efforts visible in the Codex desktop model picker.
///
/// The app's default allowlist is low, medium, high, xhigh, ultra, and
/// persistent. Catalog levels such as none and max are dropped from the
/// picker until they are added to enabled-reasoning-efforts. Do not create
/// the state file, and do not remove efforts the user already enabled.
pub fn keep_catalog_reasoning_efforts_selectable(codex_home: &Path) -> Result<(), String> {
    let path = codex_home.join(".codex-global-state.json");
    if !path.exists() {
        return Ok(());
    }
    reject_link(&path)?;
    let mut value: JsonValue = serde_json::from_slice(
        &fs::read(&path)
            .map_err(|error| format!("managed_reasoning_allowlist_read_failed:{error}"))?,
    )
    .map_err(|_| "managed_reasoning_allowlist_state_invalid".to_string())?;
    let Some(root) = value.as_object_mut() else {
        return Ok(());
    };
    let state = root
        .entry("electron-persisted-atom-state")
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    let Some(state) = state.as_object_mut() else {
        return Ok(());
    };
    let mut efforts = state
        .get("enabled-reasoning-efforts")
        .and_then(JsonValue::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            DESKTOP_REASONING_EFFORT_ALLOWLIST
                .iter()
                .map(|effort| (*effort).to_string())
                .collect()
        });
    for effort in CATALOG_REASONING_EFFORTS_HIDDEN_BY_DEFAULT {
        if !efforts.iter().any(|value| value == effort) {
            efforts.push((*effort).to_string());
        }
    }
    state.insert(
        "enabled-reasoning-efforts".to_string(),
        JsonValue::Array(efforts.into_iter().map(JsonValue::String).collect()),
    );
    let encoded = serde_json::to_vec_pretty(&value)
        .map_err(|error| format!("managed_reasoning_allowlist_encode_failed:{error}"))?;
    write_private_atomically_overwrite(&path, &encoded, "managed_reasoning_allowlist")
}

const PREFERENCE_SEED_MARKER: &str = ".rcodex-preference-seed";

/// Copy non-secret app preferences and onboarding state from the user's default
/// Codex home once. Routing, credentials, and conversation history stay out.
pub fn seed_default_codex_preferences(
    managed_home: &Path,
    source_home: &Path,
) -> Result<(), String> {
    let marker = managed_home.join(PREFERENCE_SEED_MARKER);
    if marker.exists() {
        return Ok(());
    }
    if managed_home.canonicalize().ok() == source_home.canonicalize().ok() && source_home.exists() {
        return Ok(());
    }
    if let Some(source_config) = read_regular_toml(&source_home.join("config.toml"))? {
        let config_path = managed_home.join("config.toml");
        reject_link(&config_path)?;
        let mut config = read_regular_toml(&config_path)?.unwrap_or_default();
        merge_codex_preferences(&mut config, &source_config);
        ensure_collaboration_feature(&mut config);
        let encoded = toml::to_string(&config)
            .map_err(|_| "managed_preference_seed_encode_failed".to_string())?;
        write_private_atomically_overwrite(
            &config_path,
            encoded.as_bytes(),
            "managed_preference_seed",
        )?;
    }
    seed_onboarding_state(managed_home, source_home)?;
    seed_user_guidance(managed_home, source_home)?;
    write_private_atomically_overwrite(&marker, b"1\n", "managed_preference_seed_marker")
}

/// Apply an explicit, user-selected subset of the safe default preference
/// seed to an existing managed home. This intentionally does not use the
/// one-time marker: an explicit sync is the user's request to refresh the
/// selected categories. Credentials, sessions, MCP and browser data never
/// participate.
pub fn sync_default_codex_preferences(
    managed_home: &Path,
    source_home: &Path,
    sections: &[String],
) -> Result<Vec<String>, String> {
    let wanted: std::collections::BTreeSet<&str> = sections.iter().map(String::as_str).collect();
    let mut applied = Vec::new();
    if wanted.contains("general") || wanted.contains("plugins") || wanted.contains("permissions") {
        if let Some(source_config) = read_regular_toml(&source_home.join("config.toml"))? {
            let config_path = managed_home.join("config.toml");
            reject_link(&config_path)?;
            let mut config = read_regular_toml(&config_path)?.unwrap_or_default();
            if wanted.contains("general") {
                merge_codex_general_preferences(&mut config, &source_config);
                ensure_collaboration_feature(&mut config);
                applied.push("general".to_string());
            }
            if wanted.contains("plugins") {
                merge_codex_plugins(&mut config, &source_config);
                applied.push("plugins".to_string());
            }
            if wanted.contains("permissions") {
                merge_codex_projects(&mut config, &source_config);
                applied.push("permissions".to_string());
            }
            let encoded = toml::to_string(&config)
                .map_err(|_| "managed_preference_sync_encode_failed".to_string())?;
            write_private_atomically_overwrite(
                &config_path,
                encoded.as_bytes(),
                "managed_preference_sync",
            )?;
        }
    }
    if wanted.contains("appearance") || wanted.contains("onboarding") {
        // The state file contains both categories; the existing whitelist
        // keeps session, device, browser and account state out of the copy.
        seed_onboarding_state(managed_home, source_home)?;
        applied.extend(
            ["appearance", "onboarding"]
                .into_iter()
                .filter(|section| wanted.contains(section))
                .map(str::to_string),
        );
    }
    if wanted.contains("guidance") {
        seed_user_guidance(managed_home, source_home)?;
        applied.push("guidance".to_string());
    }
    Ok(applied)
}

/// Copy user-authored instructions and skills once, while preserving the
/// bundled `.system` skills already installed in the managed home. These are
/// plain guidance files; credentials, sessions, MCP and shell configuration
/// remain outside the seed contract.
fn seed_user_guidance(managed_home: &Path, source_home: &Path) -> Result<(), String> {
    copy_guidance_file(
        &source_home.join("AGENTS.md"),
        &managed_home.join("AGENTS.md"),
    )?;
    copy_guidance_tree(
        &source_home.join("rules"),
        &managed_home.join("rules"),
        false,
    )?;
    copy_guidance_tree(
        &source_home.join("skills"),
        &managed_home.join("skills"),
        true,
    )
}

fn copy_guidance_file(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("managed_guidance_source_stat_failed:{error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("managed_guidance_source_invalid".to_string());
    }
    if metadata.len() > MAX_GUIDANCE_SEED_BYTES {
        return Err("managed_guidance_too_large".to_string());
    }
    reject_link(destination)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("managed_guidance_destination_create_failed:{error}"))?;
    }
    let bytes =
        fs::read(source).map_err(|error| format!("managed_guidance_source_read_failed:{error}"))?;
    write_private_atomically_overwrite(destination, &bytes, "managed_guidance")
}

fn copy_guidance_tree(source: &Path, destination: &Path, skip_system: bool) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("managed_guidance_source_stat_failed:{error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("managed_guidance_source_invalid".to_string());
    }
    fs::create_dir_all(destination)
        .map_err(|error| format!("managed_guidance_destination_create_failed:{error}"))?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("managed_guidance_source_read_failed:{error}"))?
    {
        let entry =
            entry.map_err(|error| format!("managed_guidance_source_read_failed:{error}"))?;
        let name = entry.file_name();
        if skip_system && name == ".system" {
            continue;
        }
        let source_child = entry.path();
        let destination_child = destination.join(&name);
        let child_meta = fs::symlink_metadata(&source_child)
            .map_err(|error| format!("managed_guidance_source_stat_failed:{error}"))?;
        if child_meta.file_type().is_symlink() {
            return Err("managed_guidance_source_link_rejected".to_string());
        }
        if child_meta.is_dir() {
            copy_guidance_tree(&source_child, &destination_child, false)?;
        } else if child_meta.is_file() {
            copy_guidance_file(&source_child, &destination_child)?;
        } else {
            return Err("managed_guidance_source_invalid".to_string());
        }
    }
    Ok(())
}

/// Keep collaboration tools on code-mode models. From Codex 0.160.0 the
/// isolated home must store `features.multi_agent_v2` as the boolean `true`.
/// A nested table, including `non_code_mode_only = false`, removes the
/// `collaboration` namespace. Codex 0.159.0-alpha.12.1 still exposed
/// `spawn_agent` with only `goals = true`. Preference copies must not
/// reintroduce the table.
pub(super) fn ensure_collaboration_feature(config: &mut toml::Table) {
    let mut features = match config.remove("features") {
        Some(toml::Value::Table(table)) => table,
        _ => toml::Table::new(),
    };
    features.insert("multi_agent_v2".to_string(), toml::Value::Boolean(true));
    config.insert("features".to_string(), toml::Value::Table(features));
}

fn merge_codex_preferences(dest: &mut toml::Table, source: &toml::Table) {
    merge_codex_general_preferences(dest, source);
    merge_codex_plugins(dest, source);
    merge_codex_projects(dest, source);
}

fn merge_codex_general_preferences(dest: &mut toml::Table, source: &toml::Table) {
    for key in ["desktop", "features"] {
        if let Some(table) = source.get(key).and_then(toml::Value::as_table) {
            dest.insert(key.to_string(), toml::Value::Table(table.clone()));
        }
    }
    for key in [
        "notify",
        "network_access",
        "disable_response_storage",
        "windows_wsl_setup_acknowledged",
        "personality",
        "hide_agent_reasoning",
    ] {
        if let Some(value) = source.get(key) {
            dest.insert(key.to_string(), value.clone());
        }
    }
}

fn merge_codex_plugins(dest: &mut toml::Table, source: &toml::Table) {
    if let Some(plugins) = source.get("plugins").and_then(toml::Value::as_table) {
        let mut slim = dest
            .get("plugins")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default();
        for (name, value) in plugins {
            let Some(table) = value.as_table() else {
                continue;
            };
            let Some(enabled) = table.get("enabled") else {
                continue;
            };
            let mut plugin = toml::Table::new();
            plugin.insert("enabled".to_string(), enabled.clone());
            slim.insert(name.clone(), toml::Value::Table(plugin));
        }
        if !slim.is_empty() {
            dest.insert("plugins".to_string(), toml::Value::Table(slim));
        }
    }
}

fn merge_codex_projects(dest: &mut toml::Table, source: &toml::Table) {
    if let Some(projects) = source.get("projects").and_then(toml::Value::as_table) {
        let mut slim = dest
            .get("projects")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default();
        for (path, value) in projects {
            let Some(trust) = value.as_table().and_then(|table| table.get("trust_level")) else {
                continue;
            };
            let mut project = toml::Table::new();
            project.insert("trust_level".to_string(), trust.clone());
            slim.insert(path.clone(), toml::Value::Table(project));
        }
        if !slim.is_empty() {
            dest.insert("projects".to_string(), toml::Value::Table(slim));
        }
    }
}

fn inherited_onboarding_atom(key: &str) -> bool {
    key.starts_with("electron:onboarding-")
        || key == "last_completed_onboarding"
        || key == "home-composer-mode-v1"
        || key == "composer-auto-context-enabled"
        || key == "composer-model-picker-menu-view-v1"
        || key == "composer-model-picker-selection-mode-v1"
        || key == "composer-permission-mode-visibility"
        // Stable, non-secret Settings preferences. Deliberately exclude
        // thread/project ids, drafts, browser tabs, workspace roots and
        // device-specific window bounds.
        || matches!(
            key,
            "app-shell-file-tree-open"
                | "browser-sidebar-comment-mode-coachmark-dismissed"
                | "codex-mobile-sidebar-nav-item-clicked-v1"
                | "composer-recent-model-configurations-v1"
                | "diff-filter"
                | "flat-project-sidebar-preferences-v1"
                | "has-accepted-appshot-intro"
                | "has-dismissed-priority-filter-coachmark-v1"
                | "has-dismissed-skills-apps-tooltip"
                | "has-opened-skill-creator-prefill-v1"
                | "has-seen-browser-profile-import-nux-v1"
                | "has-seen-codex-mobile-announcement"
                | "has-user-changed-service-tier"
                | "image-side-panel-auto-expanded-v1"
                | "last-explicit-review-diff-filter"
                | "markdownCodeBlockWordWrap"
                | "sidebar-collapsed-groups"
                | "sidebar-collapsed-sections-v1"
                | "sidebar-width"
                | "skip-full-access-confirm"
        )
        || key.starts_with("permission-selection-by-host-id:")
}

const PROJECTLESS_ONBOARDING_COMPLETED: &str = "electron:onboarding-projectless-completed";

/// Newer Codex shows welcome unless this flag is true, the ChatGPT account
/// already completed desktop onboarding, or the sidebar has a project folder.
/// Isolated homes do not copy those folders. When the default home has already
/// been used and did not write the flag itself, record completion without
/// inventing a non-coding role.
fn should_mark_projectless_onboarding_complete(
    source_home: &Path,
    source_atoms: Option<&JsonMap<String, JsonValue>>,
) -> Result<bool, String> {
    if let Some(atoms) = source_atoms {
        if atoms.contains_key(PROJECTLESS_ONBOARDING_COMPLETED) {
            return Ok(false);
        }
        let welcome_dismissed = atoms
            .get("electron:onboarding-welcome-pending")
            .and_then(JsonValue::as_bool)
            == Some(false);
        let has_home_mode = atoms
            .get("home-composer-mode-v1")
            .and_then(JsonValue::as_str)
            .is_some_and(|mode| !mode.trim().is_empty());
        if welcome_dismissed || has_home_mode {
            return Ok(true);
        }
    }
    source_has_project_trust(source_home)
}

fn source_has_project_trust(source_home: &Path) -> Result<bool, String> {
    let Some(config) = read_regular_toml(&source_home.join("config.toml"))? else {
        return Ok(false);
    };
    Ok(config
        .get("projects")
        .and_then(toml::Value::as_table)
        .is_some_and(|projects| {
            projects.values().any(|value| {
                value
                    .as_table()
                    .and_then(|table| table.get("trust_level"))
                    .is_some()
            })
        }))
}

fn read_source_onboarding_atoms(
    source_home: &Path,
) -> Result<Option<JsonMap<String, JsonValue>>, String> {
    let source_path = source_home.join(".codex-global-state.json");
    if !source_path.exists()
        || source_path
            .symlink_metadata()
            .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Ok(None);
    }
    let source: JsonValue = serde_json::from_slice(
        &fs::read(&source_path)
            .map_err(|error| format!("managed_onboarding_seed_read_failed:{error}"))?,
    )
    .map_err(|_| "managed_onboarding_seed_invalid".to_string())?;
    Ok(source
        .get("electron-persisted-atom-state")
        .and_then(JsonValue::as_object)
        .cloned())
}

fn seed_onboarding_state(managed_home: &Path, source_home: &Path) -> Result<(), String> {
    let source_atoms = read_source_onboarding_atoms(source_home)?;
    let mark_complete =
        should_mark_projectless_onboarding_complete(source_home, source_atoms.as_ref())?;
    if source_atoms.is_none() && !mark_complete {
        return Ok(());
    }
    let dest_path = managed_home.join(".codex-global-state.json");
    reject_link(&dest_path)?;
    let mut dest: JsonValue = if dest_path.exists() {
        serde_json::from_slice(
            &fs::read(&dest_path)
                .map_err(|error| format!("managed_onboarding_seed_dest_read_failed:{error}"))?,
        )
        .map_err(|_| "managed_onboarding_seed_dest_invalid".to_string())?
    } else {
        JsonValue::Object(JsonMap::new())
    };
    let Some(root) = dest.as_object_mut() else {
        return Ok(());
    };
    let atoms = root
        .entry("electron-persisted-atom-state")
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    let Some(atoms) = atoms.as_object_mut() else {
        return Ok(());
    };
    if let Some(source_atoms) = source_atoms.as_ref() {
        atoms.retain(|key, _| {
            !key.starts_with("electron:onboarding-") || source_atoms.contains_key(key)
        });
        for (key, value) in source_atoms {
            if inherited_onboarding_atom(key) {
                atoms.insert(key.clone(), value.clone());
            }
        }
    }
    if mark_complete {
        atoms.insert(
            PROJECTLESS_ONBOARDING_COMPLETED.to_string(),
            JsonValue::Bool(true),
        );
    }
    let encoded = serde_json::to_vec_pretty(&dest)
        .map_err(|error| format!("managed_onboarding_seed_encode_failed:{error}"))?;
    write_private_atomically_overwrite(&dest_path, &encoded, "managed_onboarding_seed")
}
