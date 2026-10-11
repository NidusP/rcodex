//! 主进程控制面客户端：拉起/探活/停止独立 Gateway sidecar。
//!
//! Codex 请求不走这里。只管理 sidecar 生命周期和 control token。

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

#[cfg(test)]
use std::cell::Cell;

#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::sleep;
use uuid::Uuid;

use super::gateway_listen_port;
use super::runtime_types::{
    GatewayBinding, GatewayBindingRegistration, ManagedGatewayClientState,
    ManagedGatewayCredential, ManagedGatewayStatus, SidecarEndpoint, SidecarReady,
    SidecarWireStatus,
};

/// Main-process control-plane client for the independent Gateway sidecar.
/// Codex requests never use this client; it only manages sidecar lifecycle.
pub struct ManagedGatewayClient {
    app_data_dir: PathBuf,
    database_path: PathBuf,
    state: Mutex<ManagedGatewayClientState>,
    start_lock: Mutex<()>,
    retirement_lock: Mutex<()>,
    http: Client,
}

impl ManagedGatewayClient {
    pub fn new(app_data_dir: PathBuf, database_path: PathBuf) -> Result<Self, String> {
        let ready_path = app_data_dir.join("gateway-sidecar.json");
        let endpoint = read_sidecar_ready(&ready_path).map(|ready| SidecarEndpoint {
            control_base_url: ready.control_base_url,
            token: ready.control_token,
            protocol_version: ready.protocol_version,
        });
        let http = Client::builder()
            // The control plane is always a 127.0.0.1 sidecar. Letting a
            // desktop proxy intercept it can turn a healthy local response
            // into an unrelated HTML/error page and falsely report the
            // Gateway as unavailable.
            .no_proxy()
            .connect_timeout(Duration::from_secs(1))
            .build()
            .map_err(|error| format!("managed_gateway_control_client_failed:{error}"))?;
        Ok(Self {
            app_data_dir,
            database_path,
            state: Mutex::new(ManagedGatewayClientState {
                child: None,
                endpoint,
                unavailable: false,
            }),
            start_lock: Mutex::new(()),
            retirement_lock: Mutex::new(()),
            http,
        })
    }

    pub async fn ensure_serving(&self) -> Result<ManagedGatewayStatus, String> {
        self.retire_legacy_bindings().await?;
        if let Some(status) = self.serving_status().await {
            return Ok(status);
        }
        let _start_guard = self.start_lock.lock().await;
        if let Some(status) = self.serving_status().await {
            return Ok(status);
        }
        self.start_sidecar().await?;
        self.request_status().await
    }

    pub async fn status(&self) -> Result<ManagedGatewayStatus, String> {
        self.retire_legacy_bindings().await?;
        {
            let missing = self.state.lock().await.endpoint.is_none();
            if missing {
                // tokio Mutex 不可重入：先记下 missing 再释放，才能调用 adopt。
                self.adopt_ready_file().await;
            }
        }
        let state = self.state.lock().await;
        let has_endpoint = state.endpoint.is_some();
        drop(state);
        if !has_endpoint {
            // 控制令牌只写在 ready 文件里。文件丢失时无法探活控制面，
            // 但隔离 CLI/App 仍可能经 127.0.0.1:43123 转发。
            // 数据口占用时显示 unavailable，避免把仍在服务的 sidecar 报成已停止。
            if gateway_data_port_open() {
                return Ok(super::runtime::unavailable_status());
            }
            return Ok(super::runtime::stopped_status());
        }
        match self.request_status().await {
            Ok(status) => {
                self.state.lock().await.unavailable = false;
                Ok(status)
            }
            Err(error) if error == "managed_gateway_unavailable" => {
                let data_port_open = gateway_data_port_open();
                let mut state = self.state.lock().await;
                state.endpoint = None;
                state.unavailable = data_port_open;
                if data_port_open {
                    Ok(super::runtime::unavailable_status())
                } else {
                    Ok(super::runtime::stopped_status())
                }
            }
            Err(error) => Err(error),
        }
    }

