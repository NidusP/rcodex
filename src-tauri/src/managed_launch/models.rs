//! 受管理启动方案的公开结构。
//!
//! 只放序列化类型和 Store 句柄字段。行为方法在 workspace / prepare / home 等模块。

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{
    route_store::RouteStore,
    routing::{GatewayTimeoutSnapshot, RouteKind, RouteSetRevision, WireApi},
    secrets::InstanceGatewayKey,
};

#[derive(Clone)]
pub struct ManagedLaunchStore {
    pub(super) connection: Arc<Mutex<Connection>>,
    pub(super) route_store: RouteStore,
    pub(super) instances_root: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateManagedLaunchPresetInput {
    #[serde(default)]
    pub entry_kind: Option<String>,
    pub name: String,
    pub official_account_id: Option<String>,
    #[serde(default)]
    pub api_key_provider_ids: Vec<String>,
    pub models: Vec<ManagedModelSelectionInput>,
    #[serde(default)]
    pub auto_review_public_model_id: Option<String>,
    /// Launch-scoped transport policy. Optional for older clients.
    #[serde(default)]
    pub transport_mode: Option<String>,
    pub default_workspace_path: Option<String>,
    /// Insert a new managed_app preset instead of updating the preferred one.
    #[serde(default)]
    pub create_new: bool,
    /// Update this preset. Empty falls back to the preferred managed_app row.
    #[serde(default)]
    pub launch_preset_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelSelectionInput {
    pub source_kind: RouteKind,
    pub source_id: String,
    pub upstream_model_id: String,
    pub public_model_id: String,
    pub display_name: String,
    /// Workspace override. Empty means the family default.
    #[serde(default)]
    pub context_window: Option<u64>,
    /// Workspace override. Empty means 95% of the resolved window, or the family value.
    #[serde(default)]
    pub auto_compact_token_limit: Option<u64>,
    /// Per Provider-model verified effort override. Empty uses the family profile.
    #[serde(default)]
    pub reasoning_levels: Vec<String>,
    #[serde(default)]
    pub default_reasoning_level: Option<String>,
}

impl Default for ManagedModelSelectionInput {
    fn default() -> Self {
        Self {
            source_kind: RouteKind::ApiProvider,
            source_id: String::new(),
            upstream_model_id: String::new(),
            public_model_id: String::new(),
            display_name: String::new(),
            context_window: None,
            auto_compact_token_limit: None,
            reasoning_levels: Vec::new(),
            default_reasoning_level: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedPresetSaved {
    pub id: String,
    pub version: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedPresetCompositionSummary {
    pub launch_preset_id: String,
    pub official_account_id: Option<String>,
    pub api_key_provider_ids: Vec<String>,
    pub models: Vec<ManagedPresetModelSummary>,
    pub auto_review_public_model_id: Option<String>,
    pub state: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedPresetModelSummary {
    pub route_identity_id: String,
    pub source_kind: RouteKind,
    pub source_id: String,
    pub upstream_model_id: String,
    pub public_model_id: String,
    pub display_name: String,
    pub context_window: Option<u64>,
    pub auto_compact_token_limit: Option<u64>,
    pub reasoning_levels: Vec<String>,
    pub default_reasoning_level: Option<String>,
}

pub struct PreparedManagedLaunch {
    pub instance_id: String,
    pub launch_nonce: String,
    /// The managed App's SQLite home, always equal to `launch_codex_home`.
    /// Legacy `managed-app-sqlite/<preset>` data is migrated into the real
    /// instance home before this launch is persisted.
    pub sqlite_home: Option<PathBuf>,
    /// Home passed to the Codex process. The macOS managed App uses a short
    /// real directory because Unix-domain socket paths have a small limit;
    /// CLI launches may retain their compatible short alias.
    pub launch_codex_home: PathBuf,
    /// Persistent isolated Codex home. Never the user's default ~/.codex.
    pub managed_codex_home: PathBuf,
    /// Workspace channel used as this instance's API login. None means the
    /// launch path still has to resolve a single-provider default.
    pub login_provider_id: Option<String>,
    pub electron_user_data_dir: Option<PathBuf>,
    pub status_file: PathBuf,
    pub workspace_path: Option<String>,
    pub instance_key_ref: Option<String>,
    pub instance_key_hash: Option<String>,
    /// Launch-scoped copy of the newly generated Gateway admission key.
    /// Keeping it in the preparation result avoids a second synchronous
    /// credential read immediately before spawning Codex. SQLite stores only
    /// the hash; the plaintext exists only for this launch and its one-shot
    /// handoff pipe.
    pub instance_key: Option<InstanceGatewayKey>,
    pub profile_name: String,
    pub filtered_mcp_servers: Vec<String>,
    pub route_set: RouteSetRevision,
    pub transport_mode: ManagedLaunchTransport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedLaunchTransport {
    Gateway,
}

pub(super) struct ProviderSnapshot {
    pub(super) provider_id: String,
    pub(super) name: String,
    pub(super) revision_id: String,
    pub(super) wire_api: WireApi,
    #[allow(dead_code)]
    pub(super) base_url: String,
    #[allow(dead_code)]
    pub(super) secret_ref: String,
    pub(super) timeouts: GatewayTimeoutSnapshot,
    pub(super) position: u32,
}
