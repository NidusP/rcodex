//! Family profiles for third-party Codex catalog entries.
//!
//! Codex requires a version-matched catalog JSON. R Codex therefore clones a bundled
//! GPT skeleton so unknown fields stay valid for the installed app. That clone
//! is structural only: a Grok route must not keep GPT-6's identity, reasoning
//! ladder, or Fast/priority tiers, or the model will try to drive Codex as if it
//! were GPT-6.
//!
//! One generator (`model_entry_from_template`) plus these profiles. Adding a
//! family is data, not a second catalog compiler.

use serde_json::{Map as JsonMap, Value as JsonValue};

use crate::routing::{
    is_ephemeral_family_alias, is_internal_codex_model_id, ModelRoute, RouteSource, WireApi,
};

/// xAI published context window for the Grok 4.x family this round.
pub const GROK_CONTEXT_WINDOW: u64 = 500_000;
/// Default compact threshold: 95% of [`GROK_CONTEXT_WINDOW`].
pub const GROK_AUTO_COMPACT_TOKEN_LIMIT: u64 = 475_000;
/// Inclusive client maximum. Official 1M windows may be stored; larger numbers stay rejected.
pub const MAX_CLIENT_CONTEXT_WINDOW: u64 = 1_000_000;
/// Published 1,000,000-token context used by current DeepSeek, GLM 5.2/5.3, and Qwen 3.7/3.8 cards.
pub const OFFICIAL_MILLION_CONTEXT_WINDOW: u64 = 1_000_000;
/// 95% of the official 1M window, matching the family compact rule.
pub const OFFICIAL_MILLION_AUTO_COMPACT_TOKEN_LIMIT: u64 = 950_000;

/// Semantic overlay applied on top of a cloned Codex catalog skeleton.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelFamilyProfile {
    /// Short family name used in the identity sentence (Grok, deepseek, …).
    pub family_name: String,
    /// Reasoning efforts this family may advertise, in UI order.
    pub reasoning_levels: Vec<String>,
    /// Default effort; must be one of `reasoning_levels` when present.
    pub default_reasoning_level: Option<String>,
    /// Drop OpenAI Fast/priority catalog fields that are not this family's.
    pub strip_openai_speed_tiers: bool,
    /// Drop the cloned model's forced sub-agent effort until it is verified.
    pub strip_unverified_multi_agent_effort: bool,
    /// Prevent a cloned GPT row from enabling the ChatGPT-specific Node REPL.
    pub disable_openai_node_repl: bool,
    /// Official recommended client context window. `None` means do not invent a number.
    pub context_window: Option<u64>,
    /// Official recommended auto-compact threshold. `None` means `context_window × 95%`.
    pub auto_compact_token_limit: Option<u64>,
    /// Responses image-input for this family. Grok 4.x is true after a 2026-09-21
    /// 3rxai probe using the standard Responses `input_image.image_url` data URL.
    /// Chat Completions uses the same flag and converts input_image to image_url.
    pub supports_images: bool,
}

impl ModelFamilyProfile {
    /// Product identity: the model is itself, using Codex as a workspace.
    pub fn identity_block(&self) -> String {
        let family = &self.family_name;
        format!(
            "你是 {family}，在 Codex 工作区里用本地工具。\n\
You are {family}, working in a Codex workspace with local tools.\n\
Use only the local workspace tools provided in this session."
        )
    }
}

/// Resolve the family from the upstream model id, never the R Codex public slug.
///
/// Public ids are stable routing keys and may look like hashed prefixes; the
/// upstream id is what the provider actually serves (`grok-4.6`, `deepseek-chat`).
pub fn resolve_model_family(upstream_model_id: &str) -> ModelFamilyProfile {
    let normalized = upstream_model_id.trim().to_ascii_lowercase();
    if normalized.contains("grok") {
        return grok_profile(&normalized);
    }
    if normalized.contains("deepseek") {
        return deepseek_profile(&normalized);
    }
    if normalized.contains("glm") {
        return glm_profile(&normalized);
    }
    if normalized.contains("qwen") {
        return qwen_profile(&normalized);
    }
    if normalized.contains("claude") || normalized.contains("anthropic") {
        return fallback_reasoning_profile("Claude");
    }
    if normalized.starts_with("gpt-") || normalized.contains("openai") {
        return named_profile(
            "GPT",
            vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
                "max".to_string(),
                "ultra".to_string(),
            ],
            Some("high".to_string()),
        );
    }
    fallback_reasoning_profile(&unknown_family_label(upstream_model_id))
}

fn grok_profile(normalized: &str) -> ModelFamilyProfile {
    // Ladders follow xAI's published Grok reasoning contract, not GPT's.
    // Context windows are the same 500k official figure for every Grok version
    // this round; add a per-version number here when a distinct official
    // window is published.
    let mut profile = if normalized.contains("4.6") || normalized.contains("4-6") {
        named_profile(
            "Grok",
            vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
            ],
            Some("high".to_string()),
        )
    } else if normalized.contains("4.3") || normalized.contains("4-3") {
        named_profile(
            "Grok",
            vec![
                "none".to_string(),
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
            ],
            Some("high".to_string()),
        )
    } else {
        named_profile(
            "Grok",
            vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
            ],
            Some("high".to_string()),
        )
    };
    profile.context_window = Some(GROK_CONTEXT_WINDOW);
    profile.auto_compact_token_limit = Some(GROK_AUTO_COMPACT_TOKEN_LIMIT);
    // Grok 4.x understands images on official xAI and on 3rxai Responses when
    // the body is the Codex/OpenAI `input_image` + string `image_url` form.
    profile.supports_images = grok_version_is_4x(normalized);
    profile
}