    pub async fn register_binding(&self, binding: GatewayBinding) -> Result<(), String> {
        self.ensure_serving().await?;
        super::lifecycle::validate_isolated_binding(&binding.route_set)?;
        self.request_json(
            "/control/register",
            &GatewayBindingRegistration {
                binding,
                credentials: Vec::new(),
            },
        )
        .await
    }

    pub async fn register_binding_with_credentials(
        &self,
        binding: GatewayBinding,
        credentials: Vec<ManagedGatewayCredential>,
    ) -> Result<(), String> {
        self.ensure_serving().await?;
        super::lifecycle::validate_isolated_binding(&binding.route_set)?;
        self.request_json(
            "/control/register",
            &GatewayBindingRegistration {
                binding,
                credentials,
            },
        )
        .await
    }

    pub async fn unregister_binding(&self, binding_id: &str) -> Result<(), String> {
        let _ = self
            .request_json(
                "/control/unregister",
                // Keep the legacy JSON key while old sidecars can still be
                // discovered and reused by the desktop process.
                &serde_json::json!({ "instanceId": binding_id }),
            )
            .await?;
        Ok(())
    }

    pub async fn stop_if_unused(&self) -> Result<ManagedGatewayStatus, String> {
        if self.state.lock().await.endpoint.is_none() {
            return Ok(super::runtime::stopped_status());
        }
        let status = self
            .request_json_value("/control/stop", &serde_json::json!({}))
            .await?;
        let status = serde_json::from_value::<SidecarWireStatus>(status)
            .map_err(|error| format!("managed_gateway_status_decode_failed:{error}"))?;
        if status.state == "stopped" {
            let mut state = self.state.lock().await;
            state.endpoint = None;
            state.unavailable = false;
            if let Some(mut child) = state.child.take() {
                let _ = child.wait();
            }
            let _ = fs::remove_file(self.app_data_dir.join("gateway-sidecar.json"));
        }
        Ok(status.into())
    }

