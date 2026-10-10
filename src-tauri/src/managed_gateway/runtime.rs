//! 进程内 Gateway runtime：绑定注册表和 serving 状态。
//!
//! 不 spawn sidecar 二进制（那是 `sidecar.rs` 客户端），不转换协议。
//! `run_sidecar` 和主进程客户端都通过这里登记/注销绑定。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex, RwLock};

use crate::conversation_archive::ArchiveCapture;
use crate::diagnostics::DiagnosticsStore;
use crate::usage::UsageStore;

use super::http::managed_gateway_router;
use super::lifecycle::{provider_runtime, validate_admission_key_hash, validate_isolated_binding};
use super::runtime_types::{
    BindingRuntime, GatewayBinding, ManagedGatewayAppState, ManagedGatewayCredential,
    ManagedGatewayHandle, ManagedGatewayRuntime, ManagedGatewayStatus, ProviderUpstreamRuntime,
};
use super::LOOPBACK_HOST;
use crate::providers::{GatewayUpstream, ProviderStore};

impl ManagedGatewayRuntime {
    pub fn new() -> Self {
        Self {
            handle: Mutex::new(None),
            registry: Arc::new(RwLock::new(BTreeMap::new())),
            diagnostics: Arc::new(RwLock::new(None)),
            usage: Arc::new(RwLock::new(None)),
            archive: Arc::new(RwLock::new(None)),
            archive_database: std::sync::RwLock::new(None),
            in_flight_requests: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn set_diagnostics_now(&self, store: DiagnosticsStore) {
        if let Ok(mut guard) = self.diagnostics.try_write() {
            *guard = Some(store);
        }
    }

    pub fn set_usage_now(&self, store: UsageStore) {
        if let Ok(mut guard) = self.usage.try_write() {
            *guard = Some(store);
        }
    }

    pub fn set_archive_now(&self, capture: ArchiveCapture) {
        if let Ok(mut guard) = self.archive.try_write() {
            *guard = Some(capture);
        }
    }

    pub fn set_archive_database_now(&self, database: crate::database::Database) {
        if let Ok(mut guard) = self.archive_database.write() {
            *guard = Some(database);
        }
    }

    #[cfg(test)]
    pub async fn ensure_serving(&self) -> Result<ManagedGatewayStatus, String> {
        self.ensure_serving_on_port(None).await
    }

    pub async fn ensure_serving_on_port(
        &self,
        requested_port: Option<u16>,
    ) -> Result<ManagedGatewayStatus, String> {
        let mut handle = self.handle.lock().await;
        if handle.is_none() {
            let listener = TcpListener::bind((LOOPBACK_HOST, requested_port.unwrap_or(0)))
                .await
                .map_err(|error| format!("managed_gateway_bind_failed:{error}"))?;
            let port = listener
                .local_addr()
                .map_err(|error| format!("managed_gateway_address_failed:{error}"))?
                .port();
            let router = managed_gateway_router(ManagedGatewayAppState {
                registry: self.registry.clone(),
                diagnostics: self.diagnostics.clone(),
                usage: self.usage.clone(),
                archive: self.archive.clone(),
                in_flight_requests: self.in_flight_requests.clone(),
            });
            let (shutdown, receiver) = oneshot::channel();
            tokio::spawn(async move {
                let _ = axum::serve(listener, router)
                    .with_graceful_shutdown(async {
                        let _ = receiver.await;
                    })
                    .await;
            });
            *handle = Some(ManagedGatewayHandle { port, shutdown });
        }
        drop(handle);
        Ok(self.status().await)
    }

    pub(super) async fn register_binding(
        &self,
        binding: GatewayBinding,
        provider_store: &ProviderStore,
    ) -> Result<(), String> {
        binding
            .route_set
            .validate_ready()
            .map_err(|error| error.to_string())?;
        validate_admission_key_hash(&binding.admission_key_hash)?;
        self.gateway_port().await?;
        validate_isolated_binding(&binding.route_set)?;

        // Keychain access is a synchronous OS call and can wait for a user
        // authorization prompt. Keep it off the async control-plane worker so
        // health/status requests remain responsive, and bound the launch
        // operation when the desktop session cannot authorize the item.
        let provider_store = provider_store.clone();
        let providers = binding.route_set.providers.clone();
        let upstreams = tokio::time::timeout(
            Duration::from_secs(15),
            tokio::task::spawn_blocking(move || {
                let mut upstreams = BTreeMap::new();
                for provider in &providers {
                    let upstream =
                        provider_store.upstream_for_revision(&provider.provider_revision_id)?;
                    if upstream.provider_id != provider.provider_id
                        || upstream.provider_revision_id != provider.provider_revision_id
                    {
                        return Err("provider_revision_mismatch".to_string());
                    }
                    upstreams.insert(
                        provider.provider_revision_id.clone(),
                        provider_runtime(upstream, provider.timeouts.clone())?,
                    );
                }
                Ok::<_, String>(upstreams)
            }),
        )
        .await
        .map_err(|_| "managed_gateway_credential_timeout".to_string())?
        .map_err(|_| "managed_gateway_credential_worker_failed".to_string())??;

        self.install_binding(binding, upstreams).await
    }

    pub(super) async fn register_binding_with_credentials(
        &self,
        binding: GatewayBinding,
        credentials: Vec<ManagedGatewayCredential>,
    ) -> Result<(), String> {
        binding
            .route_set
            .validate_ready()
            .map_err(|error| error.to_string())?;
        validate_admission_key_hash(&binding.admission_key_hash)?;
        self.gateway_port().await?;
        validate_isolated_binding(&binding.route_set)?;
        let credentials = credentials
            .into_iter()
            .map(|credential| (credential.provider_revision_id.clone(), credential))
            .collect::<BTreeMap<_, _>>();
        let mut upstreams = BTreeMap::new();
        for provider in &binding.route_set.providers {
            let credential = credentials
                .get(&provider.provider_revision_id)
                .ok_or_else(|| "provider_credential_missing".to_string())?;
            if credential.provider_id != provider.provider_id
                || credential.provider_revision_id != provider.provider_revision_id
            {
                return Err("provider_revision_mismatch".to_string());
            }
            let api_key = crate::secrets::validate_api_key(credential.api_key.clone())?;
            upstreams.insert(
                provider.provider_revision_id.clone(),
                provider_runtime(
                    GatewayUpstream {
                        provider_id: credential.provider_id.clone(),
                        provider_revision_id: credential.provider_revision_id.clone(),
                        base_url: credential.base_url.clone(),
                        api_key,
                        provider_name: credential.provider_name.clone(),
                        transport_mode: credential.transport_mode,
                    },
                    provider.timeouts.clone(),
                )?,
            );
        }
        self.install_binding(binding, upstreams).await
    }

    pub(super) async fn install_binding(
        &self,
        binding: GatewayBinding,
        upstreams: BTreeMap<String, ProviderUpstreamRuntime>,
    ) -> Result<(), String> {
        let archive_home = self
            .archive_database
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().cloned())
            .and_then(|database| {
                database.connection().lock().ok().and_then(|connection| {
                    connection
                        .query_row(
                            "SELECT managed_codex_home FROM codex_instances WHERE id=?1",
                            [&binding.binding_id],
                            |row| row.get::<_, String>(0),
                        )
                        .ok()
                        .map(PathBuf::from)
                })
            });
        let binding_id = binding.binding_id.clone();
        let runtime = Arc::new(BindingRuntime {
            binding_id: binding.binding_id,
            archive_home,
            admission_key_hash: binding.admission_key_hash,
            route_set: binding.route_set,
            upstreams,
        });
        let mut registry = self.registry.write().await;
        if registry.contains_key(&binding_id) {
            return Err("binding_exists".to_string());
        }
        registry.insert(binding_id, runtime);
        Ok(())
    }

    pub(super) async fn gateway_port(&self) -> Result<u16, String> {
        self.handle
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| "managed_gateway_not_serving".to_string())
            .map(|handle| handle.port)
    }

