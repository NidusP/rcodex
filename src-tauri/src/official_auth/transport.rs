//! app-server JSONL 传输和隔离进程启动。
//!
//! 不解析账户协议。由 Adapter 探测/会话和测试 FakeTransport 使用。

use serde_json::Value;
use std::{
    env, fs, io,
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use super::protocol::{io_error, OfficialAuthError};

const MAX_PROTOCOL_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_COMMAND_CAPTURE_BYTES: u64 = 1024 * 1024;

const SAFE_ENVIRONMENT_VARIABLES: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "PATH",
    "TMPDIR",
    "TEMP",
    "TMP",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "CODEX_CA_CERTIFICATE",
    "SSL_CERT_FILE",
];

pub(crate) trait JsonLineTransport: Send {
    fn send(&mut self, value: &Value) -> Result<(), OfficialAuthError>;
    fn receive(&mut self, timeout: Duration) -> Result<Value, OfficialAuthError>;
    fn shutdown(&mut self);
}

pub(crate) enum TransportEvent {
    Message(Value),
    Malformed,
    Closed,
    Io,
}

pub(crate) struct ProcessTransport {
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
    events: Receiver<TransportEvent>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl ProcessTransport {
    pub(crate) fn spawn(mut command: Command) -> Result<Self, OfficialAuthError> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|source| io_error("spawn_app_server", source))?;
        let stdin = child
            .stdin
            .take()
            .ok_or(OfficialAuthError::TransportClosed)?;
        let stdout = child
            .stdout
            .take()
            .ok_or(OfficialAuthError::TransportClosed)?;
        let stderr = child
            .stderr
            .take()
            .ok_or(OfficialAuthError::TransportClosed)?;
        let (sender, events) = mpsc::channel();
        let stdout_thread = match spawn_stdout_reader(stdout, sender) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let stderr_thread = match thread::Builder::new()
            .name("codex-app-server-stderr".to_owned())
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                let _ = io::copy(&mut reader, &mut io::sink());
            }) {
            Ok(thread) => thread,
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_thread.join();
                return Err(io_error("spawn_stderr_drain", source));
            }
        };

        Ok(Self {
            child,
            stdin: Some(BufWriter::new(stdin)),
            events,
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
        })
    }
}

impl JsonLineTransport for ProcessTransport {
    fn send(&mut self, value: &Value) -> Result<(), OfficialAuthError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or(OfficialAuthError::TransportClosed)?;
        serde_json::to_writer(&mut *stdin, value)
            .map_err(|source| io_error("serialize_app_server_request", io::Error::other(source)))?;
        stdin
            .write_all(b"\n")
            .and_then(|_| stdin.flush())
            .map_err(|source| io_error("write_app_server_request", source))
    }

    fn receive(&mut self, timeout: Duration) -> Result<Value, OfficialAuthError> {
        match self.events.recv_timeout(timeout) {
            Ok(TransportEvent::Message(value)) => Ok(value),
            Ok(TransportEvent::Malformed) => Err(OfficialAuthError::ProtocolMalformed),
            Ok(TransportEvent::Closed) => Err(OfficialAuthError::TransportClosed),
            Ok(TransportEvent::Io) => Err(OfficialAuthError::TransportClosed),
            Err(RecvTimeoutError::Timeout) => Err(OfficialAuthError::Timeout {
                operation: "app_server_response",
            }),
            Err(RecvTimeoutError::Disconnected) => Err(OfficialAuthError::TransportClosed),
        }
    }

    fn shutdown(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(thread) = self.stdout_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for ProcessTransport {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(crate) fn spawn_stdout_reader(
    stdout: impl Read + Send + 'static,
    sender: Sender<TransportEvent>,
) -> Result<JoinHandle<()>, OfficialAuthError> {
    thread::Builder::new()
        .name("codex-app-server-stdout".to_owned())
        .spawn(move || read_json_lines(stdout, sender))
        .map_err(|source| io_error("spawn_stdout_reader", source))
}

pub(crate) fn read_json_lines(stdout: impl Read, sender: Sender<TransportEvent>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = Vec::new();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => {
                let _ = sender.send(TransportEvent::Closed);
                return;
            }
            Ok(_) if line.len() > MAX_PROTOCOL_LINE_BYTES => {
                let _ = sender.send(TransportEvent::Malformed);
                return;
            }
            Ok(_) => {
                while matches!(line.last(), Some(b'\n' | b'\r')) {
                    line.pop();
                }
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_slice::<Value>(&line) {
                    Ok(value) => {
                        if sender.send(TransportEvent::Message(value)).is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        let _ = sender.send(TransportEvent::Malformed);
                        return;
                    }
                }
            }
            Err(_) => {
                let _ = sender.send(TransportEvent::Io);
                return;
            }
        }
    }
}

pub(crate) fn canonical_executable(path: &Path) -> Result<PathBuf, OfficialAuthError> {
    let canonical = fs::canonicalize(path).map_err(|_| OfficialAuthError::InvalidExecutable)?;
    if !canonical.is_file() {
        return Err(OfficialAuthError::InvalidExecutable);
    }
    Ok(canonical)
}

pub(crate) fn canonical_codex_home(path: &Path) -> Result<PathBuf, OfficialAuthError> {
    let canonical = fs::canonicalize(path).map_err(|_| OfficialAuthError::InvalidCodexHome)?;
    if !canonical.is_dir() {
        return Err(OfficialAuthError::InvalidCodexHome);
    }
    Ok(canonical)
}

pub(crate) fn isolated_command(executable: &Path, codex_home: &Path) -> Command {
    let mut command = Command::new(executable);
    command.env_clear();
    for name in SAFE_ENVIRONMENT_VARIABLES {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("CODEX_HOME", codex_home);
    command.env("CODEX_SQLITE_HOME", codex_home);
    command
}

pub(crate) struct CommandOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
}

pub(crate) fn run_command(
    mut command: Command,
    timeout: Duration,
    operation: &'static str,
) -> Result<CommandOutput, OfficialAuthError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|source| io_error(operation, source))?;
    let stdout = child
        .stdout
        .take()
        .ok_or(OfficialAuthError::TransportClosed)?;
    let stderr = child
        .stderr
        .take()
        .ok_or(OfficialAuthError::TransportClosed)?;
    let stdout_thread = thread::spawn(move || read_capture(stdout));
    let stderr_thread = thread::spawn(move || read_capture(stderr));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|source| io_error(operation, source))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Err(OfficialAuthError::Timeout { operation });
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_thread
        .join()
        .map_err(|_| OfficialAuthError::TransportClosed)??;
    let _ = stderr_thread
        .join()
        .map_err(|_| OfficialAuthError::TransportClosed)??;
    Ok(CommandOutput { status, stdout })
}

fn read_capture(reader: impl Read) -> Result<Vec<u8>, OfficialAuthError> {
    let mut output = Vec::new();
    reader
        .take(MAX_COMMAND_CAPTURE_BYTES)
        .read_to_end(&mut output)
        .map_err(|source| io_error("read_command_output", source))?;
    Ok(output)
}