    /// Remove an orphaned R Codex sidecar after the control endpoint is lost.
    ///
    /// This path is deliberately manual and fail-closed: it only operates on
    /// an unavailable Gateway and only after every listener on the managed
    /// data port is identified as a `codex-gateway` process.
    pub async fn cleanup_unavailable(&self) -> Result<ManagedGatewayStatus, String> {
        let _start_guard = self.start_lock.lock().await;
        let status = self.status().await?;
        if status.state != "unavailable" {
            return Ok(status);
        }

        let pids = gateway_listener_pids()?;
        if pids.is_empty() {
            if !gateway_data_port_open() {
                let mut state = self.state.lock().await;
                state.endpoint = None;
                state.unavailable = false;
                let _ = fs::remove_file(self.app_data_dir.join("gateway-sidecar.json"));
                return Ok(super::runtime::stopped_status());
            }
            return Err("managed_gateway_cleanup_process_not_found".to_string());
        }
        for pid in &pids {
            let command = gateway_process_command(*pid)
                .ok_or_else(|| "managed_gateway_cleanup_process_not_found".to_string())?;
            if !is_codex_gateway_command(&command) {
                return Err("managed_gateway_cleanup_foreign_process".to_string());
            }
        }

        for pid in &pids {
            terminate_gateway_process(*pid, false)?;
        }
        for _ in 0..20 {
            if !gateway_data_port_open() {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
        if gateway_data_port_open() {
            let remaining_pids = gateway_listener_pids()?;
            for pid in &remaining_pids {
                let command = gateway_process_command(*pid)
                    .ok_or_else(|| "managed_gateway_cleanup_process_not_found".to_string())?;
                if !is_codex_gateway_command(&command) {
                    return Err("managed_gateway_cleanup_foreign_process".to_string());
                }
                let _ = terminate_gateway_process(*pid, true);
            }
            for _ in 0..20 {
                if !gateway_data_port_open() {
                    break;
                }
                sleep(Duration::from_millis(100)).await;
            }
        }
        if gateway_data_port_open() {
            return Err("managed_gateway_cleanup_failed".to_string());
        }

        let mut state = self.state.lock().await;
        state.endpoint = None;
        state.unavailable = false;
        if let Some(mut child) = state.child.take() {
            let _ = child.wait();
        }
        let _ = fs::remove_file(self.app_data_dir.join("gateway-sidecar.json"));
        Ok(super::runtime::stopped_status())
    }

    /// Kill a codex-gateway that still owns the data port after its control
    /// token file disappeared, then start a replacement sidecar. The new
    /// process restores active bindings from SQLite.
    pub async fn restart_unavailable(&self) -> Result<ManagedGatewayStatus, String> {
        self.cleanup_unavailable().await?;
        self.ensure_serving().await
    }

    pub async fn set_archive_policy(&self, enabled: bool) -> Result<(), String> {
        self.ensure_serving().await?;
        self.request_json(
            "/control/archive-policy",
            &serde_json::json!({ "enabled": enabled }),
        )
        .await
    }

    async fn serving_status(&self) -> Option<ManagedGatewayStatus> {
        {
            let missing = self.state.lock().await.endpoint.is_none();
            if missing {
                // tokio Mutex 不可重入：先记下 missing 再释放，才能调用 adopt。
                self.adopt_ready_file().await;
            }
        }
        let status = self.try_status().await?;
        (status.state == "serving").then_some(status)
    }

    async fn adopt_ready_file(&self) -> bool {
        let Some(ready) = read_sidecar_ready(&self.app_data_dir.join("gateway-sidecar.json"))
        else {
            return false;
        };
        let mut state = self.state.lock().await;
        state.endpoint = Some(SidecarEndpoint {
            control_base_url: ready.control_base_url,
            token: ready.control_token,
            protocol_version: ready.protocol_version,
        });
        state.unavailable = false;
        true
    }

    async fn try_status(&self) -> Option<ManagedGatewayStatus> {
        self.request_status().await.ok()
    }

    async fn retire_legacy_bindings(&self) -> Result<(), String> {
        let _retirement = self.retirement_lock.lock().await;
        let is_legacy = self
            .state
            .lock()
            .await
            .endpoint
            .as_ref()
            .is_some_and(|endpoint| endpoint.protocol_version < super::SIDECAR_PROTOCOL_VERSION);
        if !is_legacy {
            return Ok(());
        }
        match self.request_status().await {
            Ok(_) => {}
            Err(error)
                if error == "managed_gateway_unavailable"
                    || error == "managed_gateway_not_serving" =>
            {
                self.state.lock().await.endpoint = None;
                return Ok(());
            }
            Err(_) => return Err("managed_gateway_legacy_retirement_failed".to_string()),
        }
        let mut binding_ids = legacy_binding_ids(&self.database_path)?;
        binding_ids.push(crate::secrets::DEFAULT_APP_COMPATIBILITY_ID.to_string());
        binding_ids.sort();
        binding_ids.dedup();
        for binding_id in binding_ids {
            self.request_json(
                "/control/unregister",
                &serde_json::json!({ "instanceId": binding_id }),
            )
            .await
            .map_err(|_| "managed_gateway_legacy_retirement_failed".to_string())?;
        }
        if self.request_status().await?.active_bindings == 0 {
            self.stop_if_unused().await?;
            return Ok(());
        }
        if let Some(endpoint) = self.state.lock().await.endpoint.as_mut() {
            // A pre-v2 sidecar may still serve independent running instances.
            // It is safe to reuse only after every legacy binding was removed.
            endpoint.protocol_version = super::SIDECAR_PROTOCOL_VERSION;
        }
        Ok(())
    }

    async fn request_status(&self) -> Result<ManagedGatewayStatus, String> {
        let value = self
            .request_json_value("/control/status", &serde_json::json!({}))
            .await?;
        let status = serde_json::from_value::<SidecarWireStatus>(value)
            .map_err(|error| format!("managed_gateway_status_decode_failed:{error}"))?;
        Ok(status.into())
    }

    async fn request_json<T: Serialize>(&self, path: &str, body: &T) -> Result<(), String> {
        let _ = self.request_json_value(path, body).await?;
        Ok(())
    }

    async fn request_json_value<T: Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<Value, String> {
        let endpoint = self
            .state
            .lock()
            .await
            .endpoint
            .clone()
            .ok_or_else(|| "managed_gateway_not_serving".to_string())?;
        let response = self
            .http
            .post(format!("{}{}", endpoint.control_base_url, path))
            .header("x-rcodex-control-token", endpoint.token)
            .json(body)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map_err(|_| "managed_gateway_unavailable".to_string())?;
        let status = response.status();
        let value = response
            .json::<Value>()
            .await
            .map_err(|_| "managed_gateway_invalid_response".to_string())?;
        if !status.is_success() {
            return Err(value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("managed_gateway_control_failed")
                .to_string());
        }
        Ok(value)
    }

    async fn start_sidecar(&self) -> Result<(), String> {
        if self.serving_status().await.is_some() {
            return Ok(());
        }
        // 数据口已被占用时不得删 ready 文件、不得再 spawn。
        // 旧逻辑会把 gateway-sidecar.json 删掉，新进程 bind 失败，5 秒后只杀掉新 child，
        // 旧 sidecar 继续给隔离 CLI 转发，控制面却报 stopped / start_timeout。
        if gateway_data_port_open() {
            if self.adopt_ready_file().await && self.serving_status().await.is_some() {
                return Ok(());
            }
            return Err("managed_gateway_port_in_use".to_string());
        }
        let ready_path = self.app_data_dir.join("gateway-sidecar.json");
        let _ = fs::remove_file(&ready_path);
        let token = format!("3rctl_{}", Uuid::new_v4().simple());
        let executable = find_sidecar_executable()?;
        let mut command = Command::new(executable);
        command
            .arg("--database-path")
            .arg(&self.database_path)
            .arg("--ready-file")
            .arg(&ready_path)
            .arg("--control-token")
            .arg(&token)
            .arg("--data-port")
            .arg(gateway_listen_port().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        // Sidecar used to discard stderr, so Codex's loopback 502 had no
        // local trace of the real upstream. Keep a bounded append-only log
        // next to the ready file; the child never writes secrets.
        command.stderr(sidecar_stderr(&self.app_data_dir));
        detach_sidecar_process(&mut command);
        let child = command
            .spawn()
            .map_err(|error| format!("managed_gateway_sidecar_spawn_failed:{error}"))?;
        {
            let mut state = self.state.lock().await;
            state.child = Some(child);
            state.endpoint = None;
        }
        for _ in 0..100 {
            if let Some(ready) = read_sidecar_ready(&ready_path) {
                let mut state = self.state.lock().await;
                state.endpoint = Some(SidecarEndpoint {
                    control_base_url: ready.control_base_url,
                    token: ready.control_token,
                    protocol_version: ready.protocol_version,
                });
                state.unavailable = false;
                return Ok(());
            }
            sleep(Duration::from_millis(50)).await;
        }
        if let Some(mut child) = self.state.lock().await.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Err("managed_gateway_sidecar_start_timeout".to_string())
    }
}

#[cfg(test)]
thread_local! {
    static TEST_DATA_PORT_OPEN: Cell<Option<bool>> = const { Cell::new(None) };
}

fn gateway_data_port_open() -> bool {
    #[cfg(test)]
    if let Some(forced) = TEST_DATA_PORT_OPEN.with(Cell::get) {
        return forced;
    }
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], gateway_listen_port())),
        Duration::from_millis(150),
    )
    .is_ok()
}

