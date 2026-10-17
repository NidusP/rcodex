//! 版本匹配的模型目录和 compact 合同。
//!
//! 不写 config.toml、不探测真实 Codex 进程。启动准备用它从捆绑目录生成隔离目录。

use std::{collections::BTreeSet, fs, path::Path};

use serde_json::{Map as JsonMap, Value as JsonValue};
use uuid::Uuid;

use crate::model_family_profile::{
    auto_review_override_for_route, overlay_third_party_catalog_entry,
};
use crate::routing::{is_internal_codex_model_id, CompactMode, ModelRoute, WireApi};

use super::{INSTANCE_GATEWAY_ENV, MAX_BUNDLED_CATALOG_BYTES};

/// The isolated provider name deliberately stays outside the official OpenAI
/// identity, so the installed Codex chooses its local compact path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CompactContract {
    #[default]
    NameGate,
}

impl CompactContract {
    pub fn compact_mode_for_launch(self, _launch_target: &str) -> CompactMode {
        let _ = self;
        CompactMode::LocalClient
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodexCatalogCompatibility {
    pub hidden_visibility: &'static str,
    pub supports_hidden_field: bool,
    pub supports_supported_in_api: bool,
    pub supports_auto_review_override: bool,
    pub compact_contract: CompactContract,
}

const VERSION_PROFILES: &[(&str, CodexCatalogCompatibility)] = &[(
    "codex-cli 0.150.",
    CodexCatalogCompatibility {
        hidden_visibility: "hide",
        supports_hidden_field: true,
        supports_supported_in_api: true,
        supports_auto_review_override: true,
        compact_contract: CompactContract::NameGate,
    },
)];

pub fn codex_catalog_compatibility(
    codex_version: &str,
    bundled_catalog: &JsonValue,
) -> CodexCatalogCompatibility {
    if let Some((_, profile)) = VERSION_PROFILES
        .iter()
        .find(|(prefix, _)| codex_version.starts_with(prefix))
    {
        return *profile;
    }
    let template = bundled_catalog
        .get("models")
        .and_then(JsonValue::as_array)
        .and_then(|models| models.iter().find_map(JsonValue::as_object));
    let legacy_default = codex_version.is_empty();
    CodexCatalogCompatibility {
        hidden_visibility: match template
            .and_then(|entry| entry.get("visibility"))
            .and_then(JsonValue::as_str)
        {
            Some("hidden") => "hidden",
            Some("none") => "none",
            Some("hide") => "hide",
            _ => "hide",
        },
        supports_hidden_field: legacy_default
            || template.is_some_and(|entry| entry.contains_key("hidden")),
        supports_supported_in_api: legacy_default
            || template.is_some_and(|entry| entry.contains_key("supported_in_api")),
        supports_auto_review_override: true,
        compact_contract: CompactContract::NameGate,
    }
}

pub fn read_bundled_catalog(codex_executable: &Path) -> Result<JsonValue, String> {
    let probe_home = std::env::temp_dir().join(format!("rcodex-codex-catalog-{}", Uuid::new_v4()));
    fs::create_dir_all(&probe_home)
        .map_err(|error| format!("codex_catalog_probe_home_failed:{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&probe_home, fs::Permissions::from_mode(0o700));
    }
    let result = crate::support::command_without_console(codex_executable)
        .args(["debug", "models", "--bundled"])
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN")
        .env_remove(INSTANCE_GATEWAY_ENV)
        .env("CODEX_HOME", &probe_home)
        .env("CODEX_SQLITE_HOME", &probe_home)
        .output()
        .map_err(|error| format!("codex_catalog_probe_failed:{error}"));
    let _ = fs::remove_dir_all(&probe_home);
    let output = result?;
    if !output.status.success() {
        return Err("codex_catalog_probe_failed".to_string());
    }
    if output.stdout.len() > MAX_BUNDLED_CATALOG_BYTES {
        return Err("codex_catalog_too_large".to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "codex_catalog_invalid".to_string())
}

#[cfg(test)]
pub fn build_version_matched_catalog(
    bundled_catalog: &JsonValue,
    routes: &[ModelRoute],
) -> Result<JsonValue, String> {
    build_version_matched_catalog_with_auto_review(bundled_catalog, routes, None)
}

#[cfg(test)]
pub fn build_version_matched_catalog_with_auto_review(
    bundled_catalog: &JsonValue,
    routes: &[ModelRoute],
    auto_review_public_model_id: Option<&str>,
) -> Result<JsonValue, String> {
    build_version_matched_catalog_for_version(
        bundled_catalog,
        routes,
        "",
        auto_review_public_model_id,
    )
}

pub fn build_version_matched_catalog_for_version(
    bundled_catalog: &JsonValue,
    routes: &[ModelRoute],
    codex_version: &str,
    auto_review_public_model_id: Option<&str>,
) -> Result<JsonValue, String> {
    let compatibility = codex_catalog_compatibility(codex_version, bundled_catalog);
    if routes.is_empty() {
        return Err("route_set_has_no_models".to_string());
    }
    let has_automatic_review_route = routes
        .iter()
        .any(|route| auto_review_override_for_route(route).is_some());
    if (auto_review_public_model_id.is_some() || has_automatic_review_route)
        && !compatibility.supports_auto_review_override
    {
        return Err("auto_review_model_unsupported_by_codex_version".to_string());
    }
    if let Some(public_model_id) = auto_review_public_model_id {
        let review_route = routes
            .iter()
            .find(|route| route.public_model_id == public_model_id)
            .ok_or_else(|| "auto_review_model_not_selected".to_string())?;
        if review_route.wire_api != WireApi::Responses {
            return Err("auto_review_model_requires_responses".to_string());
        }
    }
    let bundled_models = bundled_catalog
        .get("models")
        .and_then(JsonValue::as_array)
        .filter(|models| !models.is_empty())
        .ok_or_else(|| "codex_catalog_has_no_template".to_string())?;
    let fallback = bundled_models
        .first()
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "codex_catalog_template_invalid".to_string())?;

    let mut public_ids = BTreeSet::new();
    let mut ordered_routes = routes.iter().collect::<Vec<_>>();
    ordered_routes.sort_by(|left, right| {
        let left_key = left
            .display_name
            .split_once('/')
            .map(|(provider, model)| (provider.trim(), model.trim()))
            .unwrap_or(("", left.display_name.trim()));
        let right_key = right
            .display_name
            .split_once('/')
            .map(|(provider, model)| (provider.trim(), model.trim()))
            .unwrap_or(("", right.display_name.trim()));
        left_key
            .0
            .to_ascii_lowercase()
            .cmp(&right_key.0.to_ascii_lowercase())
            .then_with(|| {
                left_key
                    .1
                    .to_ascii_lowercase()
                    .cmp(&right_key.1.to_ascii_lowercase())
            })
            .then_with(|| left.public_model_id.cmp(&right.public_model_id))
    });
    let mut models = Vec::with_capacity(routes.len());
    for (catalog_position, route) in ordered_routes.into_iter().enumerate() {
        route.validate().map_err(|error| error.to_string())?;
        if !public_ids.insert(route.public_model_id.as_str()) {
            return Err("duplicate_public_model_id".to_string());
        }
        let template = bundled_models
            .iter()
            .filter_map(JsonValue::as_object)
            .find(|entry| {
                entry.get("slug").and_then(JsonValue::as_str)
                    == Some(route.upstream_model_id.as_str())
            })
            .unwrap_or(fallback);
        models.push(JsonValue::Object(model_entry_from_template(
            template,
            route,
            compatibility,
            catalog_position as u32,
        )?));
    }
    Ok(serde_json::json!({ "models": models }))
}

fn model_entry_from_template(
    template: &JsonMap<String, JsonValue>,
    route: &ModelRoute,
    compatibility: CodexCatalogCompatibility,
    catalog_position: u32,
) -> Result<JsonMap<String, JsonValue>, String> {
    let mut entry = template.clone();
    entry.insert(
        "slug".to_string(),
        JsonValue::String(route.public_model_id.clone()),
    );
    entry.insert(
        "display_name".to_string(),
        JsonValue::String(route.display_name.clone()),
    );
    entry.insert(
        "description".to_string(),
        JsonValue::String(format!("Managed by R Codex via {}.", route.adapter_kind)),
    );
    let hidden_model = is_internal_codex_model_id(&route.upstream_model_id)
        || is_internal_codex_model_id(&route.public_model_id);
    entry.insert(
        "visibility".to_string(),
        JsonValue::String(
            if hidden_model {
                compatibility.hidden_visibility
            } else {
                "list"
            }
            .to_string(),
        ),
    );
    if compatibility.supports_hidden_field {
        entry.insert("hidden".to_string(), JsonValue::Bool(hidden_model));
    }
    if compatibility.supports_supported_in_api {
        entry.insert(
            "supported_in_api".to_string(),
            JsonValue::Bool(!hidden_model),
        );
    }
    entry.insert(
        "priority".to_string(),
        JsonValue::Number(u64::from(catalog_position).into()),
    );
    entry.insert("prefer_websockets".to_string(), JsonValue::Bool(false));
    entry.insert("use_responses_lite".to_string(), JsonValue::Bool(false));
    entry.insert(
        "supports_parallel_tool_calls".to_string(),
        JsonValue::Bool(route.capabilities.supports_tools),
    );
    if !route.capabilities.supports_tools {
        entry.insert(
            "experimental_supported_tools".to_string(),
            JsonValue::Array(Vec::new()),
        );
        entry.insert(
            "include_plugin_usage_instructions".to_string(),
            JsonValue::Bool(false),
        );
        entry.insert(
            "include_skills_usage_instructions".to_string(),
            JsonValue::Bool(false),
        );
    }
    if let Some(context_window) = route.capabilities.context_window {
        entry.insert(
            "context_window".to_string(),
            JsonValue::Number(context_window.into()),
        );
        entry.insert(
            "max_context_window".to_string(),
            JsonValue::Number(context_window.into()),
        );
        if let Some(limit) = route.capabilities.auto_compact_token_limit {
            entry.insert(
                "auto_compact_token_limit".to_string(),
                JsonValue::Number(limit.into()),
            );
        } else {
            entry.remove("auto_compact_token_limit");
        }
    } else {
        entry.remove("context_window");
        entry.remove("max_context_window");
        entry.remove("auto_compact_token_limit");
    }
    if !route.capabilities.input_modalities.is_empty() {
        entry.insert(
            "input_modalities".to_string(),
            serde_json::to_value(&route.capabilities.input_modalities)
                .map_err(|error| format!("catalog_capabilities_encode_failed:{error}"))?,
        );
    }
    entry.insert(
        "supports_search_tool".to_string(),
        JsonValue::Bool(route.capabilities.supports_web_search),
    );
    overlay_third_party_catalog_entry(
        &mut entry,
        route,
        compatibility.supports_auto_review_override,
    );
    Ok(entry)
}
