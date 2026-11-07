//! Model exposure IPC and image-probe orchestration. Domain writes belong to
//! ModelExposureStore; credentials never cross the IPC boundary.

use std::collections::HashSet;
use std::sync::Mutex;

use tauri::Emitter;
use tokio::sync::Semaphore;

use crate::model_exposure::{ModelExposureStore, ModelExposureSummary};
use crate::providers::ProviderStore;

pub(crate) struct ModelImageProbeJobs {
    slots: Semaphore,
    active: Mutex<HashSet<String>>,
}

impl Default for ModelImageProbeJobs {
    fn default() -> Self {
        Self {
            slots: Semaphore::new(2),
            active: Mutex::new(HashSet::new()),
        }
    }
}

struct ProbeJob<'a> {
    jobs: &'a ModelImageProbeJobs,
    key: String,
}

impl Drop for ProbeJob<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.jobs.active.lock() {
            active.remove(&self.key);
        }
    }
}

#[tauri::command]
pub(crate) fn list_provider_model_exposures(
    provider_id: String,
    exposures: tauri::State<'_, ModelExposureStore>,
) -> Result<Vec<ModelExposureSummary>, String> {
    exposures.list(&provider_id)
}

#[tauri::command]
pub(crate) async fn set_provider_model_exposure(
    app: tauri::AppHandle,
    provider_id: String,
    upstream_model_id: String,
    exposed: bool,
    exposures: tauri::State<'_, ModelExposureStore>,
    providers: tauri::State<'_, ProviderStore>,
    jobs: tauri::State<'_, ModelImageProbeJobs>,
) -> Result<ModelExposureSummary, String> {
    let change = exposures.set_exposure(&provider_id, &upstream_model_id, exposed)?;
    let _ = app.emit("codex-control-snapshot-invalidated", ());
    if !change.should_probe {
        return Ok(change.summary);
    }
    let result = run_image_probe(
        &exposures,
        &providers,
        &jobs,
        &provider_id,
        &upstream_model_id,
    )
    .await;
    let _ = app.emit("codex-control-snapshot-invalidated", ());
    result
}

#[tauri::command]
pub(crate) async fn refresh_model_image_probe(
    app: tauri::AppHandle,
    provider_id: String,
    upstream_model_id: String,
    exposures: tauri::State<'_, ModelExposureStore>,
    providers: tauri::State<'_, ProviderStore>,
    jobs: tauri::State<'_, ModelImageProbeJobs>,
) -> Result<ModelExposureSummary, String> {
    let result = run_image_probe(
        &exposures,
        &providers,
        &jobs,
        &provider_id,
        &upstream_model_id,
    )
    .await;
    let _ = app.emit("codex-control-snapshot-invalidated", ());
    result
}

async fn run_image_probe(
    exposures: &ModelExposureStore,
    providers: &ProviderStore,
    jobs: &ModelImageProbeJobs,
    provider_id: &str,
    model_id: &str,
) -> Result<ModelExposureSummary, String> {
    let context = exposures.probe_context(provider_id, model_id)?;
    // Include the revision and protocol so a new configuration never waits for
    // or reuses a previous configuration's answer. RAII also handles cancel.
    let key = serde_json::to_string(&(&context.provider_revision_id, model_id, &context.wire_api))
        .map_err(|_| "image_probe_key_failed".to_string())?;
    if !jobs
        .active
        .lock()
        .map_err(|_| "image_probe_lock_failed".to_string())?
        .insert(key.clone())
    {
        return Err("image_probe_in_progress".to_string());
    }
    let _job = ProbeJob { jobs, key };
    let _slot = jobs
        .slots
        .acquire()
        .await
        .map_err(|_| "image_probe_unavailable".to_string())?;
    // Recheck after the queue wait; neither credentials nor the DB lock are
    // retained while another probe is running.
    let current = exposures.probe_context(provider_id, model_id)?;
    if current.provider_revision_id != context.provider_revision_id
        || current.wire_api != context.wire_api
    {
        return exposures.summary(provider_id, model_id);
    }
    let upstream = providers.upstream_for_revision(&context.provider_revision_id)?;
    if upstream.provider_id != provider_id {
        return Err("provider_revision_mismatch".to_string());
    }
    let result = crate::model_image_probe::execute(
        &upstream.base_url,
        upstream.api_key.expose(),
        model_id,
        &context.wire_api,
    )
    .await;
    exposures.save_probe_result(&context, &result.state, result.error_code.as_deref())?;
    exposures.summary(provider_id, model_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelling_a_probe_releases_the_duplicate_guard() {
        let jobs = ModelImageProbeJobs::default();
        let key = "revision/model".to_string();
        assert!(jobs.active.lock().unwrap().insert(key.clone()));
        {
            let _job = ProbeJob {
                jobs: &jobs,
                key: key.clone(),
            };
            assert!(!jobs.active.lock().unwrap().insert(key.clone()));
        }
        assert!(jobs.active.lock().unwrap().insert(key));
        assert_eq!(jobs.slots.available_permits(), 2);
    }
}