fn gateway_listener_pids() -> Result<Vec<u32>, String> {
    #[cfg(unix)]
    {
        let output = Command::new("lsof")
            .args([
                "-nP",
                "-a",
                "-t",
                &format!("-iTCP:{}", gateway_listen_port()),
                "-sTCP:LISTEN",
            ])
            .output()
            .map_err(|_| "managed_gateway_cleanup_process_lookup_failed".to_string())?;
        if !output.status.success() && output.stdout.is_empty() {
            return Ok(Vec::new());
        }
        let mut pids = output
            .stdout
            .split(|byte| *byte == b'\n' || *byte == b'\r')
            .filter_map(|line| std::str::from_utf8(line).ok()?.trim().parse::<u32>().ok())
            .collect::<Vec<_>>();
        pids.sort_unstable();
        pids.dedup();
        return Ok(pids);
    }
    #[cfg(windows)]
    {
        let output = Command::new("netstat")
            .args(["-ano", "-p", "tcp"])
            .output()
            .map_err(|_| "managed_gateway_cleanup_process_lookup_failed".to_string())?;
        let port = format!(":{}", gateway_listen_port());
        let mut pids = output
            .stdout
            .split(|byte| *byte == b'\n' || *byte == b'\r')
            .filter_map(|line| {
                let line = std::str::from_utf8(line)
                    .ok()?
                    .split_whitespace()
                    .collect::<Vec<_>>();
                (line.len() >= 5
                    && line[1].ends_with(&port)
                    && line[3].eq_ignore_ascii_case("LISTENING"))
                .then(|| line[4].parse::<u32>().ok())
                .flatten()
            })
            .collect::<Vec<_>>();
        pids.sort_unstable();
        pids.dedup();
        return Ok(pids);
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err("managed_gateway_cleanup_unsupported_platform".to_string())
    }
}