    pub async fn unregister_binding(&self, binding_id: &str) -> bool {
        self.registry.write().await.remove(binding_id).is_some()
    }

    pub async fn stop_if_unused(&self) -> Result<ManagedGatewayStatus, String> {
        if !self.registry.read().await.is_empty() {
            return Err("configuration_in_use".to_string());
        }
        if let Some(handle) = self.handle.lock().await.take() {
            let _ = handle.shutdown.send(());
        }
        Ok(stopped_status())
    }

    pub async fn status(&self) -> ManagedGatewayStatus {
        let active_bindings = self.registry.read().await.len();
        match self.handle.lock().await.as_ref() {
            Some(handle) => ManagedGatewayStatus {
                state: "serving".to_string(),
                base_url: Some(format!("http://{LOOPBACK_HOST}:{}/v1", handle.port)),
                bind_host: LOOPBACK_HOST.to_string(),
                port: Some(handle.port),
                active_bindings,
                in_flight_requests: self.in_flight_requests.load(Ordering::Acquire),
            },
            None => ManagedGatewayStatus {
                active_bindings,
                ..stopped_status()
            },
        }
    }
}

pub(super) fn stopped_status() -> ManagedGatewayStatus {
    ManagedGatewayStatus {
        state: "stopped".to_string(),
        base_url: None,
        bind_host: LOOPBACK_HOST.to_string(),
        port: None,
        active_bindings: 0,
        in_flight_requests: 0,
    }
}

pub(super) fn unavailable_status() -> ManagedGatewayStatus {
    ManagedGatewayStatus {
        state: "unavailable".to_string(),
        ..stopped_status()
    }
}
