//! CodexAppServerAdapter：探测可执行文件并启动隔离 app-server 会话。
//!
//! 不实现 login/logout。由 official_accounts 的 ProcessAuthDriver 调用。

use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use super::probe::{fnv1a64, validate_auth_schema, ProbeDirectory, SchemaDocuments};
use super::protocol::{io_error, ClientInfo, OfficialAuthError};
use super::session::CodexAppServerSession;
use super::transport::{
    canonical_codex_home, canonical_executable, isolated_command, run_command, ProcessTransport,
};

const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAppServerProbe {
    pub executable: PathBuf,
    pub version: String,
    pub schema_fingerprint: String,
    pub supports_rate_limits: bool,
}

#[derive(Clone, Debug)]
pub struct CodexAppServerAdapter {
    probe: CodexAppServerProbe,
    request_timeout: Duration,
}

impl CodexAppServerAdapter {
    pub fn probe(executable: impl AsRef<Path>) -> Result<Self, OfficialAuthError> {
        Self::probe_with_timeouts(executable, DEFAULT_PROBE_TIMEOUT, DEFAULT_REQUEST_TIMEOUT)
    }

    pub fn probe_with_timeouts(
        executable: impl AsRef<Path>,
        probe_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Self, OfficialAuthError> {
        let executable = canonical_executable(executable.as_ref())?;
        let temporary = ProbeDirectory::create()?;

        let mut version_command = isolated_command(&executable, temporary.codex_home());
        version_command.arg("--version");
        let version_output = run_command(version_command, probe_timeout, "codex_version")?;
        if !version_output.status.success() {
            return Err(OfficialAuthError::CommandFailed {
                operation: "codex_version",
                status: version_output.status.code(),
            });
        }
        let version = String::from_utf8(version_output.stdout)
            .ok()
            .and_then(|text| {
                text.lines()
                    .find(|line| !line.trim().is_empty())
                    .map(|line| line.trim().to_owned())
            })
            .filter(|line| !line.is_empty())
            .ok_or(OfficialAuthError::InvalidExecutable)?;

        let mut schema_command = isolated_command(&executable, temporary.codex_home());
        schema_command
            .arg("app-server")
            .arg("generate-json-schema")
            .arg("--out")
            .arg(temporary.schema_dir());
        let schema_output = run_command(schema_command, probe_timeout, "app_server_schema")?;
        if !schema_output.status.success() {
            return Err(OfficialAuthError::CommandFailed {
                operation: "app_server_schema",
                status: schema_output.status.code(),
            });
        }

        let documents = SchemaDocuments::load(temporary.schema_dir())?;
        let capabilities = validate_auth_schema(&documents)?;
        let bundle = fs::read(
            temporary
                .schema_dir()
                .join("codex_app_server_protocol.schemas.json"),
        )
        .map_err(|source| io_error("read_schema_bundle", source))?;

        Ok(Self {
            probe: CodexAppServerProbe {
                executable,
                version,
                schema_fingerprint: format!("fnv1a64:{:016x}", fnv1a64(&bundle)),
                supports_rate_limits: capabilities.supports_rate_limits,
            },
            request_timeout,
        })
    }

    pub fn probe_result(&self) -> &CodexAppServerProbe {
        &self.probe
    }

    pub fn start_session(
        &self,
        codex_home: impl AsRef<Path>,
        client: ClientInfo,
    ) -> Result<CodexAppServerSession, OfficialAuthError> {
        let codex_home = canonical_codex_home(codex_home.as_ref())?;
        let mut command = isolated_command(&self.probe.executable, &codex_home);
        command
            .arg("app-server")
            .arg("--listen")
            .arg("stdio://")
            .arg("-c")
            .arg("cli_auth_credentials_store=\"file\"")
            .arg("-c")
            .arg("forced_login_method=\"chatgpt\"");

        let transport = ProcessTransport::spawn(command)?;
        CodexAppServerSession::initialize(
            Box::new(transport),
            codex_home,
            client,
            self.request_timeout,
            self.probe.supports_rate_limits,
        )
    }
}