fn gateway_process_command(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        let output = Command::new("ps")
            .args(["-ww", "-p", &pid.to_string(), "-o", "command="])
            .output()
            .ok()?;
        return output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .ok()?;
        return output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        None
    }
}

fn is_codex_gateway_command(command: &str) -> bool {
    #[cfg(windows)]
    let executable = command
        .split(',')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('"');
    #[cfg(not(windows))]
    // The packaged macOS application path can contain spaces. The sidecar
    // always receives long-form arguments, so split at the first argument
    // delimiter instead of whitespace inside the executable path.
    let executable = command.split(" --").next().unwrap_or_default().trim();
    let name = Path::new(executable)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    name == "codex-gateway" || name.eq_ignore_ascii_case("codex-gateway.exe")
}

fn terminate_gateway_process(pid: u32, force: bool) -> Result<(), String> {
    #[cfg(unix)]
    {
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        // SAFETY: callers only pass a PID whose command was verified as the
        // R Codex Gateway immediately before this function is called.
        let result = unsafe { libc::kill(pid as libc::pid_t, signal) };
        if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err("managed_gateway_cleanup_failed".to_string());
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/T"]);
        if force {
            command.arg("/F");
        }
        let status = command
            .status()
            .map_err(|_| "managed_gateway_cleanup_failed".to_string())?;
        return status
            .success()
            .then_some(())
            .ok_or_else(|| "managed_gateway_cleanup_failed".to_string());
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, force);
        Err("managed_gateway_cleanup_unsupported_platform".to_string())
    }
}

fn detach_sidecar_process(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }
}

fn sidecar_stderr(app_data_dir: &Path) -> Stdio {
    let path = app_data_dir.join("gateway-sidecar.log");
    if fs::metadata(&path)
        .map(|metadata| metadata.len() > 2 * 1024 * 1024)
        .unwrap_or(false)
    {
        let _ = fs::rename(&path, app_data_dir.join("gateway-sidecar.log.old"));
    }
    match fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            #[cfg(unix)]
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            Stdio::from(file)
        }
        Err(_) => Stdio::null(),
    }
}

fn read_sidecar_ready(path: &Path) -> Option<SidecarReady> {
    fs::read_to_string(path)
        .ok()
        .and_then(|value| serde_json::from_str::<SidecarReady>(&value).ok())
}

fn legacy_binding_ids(database_path: &Path) -> Result<Vec<String>, String> {
    let connection = rusqlite::Connection::open_with_flags(
        database_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "managed_gateway_legacy_retirement_failed".to_string())?;
    let mut statement = connection.prepare(
        "SELECT compatibility_id FROM compatibility_gateway_bindings
         UNION
         SELECT instance.id FROM codex_instances instance
         JOIN launch_configuration_revisions configuration ON configuration.id=instance.launch_configuration_revision_id
         LEFT JOIN launch_presets preset ON preset.id=configuration.launch_preset_id
         WHERE COALESCE(json_extract(configuration.config_snapshot, '$.launchTarget'), preset.entry_kind)
           IN ('compat_app','compat_cli')",
    ).map_err(|_| "managed_gateway_legacy_retirement_failed".to_string())?;
    let result = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| "managed_gateway_legacy_retirement_failed".to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "managed_gateway_legacy_retirement_failed".to_string());
    result
}