fn deepseek_profile(normalized: &str) -> ModelFamilyProfile {
    // DeepSeek API docs, retrieved 2026-10-13:
    // https://api-docs.deepseek.com/guides/thinking_mode
    // https://api-docs.deepseek.com/quick_start/pricing
    // Responses reasoning.effort accepts none, low, high, and max. Default high.
    // minimal maps to low, medium and xhigh map to high, and ultra maps to max.
    // Those are aliases, not extra catalog levels.
    // The pricing table gives both current models a 1M context. Vision is listed
    // for deepseek-flash only. Legacy flash names are still served by Flash.
    // Retired ids such as deepseek-chat are not on that card, so they keep the
    // effort ladder and do not inherit the 1M window.
    let mut profile = named_profile(
        "DeepSeek",
        vec![
            "none".to_string(),
            "low".to_string(),
            "high".to_string(),
            "max".to_string(),
        ],
        Some("high".to_string()),
    );
    if deepseek_publishes_million_context(normalized) {
        profile = with_official_window(profile, OFFICIAL_MILLION_CONTEXT_WINDOW);
    }
    profile.supports_images = deepseek_publishes_vision(normalized);
    profile
}

fn deepseek_publishes_million_context(normalized: &str) -> bool {
    normalized.contains("flash") || normalized.contains("v4-pro") || normalized.contains("v4.1")
}

fn deepseek_publishes_vision(normalized: &str) -> bool {
    !normalized.contains("v4-pro")
        && (normalized.contains("flash") || normalized.contains("vision"))
}

fn glm_profile(normalized: &str) -> ModelFamilyProfile {
    // Z.AI docs, retrieved 2026-10-13:
    // https://docs.z.ai/guides/llm/glm-5.3.md
    // https://docs.z.ai/guides/llm/glm-5.2.md
    // https://docs.z.ai/guides/vlm/glm-5.3-flash.md
    // https://docs.z.ai/guides/overview/concept-param.md
    // GLM-5.3 and GLM-5.3-Flash only accept low, high, and max, default max.
    // Thinking cannot be turned off. GLM-5.3 is text; Flash accepts images.
    // Both publish a 1M context. GLM-5.2 also publishes 1M and is text-only.
    // Its extra names collapse: none/minimal skip thinking, low/medium become
    // high, and xhigh becomes max. The catalog keeps only the distinct names.
    // reasoning_effort is documented from GLM-5.2 upward, so older GLM ids do
    // not get a ladder or an invented window.
    let five_three = normalized.contains("5.3") || normalized.contains("5-3");
    let five_two = normalized.contains("5.2") || normalized.contains("5-2");
    let flash = normalized.contains("flash");
    let mut profile = if five_three {
        named_profile(
            "GLM",
            vec!["low".to_string(), "high".to_string(), "max".to_string()],
            Some("max".to_string()),
        )
    } else if five_two {
        named_profile(
            "GLM",
            vec!["none".to_string(), "high".to_string(), "max".to_string()],
            Some("max".to_string()),
        )
    } else {
        named_profile("GLM", Vec::new(), None)
    };
    if five_three || five_two {
        profile = with_official_window(profile, OFFICIAL_MILLION_CONTEXT_WINDOW);
    }
    profile.supports_images = (five_three && flash)
        || normalized.contains("4.6v")
        || normalized.contains("4.5v")
        || normalized.contains("-vl")
        || normalized.contains("glm-ocr");
    profile
}

fn qwen_profile(normalized: &str) -> ModelFamilyProfile {
    // Alibaba Model Studio, retrieved 2026-10-13:
    // text-generation-model and vision-model were updated 2026-10-11;
    // deep-thinking describes enable_thinking, not a Codex effort enum.
    // Omni mentions reasoning_effort without listing the allowed names.
    // An empty ladder means the catalog must not invent low/medium/high/xhigh.
    // qwen-long publishes 10M, which is above the client maximum, so it stays unset.
    let mut profile = named_profile("Qwen", Vec::new(), None);
    if let Some(context_window) = qwen_official_context_window(normalized) {
        profile = with_official_window(profile, context_window);
    }
    profile.supports_images = qwen_official_vision(normalized);
    profile
}

