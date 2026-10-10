use serde::Serialize;
use serde_json::Value;

use crate::routing::is_internal_codex_model_id;

/// A versioned, user-facing description of a Codex special-model role. The
/// role is deliberately separate from the ordinary model catalog: special
/// model ids are implementation details and are never user-selectable.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecialModelRoleSummary {
    pub role_key: String,
    pub display_name: String,
    pub invocation_summary: String,
    pub native_model_id: Option<String>,
    pub native_available: bool,
    pub overrideable: bool,
    pub required_wire_api: Option<String>,
    pub status: String,
}

struct RoleDefinition {
    role_key: &'static str,
    display_name: &'static str,
    invocation_summary: &'static str,
    native_model_ids: &'static [&'static str],
    overrideable: bool,
    required_wire_api: Option<&'static str>,
}

const ROLE_DEFINITIONS: &[RoleDefinition] = &[
    RoleDefinition {
        role_key: "auto_review",
        display_name: "自动审核",
        invocation_summary: "由 Codex 审批审核流程自动调用",
        native_model_ids: &["codex-auto-review", "gpt-code-review"],
        overrideable: true,
        required_wire_api: Some("responses"),
    },
    RoleDefinition {
        role_key: "subagent",
        display_name: "子代理编排",
        invocation_summary: "由 Codex 子代理/协作流程自动调用",
        native_model_ids: &["codex-subagent", "codex-sub-agent", "codex-auto-subagent"],
        // The current R Codex/Codex contract does not expose a stable replacement
        // field for this role. Keep it informational until a version probe
        // proves the override contract.
        overrideable: false,
        required_wire_api: Some("responses"),
    },
];

pub fn inspect_bundled_catalog(catalog: &Value) -> Vec<SpecialModelRoleSummary> {
    let models = catalog
        .get("models")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    ROLE_DEFINITIONS
        .iter()
        .map(|definition| {
            let native = models.iter().find_map(|model| {
                let slug = model.get("slug").and_then(Value::as_str)?;
                let exact = definition
                    .native_model_ids
                    .iter()
                    .any(|candidate| *candidate == slug);
                let candidate_internal = is_internal_codex_model_id(slug)
                    && match definition.role_key {
                        "auto_review" => slug.contains("review"),
                        "subagent" => slug.contains("subagent") || slug.contains("sub-agent"),
                        _ => false,
                    };
                (exact || candidate_internal).then(|| slug.to_string())
            });
            let native_available = native.is_some();
            SpecialModelRoleSummary {
                role_key: definition.role_key.to_string(),
                display_name: definition.display_name.to_string(),
                invocation_summary: definition.invocation_summary.to_string(),
                native_model_id: native,
                native_available,
                overrideable: definition.overrideable,
                required_wire_api: definition.required_wire_api.map(str::to_string),
                status: if native_available {
                    "native_available".to_string()
                } else if definition.overrideable {
                    "replacement_available".to_string()
                } else {
                    "unavailable".to_string()
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn special_models_are_classified_without_becoming_routes() {
        let roles = inspect_bundled_catalog(&json!({
            "models": [
                {"slug": "codex-auto-review", "visibility": "hide"},
                {"slug": "grok-4.6", "visibility": "list"}
            ]
        }));
        let review = roles
            .iter()
            .find(|role| role.role_key == "auto_review")
            .unwrap();
        assert!(review.native_available);
        assert_eq!(review.native_model_id.as_deref(), Some("codex-auto-review"));
        assert!(review.overrideable);
        let subagent = roles
            .iter()
            .find(|role| role.role_key == "subagent")
            .unwrap();
        assert!(!subagent.native_available);
        assert!(!subagent.overrideable);
    }

    #[test]
    fn missing_overrideable_role_is_explicitly_replaceable() {
        let review = inspect_bundled_catalog(&json!({"models": []}))
            .into_iter()
            .find(|role| role.role_key == "auto_review")
            .unwrap();
        assert!(!review.native_available);
        assert_eq!(review.status, "replacement_available");
    }

    #[test]
    fn subagent_role_is_detected_but_remains_informational() {
        let subagent = inspect_bundled_catalog(&json!({
            "models": [{"slug": "codex-subagent", "visibility": "hide"}]
        }))
        .into_iter()
        .find(|role| role.role_key == "subagent")
        .unwrap();
        assert!(subagent.native_available);
        assert!(!subagent.overrideable);
        assert_eq!(subagent.status, "native_available");
    }
}
