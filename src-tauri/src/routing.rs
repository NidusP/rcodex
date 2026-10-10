use std::{collections::BTreeSet, fmt};

use serde::{Deserialize, Serialize};

const MAX_PUBLIC_MODEL_ID_LEN: usize = 128;
const MAX_INTERNAL_ID_LEN: usize = 160;
const MAX_LABEL_LEN: usize = 160;
const MAX_UPSTREAM_MODEL_ID_LEN: usize = 256;
const MAX_CATALOG_PATH_LEN: usize = 4_096;
const MAX_CONFIG_SNAPSHOT_LEN: usize = 65_536;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    Official,
    ApiProvider,
}

impl RouteKind {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::ApiProvider => "api_provider",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "official" => Some(Self::Official),
            "api_provider" => Some(Self::ApiProvider),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireApi {
    Responses,
    ChatCompletions,
}

impl WireApi {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Responses => "responses",
            Self::ChatCompletions => "chat_completions",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "responses" => Some(Self::Responses),
            "chat_completions" => Some(Self::ChatCompletions),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteSetState {
    Materializing,
    Ready,
    Failed,
}

/// Process-level compact path chosen at launch. Missing snapshots fail closed
/// as `local_client` so old bindings keep today's third-party trigger reject.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactMode {
    #[default]
    LocalClient,
    RemoteV2,
}

impl CompactMode {
    pub fn from_gateway_snapshot(snapshot: &str) -> Self {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(snapshot) else {
            return Self::LocalClient;
        };
        match value.get("compactMode").and_then(serde_json::Value::as_str) {
            Some("remote_v2") => Self::RemoteV2,
            _ => Self::LocalClient,
        }
    }

    pub fn as_snapshot_str(self) -> &'static str {
        match self {
            Self::LocalClient => "local_client",
            Self::RemoteV2 => "remote_v2",
        }
    }
}

/// Route-level compact contract. Default `unsupported` keeps old
/// `capabilities_json` loadable without a schema bump.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionSupport {
    #[default]
    Unsupported,
    NativeRemote,
    GatewaySummarizer,
}

impl CompactionSupport {
    pub fn for_api_provider(compact_mode: CompactMode, looks_like_gpt: bool) -> Self {
        match compact_mode {
            CompactMode::RemoteV2 if !looks_like_gpt => Self::GatewaySummarizer,
            _ => Self::Unsupported,
        }
    }
}

/// Fail-closed heuristic only. Never used to grant `native_remote`.
///
/// Known Grok / DeepSeek / Claude upstream ids are excluded even when a public
/// slug happens to contain `gpt`. Relay `gpt-*` names stay fail-closed.
pub fn looks_like_gpt_model_id(public_model_id: &str, upstream_model_id: &str) -> bool {
    if known_non_gpt_family(upstream_model_id) {
        return false;
    }
    chatgpt_public_slug(public_model_id) || chatgpt_public_slug(upstream_model_id)
}

fn known_non_gpt_family(model_id: &str) -> bool {
    let normalized = model_id.trim().to_ascii_lowercase();
    normalized.contains("grok")
        || normalized.contains("deepseek")
        || normalized.contains("claude")
        || normalized.contains("anthropic")
}

fn chatgpt_public_slug(model_id: &str) -> bool {
    let slug = model_id
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(model_id)
        .trim()
        .to_ascii_lowercase();
    if slug.starts_with("gpt-") || slug == "gpt" || slug.starts_with("chatgpt") {
        return true;
    }
    slug == "o1"
        || slug == "o3"
        || slug.starts_with("o1-")
        || slug.starts_with("o3-")
        || slug.starts_with("o4-")
}

/// Adapter kind for App-hardcoded ephemeral slugs remapped onto a selected
/// family Responses route. Hidden from pickers and GET /v1/models.
pub const EPHEMERAL_FAMILY_ALIAS_ADAPTER: &str = "ephemeral_family_alias";
pub fn is_ephemeral_family_alias(route: &ModelRoute) -> bool {
    route.adapter_kind == EPHEMERAL_FAMILY_ALIAS_ADAPTER
}

