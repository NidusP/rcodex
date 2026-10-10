//! Gateway 运行时句柄、绑定和 sidecar 状态结构。
//!
//! 不包含转发逻辑。主进程客户端和 sidecar 进程共用这些类型。

use std::{
    collections::BTreeMap,
    process::Child,
    sync::{atomic::AtomicUsize, Arc},
};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Mutex, RwLock};
use zeroize::Zeroize;

use crate::{
    conversation_archive::ArchiveCapture,
    diagnostics::DiagnosticsStore,
    providers::TransportMode,
    routing::{GatewayTimeoutSnapshot, RouteSetRevision},
    secrets::ApiKey,
    usage::UsageStore,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedGatewayStatus {
    pub state: String,
    pub base_url: Option<String>,
    pub bind_host: String,
    pub port: Option<u16>,
    pub active_bindings: usize,
    #[serde(default)]
    pub in_flight_requests: usize,
}

/// The complete data-plane binding contract.  A binding identifies a caller
/// namespace and the immutable route snapshot it is allowed to use.  It does
/// not describe a process, PID, lifecycle state, or compatibility fallback;
/// those concerns belong to the launch/control plane or are derived from the
/// route snapshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayBinding {
    /// Kept as `instanceId` on the wire for old sidecars; new readers also
    /// accept `bindingId` while the protocol migration is staged.
    #[serde(rename = "instanceId", alias = "bindingId")]
    pub binding_id: String,
    pub route_set: RouteSetRevision,
    /// Kept as `instanceKeyHash` on the wire for old sidecars; the domain name
    /// is admission-oriented even though the legacy JSON name remains.
    #[serde(rename = "instanceKeyHash", alias = "admissionKeyHash")]
    pub admission_key_hash: String,
}

/// Launch-scoped upstream credential material. It is sent only over the
/// authenticated loopback control channel and is never persisted or logged.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedGatewayCredential {
    pub provider_id: String,
    pub provider_revision_id: String,
    pub base_url: String,
    pub provider_name: String,
    pub transport_mode: TransportMode,
    pub api_key: String,
}

impl Drop for ManagedGatewayCredential {
    fn drop(&mut self) {
        self.api_key.zeroize();
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GatewayBindingRegistration {
    #[serde(flatten)]
    pub(super) binding: GatewayBinding,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) credentials: Vec<ManagedGatewayCredential>,
}

pub(super) struct ManagedGatewayRuntime {
    pub(super) handle: Mutex<Option<ManagedGatewayHandle>>,
    pub(super) registry: Arc<RwLock<BTreeMap<String, Arc<BindingRuntime>>>>,
    pub(super) diagnostics: Arc<RwLock<Option<DiagnosticsStore>>>,
    pub(super) usage: Arc<RwLock<Option<UsageStore>>>,
    pub(super) archive: Arc<RwLock<Option<ArchiveCapture>>>,
    pub(super) archive_database: std::sync::RwLock<Option<crate::database::Database>>,
    pub(super) in_flight_requests: Arc<AtomicUsize>,
}

pub(super) struct ManagedGatewayHandle {
    pub(super) port: u16,
    pub(super) shutdown: oneshot::Sender<()>,
}

pub(super) struct BindingRuntime {
    /// Managed instance owning this request namespace.
    pub(super) binding_id: String,
    /// The registered Codex history root, never supplied by a model request.
    pub(super) archive_home: Option<std::path::PathBuf>,
    pub(super) admission_key_hash: String,
    pub(super) route_set: RouteSetRevision,
    pub(super) upstreams: BTreeMap<String, ProviderUpstreamRuntime>,
}

pub(super) struct ProviderUpstreamRuntime {
    pub(super) provider_id: String,
    pub(super) provider_revision_id: String,
    pub(super) base_url: String,
    pub(super) api_key: Option<Arc<ApiKey>>,
    pub(super) client: Client,
    pub(super) timeouts: GatewayTimeoutSnapshot,
}

#[derive(Clone)]
pub(super) struct ManagedGatewayAppState {
    pub(super) registry: Arc<RwLock<BTreeMap<String, Arc<BindingRuntime>>>>,
    pub(super) diagnostics: Arc<RwLock<Option<DiagnosticsStore>>>,
    pub(super) usage: Arc<RwLock<Option<UsageStore>>>,
    pub(super) archive: Arc<RwLock<Option<ArchiveCapture>>>,
    pub(super) in_flight_requests: Arc<AtomicUsize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SidecarReady {
    #[serde(default)]
    pub(super) protocol_version: u32,
    pub(super) pid: u32,
    pub(super) data_base_url: String,
    pub(super) control_base_url: String,
    pub(super) control_token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SidecarWireStatus {
    pub(super) state: String,
    pub(super) base_url: Option<String>,
    pub(super) bind_host: String,
    pub(super) port: Option<u16>,
    pub(super) active_bindings: usize,
    #[serde(default)]
    pub(super) in_flight_requests: usize,
}

impl From<SidecarWireStatus> for ManagedGatewayStatus {
    fn from(status: SidecarWireStatus) -> Self {
        ManagedGatewayStatus {
            state: if status.state == "serving" {
                "serving"
            } else {
                "stopped"
            }
            .to_string(),
            base_url: status.base_url,
            bind_host: status.bind_host,
            port: status.port,
            active_bindings: status.active_bindings,
            in_flight_requests: status.in_flight_requests,
        }
    }
}

#[derive(Clone)]
pub(super) struct SidecarEndpoint {
    pub(super) control_base_url: String,
    pub(super) token: String,
    pub(super) protocol_version: u32,
}

pub(super) struct ManagedGatewayClientState {
    pub(super) child: Option<Child>,
    pub(super) endpoint: Option<SidecarEndpoint>,
    pub(super) unavailable: bool,
}
