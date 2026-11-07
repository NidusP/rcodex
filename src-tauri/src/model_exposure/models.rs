use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelExposureSummary {
    pub provider_id: String,
    pub upstream_model_id: String,
    pub exposed: bool,
    pub image_probe_state: String,
    pub image_probe_checked_at: Option<i64>,
    pub image_probe_error_code: Option<String>,
    pub referenced_by: Vec<String>,
}

pub(crate) struct ExposureChange {
    pub summary: ModelExposureSummary,
    pub should_probe: bool,
}

/// Contains identifiers only. Credentials remain in ProviderStore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProbeContext {
    pub provider_id: String,
    pub provider_revision_id: String,
    pub upstream_model_id: String,
    pub wire_api: String,
}
