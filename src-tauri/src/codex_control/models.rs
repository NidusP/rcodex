//! 控制面序列化结构：快照、预设输入、实例摘要和凭据种类。
//!
//! 不含 SQLite 读写。IPC 和 Store 共用这些类型。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexControlSnapshot {
    pub accounts: Vec<CodexAccountSummary>,
    pub providers: Vec<CodexProviderSummary>,
    pub presets: Vec<LaunchPresetSummary>,
    pub instances: Vec<CodexInstanceSummary>,
    pub compatibility: CodexControlCompatibility,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexControlCompatibility {
    pub control_plane_available: bool,
    pub official_accounts_available: bool,
    pub managed_app_available: bool,
    pub official_hybrid_available: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountSummary {
    pub id: String,
    pub masked_identity: Option<String>,
    pub workspace_label: Option<String>,
    pub plan_label: Option<String>,
    pub current_credential_revision_id: Option<String>,
    pub auth_state: Option<String>,
    pub auth_checked_at: Option<i64>,
    pub auth_error_code: Option<String>,
    pub codex_version: Option<String>,
    pub schema_fingerprint: Option<String>,
    pub quota: Option<CodexQuotaSummary>,
    pub locked_by_instances: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexQuotaSummary {
    pub primary_used_percent: Option<f64>,
    pub primary_window_minutes: Option<i64>,
    pub primary_resets_at: Option<i64>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_window_minutes: Option<i64>,
    pub secondary_resets_at: Option<i64>,
    pub refreshed_at: i64,
    pub expires_at: i64,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderSummary {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub is_default: bool,
    pub configured: bool,
    pub transport_mode: String,
    pub current_revision_id: String,
    pub locked_by_instances: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPresetSummary {
    pub id: String,
    pub entry_kind: Option<String>,
    pub name: String,
    pub credential_kind: String,
    pub api_key_provider_id: Option<String>,
    pub codex_account_id: Option<String>,
    pub gateway_policy: Option<String>,
    pub default_workspace_path: Option<String>,
    pub version: i64,
    pub is_preferred: bool,
    pub locked_by_instances: Vec<String>,
    /// Channel used as the isolated Codex API login. None means unset.
    pub login_provider_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexInstanceSummary {
    pub id: String,
    pub launch_configuration_revision_id: String,
    pub launch_preset_id: Option<String>,
    /// The only active values are `managed_app` and `cli`.
    pub launch_target: Option<String>,
    pub credential_kind: String,
    pub credential_revision_id: Option<String>,
    pub workspace_path: Option<String>,
    pub state: String,
    pub supervisor_pid: Option<u32>,
    pub pid: Option<u32>,
    pub launched_at: Option<i64>,
    pub exited_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub startup_log: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchCodexInput {
    pub launch_preset_id: String,
    pub expected_version: i64,
    pub workspace_path: Option<String>,
    #[serde(default)]
    pub launch_target: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLaunchPresetInput {
    #[serde(default)]
    pub entry_kind: Option<String>,
    pub name: String,
    pub credential_kind: String,
    pub api_key_provider_id: Option<String>,
    pub codex_account_id: Option<String>,
    pub gateway_policy: Option<String>,
    pub default_workspace_path: Option<String>,
    #[serde(default)]
    pub is_preferred: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLaunchPresetInput {
    pub expected_version: i64,
    pub name: String,
    pub default_workspace_path: Option<String>,
    #[serde(default)]
    pub is_preferred: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameLaunchPresetInput {
    pub expected_version: i64,
    pub name: String,
}

#[derive(Debug)]
pub struct PreparedOfficialLaunch {
    pub instance_id: String,
    pub launch_nonce: String,
    pub managed_codex_home: PathBuf,
    pub status_file: PathBuf,
    pub workspace_path: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    ManagedApp,
}

impl EntryKind {
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::ManagedApp => "managed_app",
        }
    }

    pub fn is_managed_workspace(value: Option<&str>) -> bool {
        value == Some(Self::ManagedApp.as_wire())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialKind {
    ApiKey,
    Official,
    None,
    ManagedRouteSet,
}

impl CredentialKind {
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Official => "official",
            Self::None => "none",
            Self::ManagedRouteSet => "managed_route_set",
        }
    }

    pub fn wire_is_api_key(value: &str) -> bool {
        value == Self::ApiKey.as_wire()
    }

    pub fn wire_is_official(value: &str) -> bool {
        value == Self::Official.as_wire()
    }

    pub fn wire_is_managed_route_set(value: &str) -> bool {
        value == Self::ManagedRouteSet.as_wire()
    }
}

pub struct InstanceState;

impl InstanceState {
    pub fn is_active(value: &str) -> bool {
        matches!(
            value,
            "launching" | "running" | "stopping" | "orphaned" | "degraded"
        )
    }

    pub fn is_finished(value: &str) -> bool {
        matches!(value, "exited" | "failed")
    }
}

#[cfg(test)]
mod product_kind_tests {
    use super::{CredentialKind, EntryKind, InstanceState};

    #[test]
    fn workspace_entry_and_instance_state_keep_their_wire_meaning() {
        assert!(EntryKind::is_managed_workspace(Some("managed_app")));
        assert!(!EntryKind::is_managed_workspace(Some("cli")));
        assert!(CredentialKind::wire_is_api_key("api_key"));
        assert!(CredentialKind::wire_is_official("official"));
        assert!(CredentialKind::wire_is_managed_route_set(
            "managed_route_set"
        ));
        assert!(InstanceState::is_active("running"));
        assert!(InstanceState::is_finished("exited"));
        assert!(!InstanceState::is_finished("orphaned"));
        assert!(!InstanceState::is_active("unknown"));
    }
}