fn qwen_official_context_window(normalized: &str) -> Option<u64> {
    if normalized.contains("qwen-long") {
        return None;
    }
    if normalized.contains("3.5-397b")
        || normalized.contains("3.5-122b")
        || normalized.contains("3.5-27b")
        || normalized.contains("3.5-35b")
        || normalized.contains("3.5-omni")
        || normalized.contains("qwen3-4b")
        || normalized.contains("qwen3-1.7b")
        || normalized.contains("qwen3-0.6b")
    {
        return Some(256_000);
    }
    if normalized.contains("qwen3.8-max")
        || normalized.contains("qwen3.8-flash")
        || normalized.contains("qwen3.8-omni")
        || normalized.contains("qwen3.7-plus")
        || normalized.contains("qwen3.7-flash")
        || normalized.contains("qwen3.7-max")
        || normalized.contains("qwen3.6-plus")
        || normalized.contains("qwen3.6-flash")
        || normalized.contains("qwen3.5-plus")
        || normalized.contains("qwen3.5-flash")
        || normalized.contains("qwen3-coder")
        || normalized.contains("qwen2.5-vl")
    {
        return Some(OFFICIAL_MILLION_CONTEXT_WINDOW);
    }
    None
}

fn qwen_official_vision(normalized: &str) -> bool {
    // The text-generation API guide says qwen3.7-max and the 2026-05-20
    // snapshot are text-only. The 2026-06-08 snapshot is multimodal.
    // Preview and 2026-05-17 are not listed as image models.
    if normalized.contains("qwen3.7-max-2026-05-20")
        || normalized.contains("qwen3.7-max-2026-05-17")
        || normalized.contains("qwen3.7-max-preview")
        || normalized.ends_with("qwen3.7-max")
    {
        return false;
    }
    normalized.contains("qwen3.7-max-2026-06-08")
        || normalized.contains("qwen3.8-max")
        || normalized.contains("qwen3.8-flash")
        || normalized.contains("qwen3.8-omni")
        || normalized.contains("qwen3.7-plus")
        || normalized.contains("qwen3.7-flash")
        || normalized.contains("qwen3.6-plus")
        || normalized.contains("qwen3.6-flash")
        || normalized.contains("omni")
        || normalized.contains("qwen2.5-vl")
        || normalized.contains("-vl")
}

fn with_official_window(
    mut profile: ModelFamilyProfile,
    context_window: u64,
) -> ModelFamilyProfile {
    profile.context_window = Some(context_window);
    profile.auto_compact_token_limit = Some(default_auto_compact_token_limit(context_window));
    profile
}

/// Generic fallback for an unrecognized upstream model family. Recognized
/// official family contracts always take precedence over this ladder.
fn fallback_reasoning_profile(family_name: &str) -> ModelFamilyProfile {
    named_profile(
        family_name,
        vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
        ],
        Some("high".to_string()),
    )
}

fn grok_version_is_4x(normalized: &str) -> bool {
    normalized.contains("4.") || normalized.contains("4-") || normalized.contains("grok-4")
}

fn named_profile(
    family_name: &str,
    reasoning_levels: Vec<String>,
    default_reasoning_level: Option<String>,
) -> ModelFamilyProfile {
    ModelFamilyProfile {
        family_name: family_name.to_string(),
        reasoning_levels,
        default_reasoning_level,
        strip_openai_speed_tiers: true,
        strip_unverified_multi_agent_effort: true,
        disable_openai_node_repl: true,
        context_window: None,
        auto_compact_token_limit: None,
        supports_images: false,
    }
}

fn unknown_family_label(upstream_model_id: &str) -> String {
    let last = upstream_model_id
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(upstream_model_id);
    let token = last
        .split('-')
        .find(|part| {
            let trimmed = part.trim();
            trimmed
                .chars()
                .any(|character| character.is_ascii_alphabetic())
                && trimmed.len() > 1
                && !is_hex_blob(trimmed)
        })
        .unwrap_or(last)
        .trim();
    title_case(token)
}

fn is_hex_blob(value: &str) -> bool {
    value.len() >= 8 && value.chars().all(|character| character.is_ascii_hexdigit())
}

fn title_case(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => {
            first.to_ascii_uppercase().to_string() + &characters.as_str().to_ascii_lowercase()
        }
        None => "this model".to_string(),
    }
}

/// Reasoning efforts to persist on a RouteSet capability snapshot.
///
/// Persist family contract for all API-key routes. The UI/official selected
/// family contract controls advertised levels; protocol forwarding is handled
/// separately and a relay that rejects an effort returns its own error.
pub fn family_reasoning_levels_for_route(route: &ModelRoute) -> Vec<String> {
    if is_internal_codex_model_id(&route.upstream_model_id)
        || is_internal_codex_model_id(&route.public_model_id)
    {
        return Vec::new();
    }
    resolve_model_family(&route.upstream_model_id).reasoning_levels
}

pub fn allowed_reasoning_levels_for_model(upstream_model_id: &str) -> Vec<String> {
    resolve_model_family(upstream_model_id).reasoning_levels
}

/// Responses image input for this route. Chat adapters stay text-only.
pub fn family_image_support_for_route(route: &ModelRoute) -> bool {
    !is_internal_codex_model_id(&route.upstream_model_id)
        && !is_internal_codex_model_id(&route.public_model_id)
        && resolve_model_family(&route.upstream_model_id).supports_images
}

