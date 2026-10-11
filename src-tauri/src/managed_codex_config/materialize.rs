//! 隔离 profile / catalog / App config.toml 物化。
//!
//! 不拷贝 Computer Use helper、不播种用户偏好。启动准备在固化 RouteSet 后调用。

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value as JsonValue;
use url::Url;

use crate::routing::validate_public_model_id;

use super::io::{
    reject_link, sha256_label, validate_gateway_url, validate_profile_name,
    write_immutable_atomically, write_private_atomically_overwrite,
};
use super::preferences::ensure_collaboration_feature;
use super::{
    INSTANCE_GATEWAY_ENV, INSTANCE_GATEWAY_HEADER, MANAGED_DIRECT_MODEL_PROVIDER_ID,
    MANAGED_GATEWAY_DISPLAY_NAME, MANAGED_MODEL_PROVIDER_ID,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedCodexConfig {
    pub profile_name: String,
    pub profile_path: PathBuf,
    pub catalog_path: PathBuf,
    pub profile_sha256: String,
    pub catalog_sha256: String,
}

#[cfg(test)]
pub fn materialize_managed_codex_config(
    codex_home: &Path,
    profile_name: &str,
    gateway_base_url: &Url,
    requires_openai_auth: bool,
    catalog: &JsonValue,
) -> Result<MaterializedCodexConfig, String> {
    let default_model_id = first_catalog_model_id(catalog)?;
    materialize_managed_codex_config_with_auto_review(
        codex_home,
        profile_name,
        default_model_id,
        gateway_base_url,
        requires_openai_auth,
        catalog,
    )
}

pub fn materialize_managed_codex_config_with_auto_review(
    codex_home: &Path,
    profile_name: &str,
    default_model_id: &str,
    gateway_base_url: &Url,
    requires_openai_auth: bool,
    catalog: &JsonValue,
) -> Result<MaterializedCodexConfig, String> {
    validate_profile_name(profile_name)?;
    validate_gateway_url(gateway_base_url)?;
    let codex_home = codex_home
        .canonicalize()
        .map_err(|error| format!("managed_codex_home_invalid:{error}"))?;
    if !codex_home.is_dir() {
        return Err("managed_codex_home_not_directory".to_string());
    }
    materialize_profile_files(
        &codex_home,
        profile_name,
        catalog,
        default_model_id,
        render_profile(
            gateway_base_url,
            requires_openai_auth,
            &codex_home.join(format!("{profile_name}.models.json")),
            default_model_id,
        )?,
    )
}

pub fn materialize_managed_app_config(
    codex_home: &Path,
    profile_path: &Path,
) -> Result<(), String> {
    let codex_home = codex_home
        .canonicalize()
        .map_err(|error| format!("managed_codex_home_invalid:{error}"))?;
    let profile_path = profile_path
        .canonicalize()
        .map_err(|error| format!("managed_app_profile_invalid:{error}"))?;
    if profile_path.parent() != Some(codex_home.as_path()) {
        return Err("managed_app_profile_outside_home".to_string());
    }
    let config_path = codex_home.join("config.toml");
    let profile_bytes = fs::read(&profile_path)
        .map_err(|error| format!("managed_app_profile_read_failed:{error}"))?;
    reject_link(&config_path)?;
    let mut config = if config_path.exists() {
        fs::read_to_string(&config_path)
            .map_err(|_| "managed_app_base_config_read_failed".to_string())?
            .parse::<toml::Table>()
            .map_err(|_| "managed_app_base_config_invalid".to_string())?
    } else {
        toml::Table::new()
    };
    let profile = std::str::from_utf8(&profile_bytes)
        .map_err(|_| "managed_app_profile_invalid".to_string())?
        .parse::<toml::Table>()
        .map_err(|_| "managed_app_profile_invalid".to_string())?;
    // Keep preferences created inside R Codex while refreshing its launch snapshot.
    // The immutable profile belongs to this same isolated home.
    config.extend(profile);
    // A missing policy is not the same as asking. Codex 0.162's TUI starts
    // that case as granular with sandbox_approval disabled, so .git and
    // network escalations are rejected without a prompt. extend() also keeps
    // a previously written approvals_reviewer.
    require_user_approval(&mut config);
    ensure_collaboration_feature(&mut config);
    let encoded =
        toml::to_string(&config).map_err(|_| "managed_app_config_encode_failed".to_string())?;
    write_private_atomically_overwrite(&config_path, encoded.as_bytes(), "managed_app_config")
}

/// Codex disables its own sandbox when CODEX_HOME is reached through the short
/// macOS alias. The alias is intentional; tell Codex it is trusted.
fn allow_short_codex_home_symlink(root: &mut toml::Table) {
    root.insert(
        "allow_symlinked_codex_home".to_string(),
        toml::Value::Boolean(true),
    );
}

fn materialize_profile_files(
    codex_home: &Path,
    profile_name: &str,
    catalog: &JsonValue,
    default_model_id: &str,
    profile: String,
) -> Result<MaterializedCodexConfig, String> {
    validate_catalog(catalog)?;
    validate_catalog_default_model(catalog, default_model_id)?;
    let catalog_bytes = serde_json::to_vec_pretty(catalog)
        .map_err(|error| format!("catalog_encode_failed:{error}"))?;
    let profile_bytes = profile.into_bytes();
    let catalog_path = codex_home.join(format!("{profile_name}.models.json"));
    let profile_path = codex_home.join(format!("{profile_name}.config.toml"));
    let catalog_existed = catalog_path.exists();
    write_immutable_atomically(&catalog_path, &catalog_bytes)?;
    if let Err(error) = write_immutable_atomically(&profile_path, &profile_bytes) {
        if !catalog_existed && !profile_path.exists() {
            let _ = fs::remove_file(&catalog_path);
        }
        return Err(error);
    }
    Ok(MaterializedCodexConfig {
        profile_name: profile_name.to_string(),
        profile_path,
        catalog_path,
        profile_sha256: sha256_label(&profile_bytes),
        catalog_sha256: sha256_label(&catalog_bytes),
    })
}

fn render_profile(
    gateway_base_url: &Url,
    requires_openai_auth: bool,
    catalog_path: &Path,
    default_model_id: &str,
) -> Result<String, String> {
    let mut root = toml::Table::new();
    root.insert(
        "cli_auth_credentials_store".to_string(),
        toml::Value::String("file".to_string()),
    );
    root.insert(
        "model_provider".to_string(),
        toml::Value::String(MANAGED_MODEL_PROVIDER_ID.to_string()),
    );
    root.insert(
        "model_catalog_json".to_string(),
        toml::Value::String(catalog_path.display().to_string()),
    );
    root.insert(
        "model".to_string(),
        toml::Value::String(default_model_id.to_string()),
    );
    root.insert(
        "check_for_update_on_startup".to_string(),
        toml::Value::Boolean(false),
    );
    allow_short_codex_home_symlink(&mut root);
    let mut headers = toml::Table::new();
    headers.insert(
        INSTANCE_GATEWAY_HEADER.to_string(),
        toml::Value::String(INSTANCE_GATEWAY_ENV.to_string()),
    );
    let mut provider = toml::Table::new();
    provider.insert(
        "name".to_string(),
        toml::Value::String(MANAGED_GATEWAY_DISPLAY_NAME.to_string()),
    );
    provider.insert(
        "base_url".to_string(),
        toml::Value::String(gateway_base_url.as_str().trim_end_matches('/').to_string()),
    );
    provider.insert(
        "wire_api".to_string(),
        toml::Value::String("responses".to_string()),
    );
    provider.insert(
        "requires_openai_auth".to_string(),
        toml::Value::Boolean(requires_openai_auth),
    );
    provider.insert("env_http_headers".to_string(), toml::Value::Table(headers));
    let mut providers = toml::Table::new();
    providers.insert(
        MANAGED_MODEL_PROVIDER_ID.to_string(),
        toml::Value::Table(provider),
    );
    root.insert("model_providers".to_string(), toml::Value::Table(providers));
    require_user_approval(&mut root);
    toml::to_string(&root).map_err(|error| format!("managed_profile_encode_failed:{error}"))
}

fn require_user_approval(config: &mut toml::Table) {
    config.insert(
        "approval_policy".to_string(),
        toml::Value::String("on-request".to_string()),
    );
    config.remove("approvals_reviewer");
}

fn validate_catalog(catalog: &JsonValue) -> Result<(), String> {
    let models = catalog
        .get("models")
        .and_then(JsonValue::as_array)
        .filter(|models| !models.is_empty())
        .ok_or_else(|| "managed_catalog_has_no_models".to_string())?;
    let mut slugs = BTreeSet::new();
    for model in models {
        let slug = model
            .get("slug")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| "managed_catalog_model_missing_slug".to_string())?;
        validate_public_model_id(slug).map_err(|error| error.to_string())?;
        if !slugs.insert(slug) {
            return Err("managed_catalog_duplicate_slug".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
fn first_catalog_model_id(catalog: &JsonValue) -> Result<&str, String> {
    catalog
        .get("models")
        .and_then(JsonValue::as_array)
        .and_then(|models| models.first())
        .and_then(JsonValue::as_object)
        .and_then(|model| model.get("slug"))
        .and_then(JsonValue::as_str)
        .filter(|slug| !slug.trim().is_empty())
        .ok_or_else(|| "managed_catalog_default_model_missing".to_string())
}

fn validate_catalog_default_model(
    catalog: &JsonValue,
    default_model_id: &str,
) -> Result<(), String> {
    validate_public_model_id(default_model_id).map_err(|error| error.to_string())?;
    let selected = catalog
        .get("models")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_object)
        .any(|model| model.get("slug").and_then(JsonValue::as_str) == Some(default_model_id));
    if !selected {
        return Err("managed_catalog_default_model_missing".to_string());
    }
    Ok(())
}