fn find_sidecar_executable() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            candidates.push(parent.join("codex-gateway"));
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/codex-gateway"));
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/release/codex-gateway"));
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "managed_gateway_sidecar_missing".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use std::{collections::BTreeSet, sync::Arc};

    #[derive(Clone)]
    struct LegacyRuntime {
        bindings: Arc<Mutex<BTreeSet<String>>>,
        removed: Arc<Mutex<Vec<String>>>,
    }

    async fn mock_status(State(state): State<LegacyRuntime>) -> Json<Value> {
        Json(
            serde_json::json!({"state":"serving","baseUrl":"http://127.0.0.1:43123/v1",
            "bindHost":"127.0.0.1","port":43123,"activeBindings":state.bindings.lock().await.len()}),
        )
    }

    async fn mock_unregister(
        State(state): State<LegacyRuntime>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let binding_id = body["instanceId"].as_str().unwrap().to_string();
        state.bindings.lock().await.remove(&binding_id);
        state.removed.lock().await.push(binding_id);
        Json(serde_json::json!({"ok":true}))
    }

    async fn mock_stop() -> Json<Value> {
        Json(
            serde_json::json!({"state":"stopped","baseUrl":null,"bindHost":"127.0.0.1","port":null,"activeBindings":0}),
        )
    }

    async fn mock_serving_status() -> Json<Value> {
        Json(serde_json::json!({
            "state": "serving",
            "baseUrl": "http://127.0.0.1:43123/v1",
            "bindHost": "127.0.0.1",
            "port": 43123,
            "activeBindings": 1
        }))
    }

    struct ForcedDataPort;

    impl ForcedDataPort {
        fn open() -> Self {
            super::TEST_DATA_PORT_OPEN.with(|cell| cell.set(Some(true)));
            Self
        }

        fn closed() -> Self {
            super::TEST_DATA_PORT_OPEN.with(|cell| cell.set(Some(false)));
            Self
        }
    }

    impl Drop for ForcedDataPort {
        fn drop(&mut self) {
            super::TEST_DATA_PORT_OPEN.with(|cell| cell.set(None));
        }
    }

    fn temp_client() -> (PathBuf, crate::database::Database, ManagedGatewayClient) {
        let root = std::env::temp_dir().join(format!("3r-sidecar-status-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let database_path = root.join("control.sqlite");
        let database = crate::database::Database::open_path(&database_path).unwrap();
        let client = ManagedGatewayClient::new(root.clone(), database_path).unwrap();
        (root, database, client)
    }

    async fn bind_mock_control() -> Option<(tokio::net::TcpListener, std::net::SocketAddr)> {
        match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => {
                let address = listener.local_addr().unwrap();
                Some((listener, address))
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => None,
            Err(error) => panic!("bind mock control: {error}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn sidecar_process_uses_a_new_session() {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("ps -o sess= -p $$")
            .stdout(Stdio::piped());
        detach_sidecar_process(&mut command);
        let output = command.output().unwrap();
        if !output.status.success() {
            eprintln!(
                "skip sidecar_process_uses_a_new_session: status={:?} stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let child_session = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<libc::pid_t>()
            .unwrap();
        let parent_session = unsafe { libc::getsid(0) };
        assert_ne!(child_session, parent_session);
    }

    #[tokio::test]
    async fn upgrading_legacy_sidecar_retires_default_binding_without_stopping_managed_bindings() {
        // This test exercises the mock control endpoint, not whichever
        // sidecar a developer may currently have running on the fixed data
        // port. Keep its expected stopped state independent of that process.
        let _port = ForcedDataPort::closed();
        for keep_managed in [false, true] {
            let root = std::env::temp_dir().join(format!("3r-sidecar-upgrade-{}", Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let database_path = root.join("control.sqlite");
            let database = crate::database::Database::open_path(&database_path).unwrap();
            let legacy_id = crate::secrets::DEFAULT_APP_COMPATIBILITY_ID.to_string();
            let state = LegacyRuntime {
                bindings: Arc::new(Mutex::new([legacy_id.clone()].into_iter().collect())),
                removed: Arc::new(Mutex::new(Vec::new())),
            };
            if keep_managed {
                state
                    .bindings
                    .lock()
                    .await
                    .insert("managed-instance".to_string());
            }
            let Some((listener, address)) = bind_mock_control().await else {
                eprintln!(
                    "skip upgrading_legacy_sidecar_retires_default_binding_without_stopping_managed_bindings: loopback bind denied"
                );
                continue;
            };
            let router = Router::new()
                .route("/control/status", post(mock_status))
                .route("/control/unregister", post(mock_unregister))
                .route("/control/stop", post(mock_stop))
                .with_state(state.clone());
            let server = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            // A ready file from the previous version deliberately has no protocolVersion.
            fs::write(
                root.join("gateway-sidecar.json"),
                serde_json::json!({
                    "pid":1,"dataBaseUrl":"http://127.0.0.1:43123/v1",
                    "controlBaseUrl":format!("http://{address}"),"controlToken":"fixture"
                })
                .to_string(),
            )
            .unwrap();
            let client = ManagedGatewayClient::new(root.clone(), database_path).unwrap();
            let status = client.status().await.unwrap();
            assert_eq!(
                status.state,
                if keep_managed { "serving" } else { "stopped" }
            );
            assert_eq!(status.active_bindings, usize::from(keep_managed));
            assert_eq!(*state.removed.lock().await, vec![legacy_id]);
            client.status().await.unwrap();
            assert_eq!(state.removed.lock().await.len(), 1);
            server.abort();
            drop(database);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn status_adopts_ready_file_and_reports_serving() {
        let Some((listener, address)) = bind_mock_control().await else {
            eprintln!("skip status_adopts_ready_file_and_reports_serving: loopback bind denied");
            return;
        };
        let router = Router::new().route("/control/status", post(mock_serving_status));
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let (root, database, client) = temp_client();
        fs::write(
            root.join("gateway-sidecar.json"),
            serde_json::json!({
                "protocolVersion": 2,
                "pid": 1,
                "dataBaseUrl": "http://127.0.0.1:43123/v1",
                "controlBaseUrl": format!("http://{address}"),
                "controlToken": "fixture"
            })
            .to_string(),
        )
        .unwrap();
        let status = client.status().await.unwrap();
        assert_eq!(status.state, "serving");
        assert_eq!(status.active_bindings, 1);
        server.abort();
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn status_reports_unavailable_when_data_port_is_occupied_without_control() {
        let _port = ForcedDataPort::open();
        let (root, database, client) = temp_client();
        let status = client.status().await.unwrap();
        assert_eq!(status.state, "unavailable");
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn status_reports_stopped_when_data_port_is_closed() {
        let _port = ForcedDataPort::closed();
        let (root, database, client) = temp_client();
        let status = client.status().await.unwrap();
        assert_eq!(status.state, "stopped");
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ensure_serving_returns_port_in_use_instead_of_timeout_when_data_port_is_occupied() {
        let _port = ForcedDataPort::open();
        let (root, database, client) = temp_client();
        let started = std::time::Instant::now();
        let error = client.ensure_serving().await.unwrap_err();
        assert_eq!(error, "managed_gateway_port_in_use");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "port occupancy must fail closed without waiting for sidecar start timeout"
        );
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gateway_cleanup_only_accepts_our_sidecar_executable() {
        assert!(is_codex_gateway_command(
            "/Applications/R Codex.app/Contents/MacOS/codex-gateway --data-port 43123"
        ));
        assert!(!is_codex_gateway_command(
            "/usr/local/bin/python gateway.py"
        ));
        assert!(!is_codex_gateway_command("/usr/local/bin/codex"));
    }

    #[cfg(windows)]
    #[test]
    fn gateway_cleanup_accepts_windows_sidecar_executable() {
        assert!(is_codex_gateway_command(
            "\"codex-gateway.exe\",123,Console,1,024 K"
        ));
    }
}
