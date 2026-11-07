//! Permanent workspace deletion orchestrates only the selected preset, its
//! instance bindings, owned storage and archive scopes. Domain deletion and
//! path checks remain in ManagedLaunchStore.

use tauri::Emitter;

use crate::{
    codex_control::CodexControlStore,
    conversation_archive::ConversationArchive,
    managed_gateway::ManagedGatewayClient,
    managed_launch::{ManagedLaunchStore, WorkspaceDeletionPlan},
};

use super::CODEX_LAUNCH_IMPORT_LOCK;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteManagedWorkspaceInput {
    launch_preset_id: String,
    expected_version: i64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteManagedWorkspaceResult {
    deleted_preset_id: String,
    selected_preset_id: Option<String>,
}

struct DeletionLease<'a> {
    store: &'a ManagedLaunchStore,
    plan: &'a WorkspaceDeletionPlan,
}

impl Drop for DeletionLease<'_> {
    fn drop(&mut self) {
        if self.store.release_workspace_deletion(self.plan).is_err() {
            eprintln!("workspace deletion lease cleanup failed");
        }
    }
}

#[tauri::command]
pub(crate) async fn delete_managed_workspace(
    app: tauri::AppHandle,
    input: DeleteManagedWorkspaceInput,
    control: tauri::State<'_, CodexControlStore>,
    managed_launch: tauri::State<'_, ManagedLaunchStore>,
    gateway: tauri::State<'_, ManagedGatewayClient>,
    archive: tauri::State<'_, ConversationArchive>,
) -> Result<DeleteManagedWorkspaceResult, String> {
    let _operation = CODEX_LAUNCH_IMPORT_LOCK
        .try_lock()
        .map_err(|_| "resource_operation_in_progress".to_string())?;
    let plan = managed_launch
        .prepare_workspace_deletion(&input.launch_preset_id, input.expected_version)?;
    let _lease = DeletionLease {
        store: &managed_launch,
        plan: &plan,
    };
    let result = async {
        // All instances and paths were preflighted before the first mutation.
        // No implicit stop: a space with any active instance is rejected.
        for instance_id in &plan.instance_ids {
            control.ensure_instance_deletable(instance_id)?;
            match gateway.unregister_binding(instance_id).await {
                Ok(()) => {}
                Err(error) if error == "managed_gateway_not_serving" => {}
                Err(error) => {
                    return Err(format!("managed_gateway_binding_cleanup_failed:{error}"))
                }
            }
            managed_launch.revoke_binding_for_delete(instance_id)?;
        }
        // Current archives use preset ID as scope; include legacy instance
        // scopes too. Tombstones prevent a late writer from recreating them.
        let mut scopes = vec![plan.launch_preset_id.clone()];
        scopes.extend(plan.instance_ids.iter().cloned());
        archive.delete_scopes(&scopes)?;
        managed_launch.delete_workspace_storage(&plan)?;
        let selected_preset_id = managed_launch.finish_workspace_deletion(&plan)?;
        Ok(DeleteManagedWorkspaceResult {
            deleted_preset_id: plan.launch_preset_id.clone(),
            selected_preset_id,
        })
    }
    .await;
    let _ = app.emit("codex-control-snapshot-invalidated", ());
    result
}