/// Family-default client window and compact threshold for a RouteSet snapshot.
///
/// `None` means the catalog must not copy GPT skeleton numbers. Compact is an
/// explicit family value when present; callers default remaining windows to 95%.
pub fn family_context_limits_for_route(route: &ModelRoute) -> (Option<u64>, Option<u64>) {
    if is_internal_codex_model_id(&route.upstream_model_id)
        || is_internal_codex_model_id(&route.public_model_id)
    {
        return (None, None);
    }
    let profile = resolve_model_family(&route.upstream_model_id);
    (profile.context_window, profile.auto_compact_token_limit)
}

/// Codex-aligned default compact threshold: 95% of the catalog window.
pub fn default_auto_compact_token_limit(context_window: u64) -> u64 {
    let compact = context_window.saturating_mul(95) / 100;
    compact.max(1).min(context_window)
}

/// Replace GPT-6 self-introduction; keep the rest of the cloned workspace contract.
pub fn overlay_base_instructions(existing: &str, profile: &ModelFamilyProfile) -> String {
    let identity = profile.identity_block();
    let trimmed = existing.trim();
    if trimmed.is_empty() {
        return identity;
    }
    let (first, rest) = split_first_paragraph(trimmed);
    if paragraph_claims_gpt6(&first) {
        if rest.trim().is_empty() {
            identity
        } else {
            format!("{identity}\n\n{}", rest.trim_start())
        }
    } else {
        format!("{identity}\n\n{trimmed}")
    }
}

fn split_first_paragraph(text: &str) -> (String, String) {
    if let Some((first, rest)) = text.split_once("\n\n") {
        return (first.to_string(), rest.to_string());
    }
    (text.to_string(), String::new())
}

fn paragraph_claims_gpt6(paragraph: &str) -> bool {
    let lowered = paragraph.to_ascii_lowercase();
    lowered.contains("based on gpt-6") || lowered.contains("you are codex, an agent based on")
}

fn reasoning_description(effort: &str) -> String {
    match effort {
        "none" => "No extra reasoning".to_string(),
        "low" => "Fast responses with lighter reasoning".to_string(),
        "medium" => "Balances speed and reasoning depth for everyday tasks".to_string(),
        "high" => "Greater reasoning depth for complex problems".to_string(),
        "xhigh" => "Extra high reasoning depth for complex problems".to_string(),
        "max" => "Deepest reasoning effort published for this model".to_string(),
        other => format!("{other} reasoning"),
    }
}

