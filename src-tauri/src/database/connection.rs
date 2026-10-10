//! SQLite 连接打开、权限和过期清理。
//!
//! 不改 schema，不读写业务表。
//! `open` 给 App 装配用；`open_path` / `in_memory` 给 sidecar 和测试。

use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;
use tauri::{App, Manager};

pub(super) const DATABASE_FILENAME: &str = "rcodex.sqlite3";
const BUSY_TIMEOUT_MS: i64 = 5_000;
const DIAGNOSTIC_RETENTION_SECS: i64 = 30 * 24 * 60 * 60;
const USAGE_RETENTION_SECS: i64 = 90 * 24 * 60 * 60;

/// The single SQLite connection shared by all domain stores.
///
/// Schema creation and migrations are deliberately owned here. Store modules
/// only perform domain reads and writes, which keeps startup ordering and
/// future migrations deterministic when multiple stores are active together.
#[derive(Clone)]
pub struct Database {
    pub(super) connection: Arc<Mutex<Connection>>,
    path: Arc<std::path::PathBuf>,
}

impl Database {
    pub fn open(app: &App) -> Result<Self, String> {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("无法定位应用数据目录：{error}"))?;
        fs::create_dir_all(&dir).map_err(|error| format!("无法创建应用数据目录：{error}"))?;
        Self::open_path(dir.join(DATABASE_FILENAME))
    }

    pub fn open_path(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let connection =
            Connection::open(&path).map_err(|error| format!("无法打开本地数据库：{error}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("无法保护本地数据库文件：{error}"))?;
        }
        configure_connection(&connection)?;
        super::schema::migrate(&connection)?;
        prune_expired(&connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            path: Arc::new(path),
        })
    }

    pub(crate) fn connection(&self) -> Arc<Mutex<Connection>> {
        self.connection.clone()
    }

    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        let connection = Connection::open_in_memory().expect("in-memory SQLite should open");
        configure_connection(&connection).expect("in-memory SQLite should configure");
        super::schema::migrate(&connection).expect("in-memory SQLite should migrate");
        Self {
            connection: Arc::new(Mutex::new(connection)),
            path: Arc::new(std::path::PathBuf::from(":memory:")),
        }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        self.path.as_ref()
    }
}

pub(super) fn configure_connection(connection: &Connection) -> Result<(), String> {
    let pragmas = format!(
        "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = {BUSY_TIMEOUT_MS};"
    );
    connection
        .execute_batch(&pragmas)
        .map_err(|error| format!("无法配置 SQLite：{error}"))
}

pub(super) fn prune_expired(connection: &Connection) -> Result<(), String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    connection
        .execute(
            "DELETE FROM gateway_diagnostic_events WHERE created_at < ?1",
            [now - DIAGNOSTIC_RETENTION_SECS],
        )
        .map_err(|error| format!("无法清理过期 Gateway 诊断：{error}"))?;
    connection
        .execute(
            "DELETE FROM provider_usage_events WHERE created_at < ?1",
            [now - USAGE_RETENTION_SECS],
        )
        .map_err(|error| format!("无法清理过期本地用量：{error}"))?;
    Ok(())
}