impl RouteSetState {
    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "materializing" => Some(Self::Materializing),
            "ready" => Some(Self::Ready),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySet {
    pub context_window: Option<u64>,
    #[serde(default)]
    pub auto_compact_token_limit: Option<u64>,
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub input_modalities: Vec<String>,
    #[serde(default)]
    pub reasoning_levels: Vec<String>,
    #[serde(default)]
    pub default_reasoning_level: Option<String>,
    #[serde(default)]
    pub supports_tools: bool,
    #[serde(default)]
    pub supports_images: bool,
    #[serde(default)]
    pub supports_web_search: bool,
    #[serde(default)]
    pub supports_websocket: bool,
    #[serde(default)]
    pub compaction_support: CompactionSupport,
}

impl CapabilitySet {
    pub fn validate(&self) -> Result<(), RoutingError> {
        if self.context_window == Some(0)
            || self.auto_compact_token_limit == Some(0)
            || self.max_output_tokens == Some(0)
        {
            return Err(RoutingError::new(
                "invalid_capabilities",
                "Token limits must be greater than zero when present.",
            ));
        }
        if let (Some(context), Some(compact)) = (self.context_window, self.auto_compact_token_limit)
        {
            if compact > context {
                return Err(RoutingError::new(
                    "invalid_capabilities",
                    "The auto-compact threshold cannot exceed the context window.",
                ));
            }
        }
        if let (Some(context), Some(output)) = (self.context_window, self.max_output_tokens) {
            if output > context {
                return Err(RoutingError::new(
                    "invalid_capabilities",
                    "The maximum output cannot exceed the context window.",
                ));
            }
        }
        validate_unique_labels(&self.input_modalities, "input modalities")?;
        validate_unique_labels(&self.reasoning_levels, "reasoning levels")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteSource {
    Official {
        account_id: String,
        credential_revision_id: String,
    },
    ApiProvider {
        provider_id: String,
        provider_revision_id: String,
    },
}

impl RouteSource {
    pub fn kind(&self) -> RouteKind {
        match self {
            Self::Official { .. } => RouteKind::Official,
            Self::ApiProvider { .. } => RouteKind::ApiProvider,
        }
    }

    fn validate(&self) -> Result<(), RoutingError> {
        match self {
            Self::Official {
                account_id,
                credential_revision_id,
            } => {
                validate_internal_id(account_id, "official account")?;
                validate_internal_id(credential_revision_id, "official credential revision")
            }
            Self::ApiProvider {
                provider_id,
                provider_revision_id,
            } => {
                validate_internal_id(provider_id, "API Provider")?;
                validate_internal_id(provider_revision_id, "API Provider revision")
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRouteIdentity {
    pub id: String,
    pub public_model_id: String,
    pub source_kind: RouteKind,
    pub source_id: String,
    pub upstream_model_id: String,
}

impl ModelRouteIdentity {
    #[cfg(test)]
    pub fn validate(&self) -> Result<(), RoutingError> {
        validate_internal_id(&self.id, "model route identity")?;
        validate_public_model_id(&self.public_model_id)?;
        validate_internal_id(&self.source_id, "model route source")?;
        validate_upstream_model_id(&self.upstream_model_id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayTimeoutSnapshot {
    pub connect_seconds: u64,
    pub first_byte_seconds: u64,
    pub stream_idle_seconds: u64,
    pub non_stream_total_seconds: u64,
}

impl GatewayTimeoutSnapshot {
    pub fn validate(&self) -> Result<(), RoutingError> {
        if !(1..=120).contains(&self.connect_seconds)
            || !(1..=600).contains(&self.first_byte_seconds)
            || !(1..=1_800).contains(&self.stream_idle_seconds)
            || !(1..=7_200).contains(&self.non_stream_total_seconds)
            || self.non_stream_total_seconds < self.first_byte_seconds
        {
            return Err(RoutingError::new(
                "invalid_timeout_snapshot",
                "Gateway timeout values are outside the supported range.",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSetProviderRevision {
    pub provider_id: String,
    pub provider_revision_id: String,
    pub position: u32,
    pub timeouts: GatewayTimeoutSnapshot,
}

impl RouteSetProviderRevision {
    fn validate(&self) -> Result<(), RoutingError> {
        validate_internal_id(&self.provider_id, "API Provider")?;
        validate_internal_id(&self.provider_revision_id, "API Provider revision")?;
        self.timeouts.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRoute {
    pub id: String,
    pub route_identity_id: String,
    pub public_model_id: String,
    pub display_name: String,
    pub source: RouteSource,
    pub upstream_model_id: String,
    pub wire_api: WireApi,
    pub adapter_kind: String,
    pub adapter_version: String,
    pub capabilities: CapabilitySet,
    pub position: u32,
}

impl ModelRoute {
    pub fn validate(&self) -> Result<(), RoutingError> {
        validate_internal_id(&self.id, "model route")?;
        validate_internal_id(&self.route_identity_id, "model route identity")?;
        validate_public_model_id(&self.public_model_id)?;
        validate_label(&self.display_name, "model display name")?;
        self.source.validate()?;
        validate_upstream_model_id(&self.upstream_model_id)?;
        validate_label(&self.adapter_kind, "adapter kind")?;
        validate_label(&self.adapter_version, "adapter version")?;
        self.capabilities.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayServiceRevision {
    pub id: String,
    pub protocol_version: u32,
    pub config_snapshot: String,
    pub config_sha256: String,
    pub created_at: i64,
}

impl GatewayServiceRevision {
    fn validate(&self) -> Result<(), RoutingError> {
        validate_internal_id(&self.id, "Gateway service revision")?;
        if self.protocol_version == 0 {
            return Err(RoutingError::new(
                "invalid_gateway_revision",
                "Gateway protocol version must be greater than zero.",
            ));
        }
        validate_text(
            &self.config_snapshot,
            MAX_CONFIG_SNAPSHOT_LEN,
            "invalid_gateway_revision",
            "Gateway config snapshot is invalid.",
        )?;
        validate_label(&self.config_sha256, "Gateway config digest")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSetRevision {
    pub id: String,
    pub launch_preset_id: String,
    pub launch_preset_version: u64,
    pub official_account_id: Option<String>,
    pub official_credential_revision_id: Option<String>,
    pub gateway_service: GatewayServiceRevision,
    pub requires_openai_auth: bool,
    pub catalog_path: String,
    pub catalog_sha256: String,
    pub state: RouteSetState,
    pub created_at: i64,
    pub providers: Vec<RouteSetProviderRevision>,
    pub routes: Vec<ModelRoute>,
}

impl RouteSetRevision {
    pub fn validate_ready(&self) -> Result<(), RoutingError> {
        if self.state != RouteSetState::Ready {
            return Err(RoutingError::new(
                "route_set_not_ready",
                "Only ready RouteSet revisions can be published.",
            ));
        }
        validate_internal_id(&self.id, "RouteSet revision")?;
        validate_internal_id(&self.launch_preset_id, "launch preset")?;
        if self.launch_preset_version == 0 {
            return Err(RoutingError::new(
                "invalid_route_set",
                "Launch preset version must be greater than zero.",
            ));
        }
        self.gateway_service.validate()?;
        validate_text(
            &self.catalog_path,
            MAX_CATALOG_PATH_LEN,
            "invalid_catalog_path",
            "Model catalog path is invalid.",
        )?;
        validate_label(&self.catalog_sha256, "model catalog digest")?;

        match (
            self.requires_openai_auth,
            self.official_account_id.as_deref(),
            self.official_credential_revision_id.as_deref(),
        ) {
            (false, None, None) => {}
            (true, Some(account_id), Some(revision_id)) => {
                validate_internal_id(account_id, "official account")?;
                validate_internal_id(revision_id, "official credential revision")?;
            }
            _ => {
                return Err(RoutingError::new(
                    "invalid_auth_contract",
                    "Official account and credential revision must match the auth requirement.",
                ))
            }
        }

        let mut provider_ids = BTreeSet::new();
        let mut provider_revisions = BTreeSet::new();
        let mut provider_positions = BTreeSet::new();
        for provider in &self.providers {
            provider.validate()?;
            if !provider_ids.insert(provider.provider_id.as_str())
                || !provider_revisions.insert(provider.provider_revision_id.as_str())
                || !provider_positions.insert(provider.position)
            {
                return Err(RoutingError::new(
                    "duplicate_provider",
                    "A RouteSet cannot contain duplicate Provider identities, revisions, or positions.",
                ));
            }
        }

        if self.routes.is_empty() {
            return Err(RoutingError::new(
                "route_set_has_no_models",
                "A ready RouteSet must contain at least one model route.",
            ));
        }
        let mut route_ids = BTreeSet::new();
        let mut identity_ids = BTreeSet::new();
        let mut public_ids = BTreeSet::new();
        let mut route_positions = BTreeSet::new();
        for route in &self.routes {
            route.validate()?;
            if !route_ids.insert(route.id.as_str())
                || !identity_ids.insert(route.route_identity_id.as_str())
                || !public_ids.insert(route.public_model_id.as_str())
                || !route_positions.insert(route.position)
            {
                return Err(RoutingError::new(
                    "duplicate_model_route",
                    "A RouteSet cannot contain duplicate routes, identities, public model IDs, or positions.",
                ));
            }
            match &route.source {
                RouteSource::Official {
                    account_id,
                    credential_revision_id,
                } => {
                    if !self.requires_openai_auth
                        || self.official_account_id.as_deref() != Some(account_id)
                        || self.official_credential_revision_id.as_deref()
                            != Some(credential_revision_id)
                    {
                        return Err(RoutingError::new(
                            "route_source_not_selected",
                            "The official model route does not belong to this RouteSet.",
                        ));
                    }
                }
                RouteSource::ApiProvider {
                    provider_id,
                    provider_revision_id,
                } => {
                    if !self.providers.iter().any(|provider| {
                        provider.provider_id == *provider_id
                            && provider.provider_revision_id == *provider_revision_id
                    }) {
                        return Err(RoutingError::new(
                            "route_source_not_selected",
                            "The API Provider model route does not belong to this RouteSet.",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn route_for_public_model(&self, public_model_id: &str) -> Option<&ModelRoute> {
        self.routes
            .iter()
            .find(|route| route.public_model_id == public_model_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingError {
    pub code: &'static str,
    pub message: &'static str,
}

impl RoutingError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

impl fmt::Display for RoutingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RoutingError {}

pub fn validate_public_model_id(value: &str) -> Result<(), RoutingError> {
    if value.is_empty()
        || value.len() > MAX_PUBLIC_MODEL_ID_LEN
        || !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        || !value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(RoutingError::new(
            "invalid_public_model_id",
            "Public model IDs must be stable lowercase identifiers without slashes.",
        ));
    }
    Ok(())
}

/// Returns whether a model identifier belongs to a Codex-managed special role
/// rather than a user-selectable provider route.
pub fn is_internal_codex_model_id(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized == "codex-auto-review"
        || normalized == "gpt-code-review"
        || ((normalized.starts_with("codex-") || normalized.starts_with("gpt-"))
            && (normalized.ends_with("-auto-review")
                || normalized.ends_with("-auto_review")
                || normalized.ends_with("-code-review")
                || normalized.ends_with("-code_review")
                || normalized.ends_with("-subagent")
                || normalized.ends_with("-sub-agent")
                || normalized.ends_with("-auto-subagent")
                || normalized.ends_with("-auto_subagent")))
}

fn validate_internal_id(value: &str, field: &'static str) -> Result<(), RoutingError> {
    if value.trim() != value
        || value.is_empty()
        || value.len() > MAX_INTERNAL_ID_LEN
        || value.chars().any(char::is_control)
    {
        return Err(RoutingError::new("invalid_internal_id", field));
    }
    Ok(())
}

fn validate_label(value: &str, field: &'static str) -> Result<(), RoutingError> {
    validate_text(value, MAX_LABEL_LEN, "invalid_label", field)
}

fn validate_text(
    value: &str,
    max_len: usize,
    code: &'static str,
    message: &'static str,
) -> Result<(), RoutingError> {
    if value.trim() != value
        || value.is_empty()
        || value.len() > max_len
        || value.chars().any(char::is_control)
    {
        return Err(RoutingError::new(code, message));
    }
    Ok(())
}

fn validate_upstream_model_id(value: &str) -> Result<(), RoutingError> {
    if value.trim() != value
        || value.is_empty()
        || value.len() > MAX_UPSTREAM_MODEL_ID_LEN
        || value.chars().any(char::is_control)
    {
        return Err(RoutingError::new(
            "invalid_upstream_model_id",
            "Upstream model ID is invalid.",
        ));
    }
    Ok(())
}

fn validate_unique_labels(values: &[String], field: &'static str) -> Result<(), RoutingError> {
    let mut seen = BTreeSet::new();
    for value in values {
        validate_label(value, field)?;
        if !seen.insert(value.as_str()) {
            return Err(RoutingError::new(
                "invalid_capabilities",
                "Capability labels must be unique.",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_route_set() -> RouteSetRevision {
        RouteSetRevision {
            id: "route-set-a".to_string(),
            launch_preset_id: "preset-a".to_string(),
            launch_preset_version: 1,
            official_account_id: None,
            official_credential_revision_id: None,
            gateway_service: GatewayServiceRevision {
                id: "gateway-v1".to_string(),
                protocol_version: 1,
                config_snapshot: "{}".to_string(),
                config_sha256: "sha256:gateway".to_string(),
                created_at: 1,
            },
            requires_openai_auth: false,
            catalog_path: "/managed/models.json".to_string(),
            catalog_sha256: "sha256:catalog".to_string(),
            state: RouteSetState::Ready,
            created_at: 2,
            providers: vec![RouteSetProviderRevision {
                provider_id: "provider-a".to_string(),
                provider_revision_id: "provider-revision-a".to_string(),
                position: 0,
                timeouts: GatewayTimeoutSnapshot {
                    connect_seconds: 10,
                    first_byte_seconds: 30,
                    stream_idle_seconds: 45,
                    non_stream_total_seconds: 120,
                },
            }],
            routes: vec![ModelRoute {
                id: "route-a".to_string(),
                route_identity_id: "identity-a".to_string(),
                public_model_id: "3r-deepseek-stable".to_string(),
                display_name: "DeepSeek · Relay".to_string(),
                source: RouteSource::ApiProvider {
                    provider_id: "provider-a".to_string(),
                    provider_revision_id: "provider-revision-a".to_string(),
                },
                upstream_model_id: "deepseek-chat".to_string(),
                wire_api: WireApi::Responses,
                adapter_kind: "responses_passthrough".to_string(),
                adapter_version: "1".to_string(),
                capabilities: CapabilitySet {
                    context_window: Some(128_000),
                    max_output_tokens: Some(8_192),
                    input_modalities: vec!["text".to_string()],
                    supports_tools: true,
                    ..CapabilitySet::default()
                },
                position: 0,
            }],
        }
    }

    #[test]
    fn public_model_ids_are_stable_and_slash_free() {
        assert!(validate_public_model_id("3r-deepseek-stable").is_ok());
        for invalid in ["provider/model", "Uppercase", " leading", "", "模型"] {
            assert_eq!(
                validate_public_model_id(invalid).unwrap_err().code,
                "invalid_public_model_id"
            );
        }
    }

    #[test]
    fn internal_codex_role_models_are_not_user_routes() {
        assert!(is_internal_codex_model_id("codex-auto-review"));
        assert!(is_internal_codex_model_id("gpt-code-review"));
        assert!(is_internal_codex_model_id("codex-next-auto_review"));
        assert!(is_internal_codex_model_id("codex-subagent"));
        assert!(is_internal_codex_model_id("gpt-auto-subagent"));
        assert!(!is_internal_codex_model_id("gpt-5-codex"));
        assert!(!is_internal_codex_model_id("provider-code-review"));
    }

    #[test]
    fn exact_lookup_has_no_prefix_or_fallback_behavior() {
        let route_set = api_route_set();
        assert_eq!(
            route_set
                .route_for_public_model("3r-deepseek-stable")
                .unwrap()
                .upstream_model_id,
            "deepseek-chat"
        );
        assert!(route_set.route_for_public_model("3r-deepseek").is_none());
        assert!(route_set.route_for_public_model("missing").is_none());
    }

    #[test]
    fn ready_route_set_rejects_unselected_source_and_duplicate_public_id() {
        let mut route_set = api_route_set();
        route_set.routes[0].source = RouteSource::ApiProvider {
            provider_id: "provider-b".to_string(),
            provider_revision_id: "provider-revision-b".to_string(),
        };
        assert_eq!(
            route_set.validate_ready().unwrap_err().code,
            "route_source_not_selected"
        );

        let mut route_set = api_route_set();
        let mut duplicate = route_set.routes[0].clone();
        duplicate.id = "route-b".to_string();
        duplicate.route_identity_id = "identity-b".to_string();
        duplicate.position = 1;
        route_set.routes.push(duplicate);
        assert_eq!(
            route_set.validate_ready().unwrap_err().code,
            "duplicate_model_route"
        );
    }

    #[test]
    fn official_routes_require_the_exact_account_revision_pair() {
        let mut route_set = api_route_set();
        route_set.requires_openai_auth = true;
        route_set.official_account_id = Some("account-a".to_string());
        route_set.official_credential_revision_id = Some("credential-a".to_string());
        route_set.routes[0].source = RouteSource::Official {
            account_id: "account-a".to_string(),
            credential_revision_id: "credential-other".to_string(),
        };
        assert_eq!(
            route_set.validate_ready().unwrap_err().code,
            "route_source_not_selected"
        );
    }

    #[test]
    fn capability_limits_and_labels_are_validated() {
        let invalid = CapabilitySet {
            context_window: Some(100),
            max_output_tokens: Some(101),
            ..CapabilitySet::default()
        };
        assert_eq!(invalid.validate().unwrap_err().code, "invalid_capabilities");

        let duplicate = CapabilitySet {
            input_modalities: vec!["text".to_string(), "text".to_string()],
            ..CapabilitySet::default()
        };
        assert_eq!(
            duplicate.validate().unwrap_err().code,
            "invalid_capabilities"
        );
    }

    #[test]
    fn missing_compaction_support_deserializes_as_unsupported() {
        let capabilities: CapabilitySet =
            serde_json::from_str(r#"{"inputModalities":["text"],"supportsTools":true}"#).unwrap();
        assert_eq!(
            capabilities.compaction_support,
            CompactionSupport::Unsupported
        );
        assert!(capabilities.supports_tools);
        assert_eq!(capabilities.auto_compact_token_limit, None);
    }

    #[test]
    fn looks_like_gpt_is_fail_closed_and_ignores_known_families() {
        assert!(looks_like_gpt_model_id("relay-gpt-4o", "gpt-4o"));
        assert!(looks_like_gpt_model_id("gpt-5-codex", "gpt-5-codex"));
        assert!(looks_like_gpt_model_id("o3-mini", "o3-mini"));
        assert!(!looks_like_gpt_model_id("grok-gpt-4o", "grok-4.6"));
        assert!(!looks_like_gpt_model_id("deepseek-chat", "deepseek-chat"));
        assert!(!looks_like_gpt_model_id(
            "claude-sonnet",
            "claude-3-5-sonnet"
        ));
        assert!(looks_like_gpt_model_id("gpt-5.6-luna", "gpt-5.6-luna"));
        assert!(!looks_like_gpt_model_id("gpt-5.6-luna", "grok-4.6"));
    }

    #[test]
    fn compact_mode_defaults_missing_snapshot_to_local_client() {
        assert_eq!(
            CompactMode::from_gateway_snapshot("{}"),
            CompactMode::LocalClient
        );
        assert_eq!(
            CompactMode::from_gateway_snapshot(r#"{"compactMode":"remote_v2"}"#),
            CompactMode::RemoteV2
        );
        assert_eq!(
            CompactionSupport::for_api_provider(CompactMode::RemoteV2, true),
            CompactionSupport::Unsupported
        );
        assert_eq!(
            CompactionSupport::for_api_provider(CompactMode::RemoteV2, false),
            CompactionSupport::GatewaySummarizer
        );
        assert_eq!(
            CompactionSupport::for_api_provider(CompactMode::LocalClient, false),
            CompactionSupport::Unsupported
        );
    }

    #[test]
    fn serialized_gateway_config_is_not_limited_to_a_ui_label() {
        let mut route_set = api_route_set();
        route_set.gateway_service.config_snapshot =
            format!("{{\"padding\":\"{}\"}}", "x".repeat(512));
        route_set.catalog_path = format!("/managed/{}/models.json", "nested/".repeat(30));
        assert!(route_set.validate_ready().is_ok());
    }
}