fn advertised_reasoning(
    route: &ModelRoute,
    profile: &ModelFamilyProfile,
) -> Option<(String, Vec<String>)> {
    // A persisted route may predate this overlay and still contain GPT-only
    // values. A family with no published effort enum advertises nothing, so a
    // saved generic ladder cannot leak back. Known ladders are filtered so an
    // old max or ultra snapshot cannot leak into another family.
    let levels = if profile.reasoning_levels.is_empty() {
        Vec::new()
    } else if route.capabilities.reasoning_levels.is_empty() {
        profile.reasoning_levels.clone()
    } else {
        profile
            .reasoning_levels
            .iter()
            .filter(|level| {
                route
                    .capabilities
                    .reasoning_levels
                    .iter()
                    .any(|value| value == *level)
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    if levels.is_empty() {
        return None;
    }
    let default = route
        .capabilities
        .default_reasoning_level
        .as_ref()
        .filter(|default| levels.contains(default))
        .cloned()
        .unwrap_or_else(|| default_reasoning_level(&levels, profile));
    Some((default, levels))
}

fn default_reasoning_level(levels: &[String], profile: &ModelFamilyProfile) -> String {
    if let Some(default) = profile.default_reasoning_level.as_ref() {
        if levels.iter().any(|level| level == default) {
            return default.clone();
        }
    }
    levels[0].clone()
}

/// Auto-review should follow the model the user is currently talking to.
///
/// Codex reads `auto_review_model_override` from the active catalog row. Writing
/// this row's own public id means Grok reviews Grok work and DeepSeek reviews
/// DeepSeek work. Native GPT rows in a merged compatibility catalog never enter
/// this function, so they keep official review. Chat-only rows cannot represent
/// review tool turns, so they stay unset.
pub fn auto_review_override_for_route(route: &ModelRoute) -> Option<String> {
    if is_internal_codex_model_id(&route.upstream_model_id)
        || is_internal_codex_model_id(&route.public_model_id)
        || is_ephemeral_family_alias(route)
    {
        return None;
    }
    if route.wire_api != WireApi::Responses
        || !matches!(route.source, RouteSource::ApiProvider { .. })
    {
        return None;
    }
    Some(route.public_model_id.clone())
}

/// Apply identity, reasoning, review routing, and GPT-only capability stripping.
pub fn overlay_third_party_catalog_entry(
    entry: &mut JsonMap<String, JsonValue>,
    route: &ModelRoute,
    supports_auto_review_override: bool,
) {
    let internal = is_internal_codex_model_id(&route.upstream_model_id)
        || is_internal_codex_model_id(&route.public_model_id);
    // A compatibility catalog can also contain native OpenAI routes. Leave
    // their identity and official review contract exactly as Codex shipped it.
    if !matches!(route.source, RouteSource::ApiProvider { .. }) {
        return;
    }
    if supports_auto_review_override {
        entry.insert(
            "auto_review_model_override".to_string(),
            auto_review_override_for_route(route)
                .map(JsonValue::String)
                .unwrap_or(JsonValue::Null),
        );
    }
    if internal {
        return;
    }

    let profile = resolve_model_family(&route.upstream_model_id);
    let existing_instructions = entry
        .get("base_instructions")
        .and_then(JsonValue::as_str)
        .unwrap_or("");
    entry.insert(
        "base_instructions".to_string(),
        JsonValue::String(overlay_base_instructions(existing_instructions, &profile)),
    );
    // Newer Codex builds also keep the full developer prompt under
    // `model_messages.instructions_template`. Overlay both entry points or a
    // mode switch can silently restore the cloned GPT identity.
    if let Some(model_messages) = entry
        .get_mut("model_messages")
        .and_then(JsonValue::as_object_mut)
    {
        if let Some(instructions_template) = model_messages
            .get("instructions_template")
            .and_then(JsonValue::as_str)
            .map(str::to_string)
        {
            model_messages.insert(
                "instructions_template".to_string(),
                JsonValue::String(overlay_base_instructions(&instructions_template, &profile)),
            );
        }
    }

    match advertised_reasoning(route, &profile) {
        Some((default_level, levels)) => {
            let rendered = levels
                .iter()
                .map(|effort| {
                    serde_json::json!({
                        "effort": effort,
                        "description": reasoning_description(effort),
                    })
                })
                .collect::<Vec<_>>();
            entry.insert(
                "default_reasoning_level".to_string(),
                JsonValue::String(default_level),
            );
            entry.insert(
                "supported_reasoning_levels".to_string(),
                JsonValue::Array(rendered),
            );
        }
        None => {
            // Preserve the current Codex schema while ensuring a cloned GPT
            // reasoning ladder cannot be selected for an unverified route.
            entry.remove("default_reasoning_level");
            entry.insert(
                "supported_reasoning_levels".to_string(),
                JsonValue::Array(Vec::new()),
            );
        }
    }

    if profile.strip_openai_speed_tiers {
        entry.remove("service_tiers");
        entry.remove("additional_speed_tiers");
    }
    if profile.strip_unverified_multi_agent_effort {
        entry.remove("multi_agent_reasoning_effort");
    }
    if profile.disable_openai_node_repl {
        // Preserve the installed Codex field shape when present, but force the
        // capability off for third-party rows that did not declare it.
        if entry.contains_key("node_repl_disabled") {
            entry.insert("node_repl_disabled".to_string(), JsonValue::Bool(true));
        }
        entry.remove("node_repl_auto_review_required");
    }

    // `supports_experimental_context` is inherited from the GPT-6 Astra
    // skeleton, but it is a Codex-side experimental context-management
    // contract, not evidence that a third-party upstream accepts compaction.
    // Remove the cloned marker so the installed Codex uses its conservative
    // default instead of advertising Astra-only context behavior for Grok,
    // DeepSeek, Claude, or an unknown provider family.
    entry.remove("supports_experimental_context");

    // Window numbers belong on CapabilitySet. A cloned GPT skeleton must not
    // keep 272000/872000 (or any other inherited window) when this route still
    // has no verified client window. Official bundled rows never enter here.
    if route.capabilities.context_window.is_none() {
        entry.remove("context_window");
        entry.remove("max_context_window");
        entry.remove("auto_compact_token_limit");
    }

    // `original` is a Codex image-detail extension on the GPT skeleton, not an
    // xAI or other third-party detail tier. A probe that accepts the string does
    // not mean that model family supports it. Keep the field only for the GPT
    // family, matched by upstream model id (gpt-* or openai). Remove it for every
    // other family, including Grok with images enabled. A user-supplied GPT
    // relay that rejects original is outside this catalog contract.
    if profile.family_name != "GPT" {
        entry.remove("supports_image_detail_original");
    }
    if !route.capabilities.supports_web_search {
        entry.remove("web_search_tool_type");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{CapabilitySet, RouteSource};

    fn sample_route(upstream: &str, public_id: &str, wire_api: WireApi) -> ModelRoute {
        ModelRoute {
            id: "route-a".to_string(),
            route_identity_id: "identity-a".to_string(),
            public_model_id: public_id.to_string(),
            display_name: public_id.to_string(),
            source: RouteSource::ApiProvider {
                provider_id: "provider-a".to_string(),
                provider_revision_id: "provider-revision-a".to_string(),
            },
            upstream_model_id: upstream.to_string(),
            wire_api,
            adapter_kind: "responses_passthrough".to_string(),
            adapter_version: "1".to_string(),
            capabilities: CapabilitySet::default(),
            position: 0,
        }
    }

    #[test]
    fn grok_46_uses_xhigh_and_defaults_to_high() {
        let profile = resolve_model_family("grok-4.6-latest");
        assert_eq!(profile.family_name, "Grok");
        assert_eq!(profile.reasoning_levels, ["low", "medium", "high", "xhigh"]);
        assert_eq!(profile.default_reasoning_level.as_deref(), Some("high"));
        assert_eq!(profile.context_window, Some(GROK_CONTEXT_WINDOW));
        assert_eq!(
            profile.auto_compact_token_limit,
            Some(GROK_AUTO_COMPACT_TOKEN_LIMIT)
        );
        assert!(profile.identity_block().contains("你是 Grok"));
        assert!(!profile.identity_block().contains("GPT-6"));
        assert!(profile.supports_images);
    }

    #[test]
    fn other_grok_versions_share_the_500k_window_this_round() {
        let profile = resolve_model_family("grok-4");
        assert_eq!(profile.context_window, Some(GROK_CONTEXT_WINDOW));
        assert_eq!(
            profile.auto_compact_token_limit,
            Some(GROK_AUTO_COMPACT_TOKEN_LIMIT)
        );
    }

    #[test]
    fn unknown_and_named_non_grok_families_do_not_invent_windows() {
        for upstream in ["deepseek-chat", "claude-sonnet", "mystery-model"] {
            let profile = resolve_model_family(upstream);
            assert_eq!(profile.context_window, None, "{upstream}");
            assert_eq!(profile.auto_compact_token_limit, None, "{upstream}");
        }
    }

    #[test]
    fn family_context_limits_follow_upstream_id() {
        let grok = sample_route("grok-4.6", "3r-grok", WireApi::Responses);
        assert_eq!(
            family_context_limits_for_route(&grok),
            (
                Some(GROK_CONTEXT_WINDOW),
                Some(GROK_AUTO_COMPACT_TOKEN_LIMIT)
            )
        );
        let deepseek = sample_route("deepseek-chat", "3r-ds", WireApi::Responses);
        assert_eq!(family_context_limits_for_route(&deepseek), (None, None));
        assert_eq!(default_auto_compact_token_limit(272_000), 258_400);
        assert_eq!(
            default_auto_compact_token_limit(GROK_CONTEXT_WINDOW),
            GROK_AUTO_COMPACT_TOKEN_LIMIT
        );
    }

    #[test]
    fn hashed_public_looking_upstream_still_matches_grok() {
        let profile = resolve_model_family("a55ea5dd-0fb1-4599-a759-ce9b1c6e-grok-4.6");
        assert_eq!(profile.family_name, "Grok");
        assert_eq!(profile.default_reasoning_level.as_deref(), Some("high"));
    }

    #[test]
    fn identity_replaces_gpt6_opener_and_keeps_workspace_rest() {
        let profile = resolve_model_family("grok-4.6");
        let overlaid = overlay_base_instructions(
            "You are Codex, an agent based on GPT-6. You and the user share one workspace.\n\nKeep local tools.",
            &profile,
        );
        assert!(overlaid.starts_with("你是 Grok，在 Codex 工作区里用本地工具。"));
        assert!(!overlaid.contains("based on GPT-6"));
        assert!(overlaid.contains("Keep local tools."));
    }

    #[test]
    fn auto_review_points_at_self_for_responses_and_skips_chat() {
        let grok = sample_route("grok-4.6", "3r-grok", WireApi::Responses);
        let chat = sample_route("deepseek-chat", "3r-ds", WireApi::ChatCompletions);
        let internal = sample_route("codex-auto-review", "codex-auto-review", WireApi::Responses);
        assert_eq!(
            auto_review_override_for_route(&grok).as_deref(),
            Some("3r-grok")
        );
        assert_eq!(auto_review_override_for_route(&chat), None);
        assert_eq!(auto_review_override_for_route(&internal), None);
    }

    #[test]
    fn grok_overlay_replaces_gpt_contract_and_points_review_at_self() {
        let route = sample_route("grok-4.6", "3r-grok", WireApi::Responses);
        let mut entry = serde_json::json!({
            "base_instructions": "You are Codex, an agent based on GPT-6.\n\nKeep local tools.",
            "model_messages": {
                "instructions_template": "You are Codex, an agent based on GPT-6.\n\nKeep template tools.",
                "multi_agent": {"role": {"root": "GPT multi-agent defaults"}}
            },
            "default_reasoning_level": "low",
            "supported_reasoning_levels": [
                {"effort": "low"}, {"effort": "max"}, {"effort": "ultra"}
            ],
            "service_tiers": [{"id": "priority"}],
            "additional_speed_tiers": ["fast"],
            "multi_agent_reasoning_effort": "xhigh",
            "multi_agent_version": "v2",
            "node_repl_disabled": false,
            "node_repl_auto_review_required": true,
            "supports_experimental_context": true,
            "context_window": 272000,
            "max_context_window": 872000,
            "auto_compact_token_limit": 258400,
            "supports_image_detail_original": true,
            "web_search_tool_type": "text_and_image",
            "tool_mode": "code_mode_only",
            "apply_patch_tool_type": "freeform"
        })
        .as_object()
        .unwrap()
        .clone();

        overlay_third_party_catalog_entry(&mut entry, &route, true);

        assert!(entry["base_instructions"]
            .as_str()
            .unwrap()
            .starts_with("你是 Grok，在 Codex 工作区里用本地工具。"));
        assert!(!entry["base_instructions"]
            .as_str()
            .unwrap()
            .contains("based on GPT-6"));
        assert!(entry["model_messages"]["instructions_template"]
            .as_str()
            .unwrap()
            .starts_with("你是 Grok，在 Codex 工作区里用本地工具。"));
        assert!(entry["model_messages"].get("multi_agent").is_some());
        assert_eq!(entry["default_reasoning_level"], "high");
        assert_eq!(
            entry["supported_reasoning_levels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|level| level["effort"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["low", "medium", "high", "xhigh"]
        );
        assert_eq!(entry["auto_review_model_override"], "3r-grok");
        assert!(!entry.contains_key("service_tiers"));
        assert!(!entry.contains_key("additional_speed_tiers"));
        assert!(!entry.contains_key("multi_agent_reasoning_effort"));
        assert_eq!(entry["multi_agent_version"], "v2");
        assert_eq!(entry["node_repl_disabled"], true);
        assert!(!entry.contains_key("node_repl_auto_review_required"));
        assert!(!entry.contains_key("supports_experimental_context"));
        assert!(!entry.contains_key("context_window"));
        assert!(!entry.contains_key("max_context_window"));
        assert!(!entry.contains_key("auto_compact_token_limit"));
        assert!(!entry.contains_key("supports_image_detail_original"));
        assert!(!entry.contains_key("web_search_tool_type"));
        assert_eq!(entry["tool_mode"], "code_mode_only");
        assert_eq!(entry["apply_patch_tool_type"], "freeform");
    }

    #[test]
    fn overlay_keeps_capability_windows_when_present() {
        let mut route = sample_route("grok-4.6", "3r-grok", WireApi::Responses);
        route.capabilities.context_window = Some(GROK_CONTEXT_WINDOW);
        route.capabilities.auto_compact_token_limit = Some(GROK_AUTO_COMPACT_TOKEN_LIMIT);
        let mut entry = serde_json::json!({
            "context_window": 272000,
            "max_context_window": 872000,
            "auto_compact_token_limit": 258400
        })
        .as_object()
        .unwrap()
        .clone();
        overlay_third_party_catalog_entry(&mut entry, &route, false);
        assert_eq!(entry["context_window"], 272000);
        assert_eq!(entry["max_context_window"], 872000);
        assert_eq!(entry["auto_compact_token_limit"], 258400);
    }

    #[test]
    fn native_openai_routes_keep_bundled_windows() {
        let mut route = sample_route("gpt-5.4", "gpt-5.4", WireApi::Responses);
        route.source = RouteSource::Official {
            account_id: "account-a".to_string(),
            credential_revision_id: "credential-a".to_string(),
        };
        let mut entry = serde_json::json!({
            "context_window": 272000,
            "max_context_window": 872000
        })
        .as_object()
        .unwrap()
        .clone();
        overlay_third_party_catalog_entry(&mut entry, &route, true);
        assert_eq!(entry["context_window"], 272000);
        assert_eq!(entry["max_context_window"], 872000);
    }
    #[test]
    fn overlay_drops_original_image_detail_even_when_images_are_supported() {
        let mut route = sample_route("grok-4.6", "3r-grok", WireApi::Responses);
        route.capabilities.supports_images = true;
        let mut entry = serde_json::json!({
            "supports_image_detail_original": true,
            "web_search_tool_type": "text_and_image"
        })
        .as_object()
        .unwrap()
        .clone();
        overlay_third_party_catalog_entry(&mut entry, &route, false);
        assert!(!entry.contains_key("supports_image_detail_original"));
        assert!(!entry.contains_key("web_search_tool_type"));
    }

    #[test]
    fn gpt_family_keeps_original_image_detail_on_a_channel_route() {
        let mut route = sample_route("gpt-6-luna", "gpt-6-luna", WireApi::Responses);
        route.capabilities.supports_images = true;
        let mut entry = serde_json::json!({
            "supports_image_detail_original": true
        })
        .as_object()
        .unwrap()
        .clone();
        overlay_third_party_catalog_entry(&mut entry, &route, false);
        assert_eq!(entry["supports_image_detail_original"], true);
    }

    #[test]
    fn unknown_family_also_strips_unverified_image_and_search_ads() {
        let route = sample_route("mystery-model", "3r-mystery", WireApi::Responses);
        let mut entry = serde_json::json!({
            "supports_image_detail_original": true,
            "web_search_tool_type": "text_and_image"
        })
        .as_object()
        .unwrap()
        .clone();
        overlay_third_party_catalog_entry(&mut entry, &route, false);
        assert!(!entry.contains_key("supports_image_detail_original"));
        assert!(!entry.contains_key("web_search_tool_type"));
    }

    #[test]
    fn unknown_family_uses_the_product_reasoning_fallback() {
        let route = sample_route("mystery-model", "3r-mystery", WireApi::Responses);
        let mut entry = serde_json::json!({
            "default_reasoning_level": "high",
            "supported_reasoning_levels": [{"effort": "high"}]
        })
        .as_object()
        .unwrap()
        .clone();
        overlay_third_party_catalog_entry(&mut entry, &route, false);
        assert_eq!(entry["default_reasoning_level"], "high");
        assert_eq!(
            entry["supported_reasoning_levels"],
            serde_json::json!([
                {"effort": "low", "description": "Fast responses with lighter reasoning"},
                {"effort": "medium", "description": "Balances speed and reasoning depth for everyday tasks"},
                {"effort": "high", "description": "Greater reasoning depth for complex problems"},
                {"effort": "xhigh", "description": "Extra high reasoning depth for complex problems"}
            ])
        );
    }

    #[test]
    fn native_openai_routes_keep_image_and_search_advertising() {
        let mut route = sample_route("gpt-5.4", "gpt-5.4", WireApi::Responses);
        route.source = RouteSource::Official {
            account_id: "account-a".to_string(),
            credential_revision_id: "credential-a".to_string(),
        };
        let mut entry = serde_json::json!({
            "supports_image_detail_original": true,
            "web_search_tool_type": "text_and_image"
        })
        .as_object()
        .unwrap()
        .clone();
        overlay_third_party_catalog_entry(&mut entry, &route, true);
        assert_eq!(entry["supports_image_detail_original"], true);
        assert_eq!(entry["web_search_tool_type"], "text_and_image");
    }
    #[test]
    fn grok_4x_responses_advertises_images_chat_and_grok3_do_not() {
        let responses = sample_route("grok-4.6", "3r-grok", WireApi::Responses);
        assert!(family_image_support_for_route(&responses));
        let chat = sample_route("grok-4.6", "3r-grok", WireApi::ChatCompletions);
        assert!(family_image_support_for_route(&chat));
        assert!(resolve_model_family("grok-4.5").supports_images);
        assert!(!resolve_model_family("grok-3-mini").supports_images);
        assert!(!resolve_model_family("deepseek-chat").supports_images);
    }

    #[test]
    fn official_family_cards_do_not_use_the_generic_ladder() {
        let flash = resolve_model_family("deepseek-flash");
        assert_eq!(flash.family_name, "DeepSeek");
        assert_eq!(flash.reasoning_levels, ["none", "low", "high", "max"]);
        assert_eq!(flash.default_reasoning_level.as_deref(), Some("high"));
        assert_eq!(flash.context_window, Some(OFFICIAL_MILLION_CONTEXT_WINDOW));
        assert_eq!(
            flash.auto_compact_token_limit,
            Some(OFFICIAL_MILLION_AUTO_COMPACT_TOKEN_LIMIT)
        );
        assert!(flash.supports_images);

        let pro = resolve_model_family("deepseek-v4-pro");
        assert_eq!(pro.context_window, Some(OFFICIAL_MILLION_CONTEXT_WINDOW));
        assert!(!pro.supports_images);
        let retired = resolve_model_family("deepseek-chat");
        assert_eq!(retired.reasoning_levels, flash.reasoning_levels);
        assert_eq!(retired.context_window, None);
        assert!(!retired.supports_images);

        let glm53 = resolve_model_family("glm-5.3");
        assert_eq!(glm53.reasoning_levels, ["low", "high", "max"]);
        assert_eq!(glm53.default_reasoning_level.as_deref(), Some("max"));
        assert_eq!(glm53.context_window, Some(OFFICIAL_MILLION_CONTEXT_WINDOW));
        assert!(!glm53.supports_images);
        let glm_flash = resolve_model_family("glm-5.3-flash");
        assert_eq!(glm_flash.reasoning_levels, glm53.reasoning_levels);
        assert!(glm_flash.supports_images);
        let glm52 = resolve_model_family("glm-5.2");
        assert_eq!(glm52.reasoning_levels, ["none", "high", "max"]);
        assert_eq!(glm52.default_reasoning_level.as_deref(), Some("max"));
        assert!(!glm52.supports_images);
        let glm46 = resolve_model_family("glm-4.6");
        assert!(glm46.reasoning_levels.is_empty());
        assert_eq!(glm46.context_window, None);
        assert!(!glm46.supports_images);

        let qwen = resolve_model_family("qwen3.8-max");
        assert_eq!(qwen.family_name, "Qwen");
        assert!(qwen.reasoning_levels.is_empty());
        assert_eq!(qwen.context_window, Some(OFFICIAL_MILLION_CONTEXT_WINDOW));
        assert!(qwen.supports_images);
        assert!(!resolve_model_family("qwen3.7-max").supports_images);
        assert!(resolve_model_family("qwen3.7-max-2026-06-08").supports_images);
        assert!(!resolve_model_family("qwen3.7-max-2026-05-20").supports_images);
        assert_eq!(
            resolve_model_family("qwen3.5-27b").context_window,
            Some(256_000)
        );
        assert_eq!(resolve_model_family("qwen-long").context_window, None);
        assert_eq!(
            resolve_model_family("claude-sonnet").reasoning_levels,
            ["low", "medium", "high", "xhigh"]
        );
    }

    #[test]
    fn qwen_overlay_drops_a_stale_generic_ladder() {
        let mut route = sample_route("qwen3.8-max", "3r-qwen", WireApi::Responses);
        route.capabilities.reasoning_levels = ["low", "medium", "high", "xhigh"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut entry = serde_json::json!({
            "default_reasoning_level": "xhigh",
            "supported_reasoning_levels": [{"effort": "xhigh"}]
        })
        .as_object()
        .unwrap()
        .clone();
        overlay_third_party_catalog_entry(&mut entry, &route, false);
        assert!(!entry.contains_key("default_reasoning_level"));
        assert_eq!(entry["supported_reasoning_levels"], serde_json::json!([]));
    }
}
