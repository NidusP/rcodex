//! One-way session import: the user's default Codex home is read-only source.
//! R Codex writes only into its isolated home. config.toml and auth.json are never
//! opened on the source side.
use rusqlite::{types::Value, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

const MAX_METADATA_LINE: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportableProject {
    pub cwd: String,
    pub session_count: usize,
    pub latest_title: String,
    pub sessions: Vec<ImportableSession>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportableSession {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionImportResult {
    pub copied: usize,
    pub skipped: usize,
    pub conflicted: usize,
}

#[derive(Debug, Clone)]
struct ThreadRow {
    id: String,
    cwd: String,
    title: String,
    rollout_path: String,
    provider: String,
    created_at: i64,
}

pub fn list_importable_projects(default_home: &Path) -> Result<Vec<ImportableProject>, String> {
    let rows = read_source_threads(default_home)?;
    let mut grouped: BTreeMap<String, Vec<ImportableSession>> = BTreeMap::new();
    for row in rows {
        if is_archived_session(&row) {
            continue;
        }
        let cwd = if row.cwd.trim().is_empty() {
            "（未分类）".to_string()
        } else {
            row.cwd
        };
        grouped.entry(cwd).or_default().push(ImportableSession {
            id: row.id,
            title: row.title,
            provider: row.provider,
            created_at: row.created_at,
        });
    }
    Ok(grouped
        .into_iter()
        .map(|(cwd, sessions)| ImportableProject {
            cwd,
            session_count: sessions.len(),
            latest_title: sessions
                .first()
                .map(|session| session.title.clone())
                .unwrap_or_default(),
            sessions,
        })
        .collect())
}

pub fn import_project_sessions(
    default_home: &Path,
    isolated_home: &Path,
    isolated_sqlite_home: &Path,
    cwd: &str,
    confirmed_stopped: bool,
) -> Result<SessionImportResult, String> {
    if !confirmed_stopped {
        return Err("session_import_stop_confirmation_required".to_string());
    }
    assert_codex_stopped()?;
    import_project_sessions_after_stop_check(default_home, isolated_home, isolated_sqlite_home, cwd)
}

fn import_project_sessions_after_stop_check(
    default_home: &Path,
    isolated_home: &Path,
    isolated_sqlite_home: &Path,
    cwd: &str,
) -> Result<SessionImportResult, String> {
    let source_root = default_home
        .canonicalize()
        .map_err(|_| "session_import_source_unreadable".to_string())?;
    let isolated = isolated_home
        .canonicalize()
        .map_err(|_| "isolated_home_not_ready".to_string())?;
    let sqlite_root = isolated_sqlite_home
        .canonicalize()
        .map_err(|_| "isolated_home_not_ready".to_string())?;
    for target in [&isolated, &sqlite_root] {
        if target.starts_with(&source_root) || source_root.starts_with(target) {
            return Err("session_import_refuses_default_home".to_string());
        }
    }
    let source_db = source_database_path(&source_root)?
        .ok_or_else(|| "session_import_source_unreadable".to_string())?;
    let destination_db = sqlite_root.join(source_db.file_name().unwrap());
    // Only Codex initializes its schema and migration ledger. Never replay
    // arbitrary CREATE statements from a source database into the target.
    checked_file(&sqlite_root, &destination_db)
        .map_err(|_| "session_import_target_database_not_ready".to_string())?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = destination_db.with_file_name(format!(
            "{}{suffix}",
            destination_db.file_name().unwrap().to_string_lossy()
        ));
        if fs::symlink_metadata(&sidecar).is_ok() {
            checked_file(&sqlite_root, &sidecar)?;
        }
    }
    let mut destination =
        Connection::open_with_flags(&destination_db, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|_| "session_import_write_failed".to_string())?;
    destination
        .busy_timeout(Duration::from_millis(250))
        .map_err(|_| "session_import_write_failed".to_string())?;
    destination
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")
        .map_err(|_| "session_import_write_failed".to_string())?;
    with_source_database(&source_db, |source| {
        let rows = threads_from_connection(source, &source_db)?;
        let wanted = if cwd == "（未分类）" { "" } else { cwd };
        let rows = rows
            .into_iter()
            .filter(|row| !is_archived_session(row))
            .filter(|row| row.cwd.trim() == wanted)
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Err("session_import_project_changed".to_string());
        }
        let source_columns = thread_columns(source)?;
        let target_columns = thread_columns(&destination)?;
        if source_columns != target_columns {
            return Err("session_import_schema_mismatch".to_string());
        }
        for required in ["id", "rollout_path", "model_provider"] {
            if !source_columns.iter().any(|column| column == required) {
                return Err("session_import_schema_mismatch".to_string());
            }
        }
        let transaction = destination
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "session_import_busy".to_string())?;
        let mut copied_files = ImportFiles::default();
        let mut result = SessionImportResult {
            copied: 0,
            skipped: 0,
            conflicted: 0,
        };
        for row in rows {
            let existing_rollout: Option<String> = transaction
                .query_row(
                    "SELECT rollout_path FROM threads WHERE id=?1",
                    [&row.id],
                    |value| value.get(0),
                )
                .optional()
                .map_err(|_| "session_import_write_failed".to_string())?;
            if row.id.is_empty()
                || row.id.len() > 160
                || !row
                    .id
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
            {
                return Err("session_import_invalid_metadata".to_string());
            }
            let rollout = PathBuf::from(&row.rollout_path);
            let rollout = if rollout.is_absolute() {
                rollout
            } else {
                source_root.join(rollout)
            };
            let relative = rollout
                .strip_prefix(&source_root)
                .map_err(|_| "session_import_source_path_rejected".to_string())?;
            if !matches!(relative.components().next(), Some(std::path::Component::Normal(name))
                if name == "sessions")
                || rollout.extension().and_then(|extension| extension.to_str()) != Some("jsonl")
            {
                return Err("session_import_source_path_rejected".to_string());
            }
            checked_file(&source_root, &rollout)?;
            let target_thread_id = if let Some(existing_path) = existing_rollout.as_deref() {
                let existing = PathBuf::from(existing_path);
                let existing = if existing.is_absolute() {
                    existing
                } else {
                    sqlite_root.join(existing)
                };
                let unchanged = existing
                    .strip_prefix(&isolated)
                    .ok()
                    .and_then(|relative| checked_file(&isolated, &existing).ok().map(|_| relative))
                    .is_some_and(|_| rollouts_equivalent(&rollout, &existing, &row.id));
                if unchanged {
                    result.skipped += 1;
                    continue;
                }
                result.conflicted += 1;
                Uuid::new_v4().to_string()
            } else {
                row.id.clone()
            };
            let session_dir = isolated.join("sessions");
            create_private_child(&isolated, &session_dir)?;
            let import_dir = session_dir.join("imported");
            create_private_child(&isolated, &import_dir)?;
            // Random filenames avoid collisions with unindexed files left by
            // an interrupted prior import; native thread identity stays intact.
            let target = import_dir.join(format!("{}-{}.jsonl", target_thread_id, Uuid::new_v4()));
            copied_files.paths.push(target.clone());
            copy_rollout_rewriting_provider(&rollout, &target, &row.id, &target_thread_id)?;
            copy_thread_row(
                source,
                &transaction,
                &row.id,
                &target_thread_id,
                &target.to_string_lossy(),
            )?;
            result.copied += 1;
        }
        transaction
            .commit()
            .map_err(|_| "session_import_write_failed".to_string())?;
        copied_files.committed = true;
        Ok(result)
    })
}

#[derive(Default)]
struct ImportFiles {
    paths: Vec<PathBuf>,
    committed: bool,
}

impl Drop for ImportFiles {
    fn drop(&mut self) {
        if !self.committed {
            for path in &self.paths {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn source_database_path(home: &Path) -> Result<Option<PathBuf>, String> {
    let entries = match fs::read_dir(home) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("session_import_source_unreadable".to_string()),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| "session_import_source_unreadable".to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(version) = name
            .strip_prefix("state_")
            .and_then(|name| name.strip_suffix(".sqlite"))
            .and_then(|version| version.parse::<u32>().ok())
        {
            checked_file(home, &entry.path())?;
            candidates.push((version, entry.path()));
        }
    }
    candidates.sort_by_key(|candidate| candidate.0);
    Ok(candidates.pop().map(|candidate| candidate.1))
}

fn read_source_threads(default_home: &Path) -> Result<Vec<ThreadRow>, String> {
    let Some(source) = source_database_path(default_home)? else {
        return Ok(Vec::new());
    };
    with_source_database(&source, |connection| {
        threads_from_connection(connection, &source)
    })
}

fn threads_from_connection(
    connection: &Connection,
    _source: &Path,
) -> Result<Vec<ThreadRow>, String> {
    let mut statement = connection.prepare(
        "SELECT id, cwd, title, rollout_path, model_provider, created_at FROM threads ORDER BY created_at DESC",
    ).map_err(|_| "session_import_schema_mismatch".to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(ThreadRow {
                id: row.get(0)?,
                cwd: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                rollout_path: row.get(3)?,
                provider: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|_| "session_import_source_unreadable".to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "session_import_source_unreadable".to_string())
}

fn is_archived_session(row: &ThreadRow) -> bool {
    Path::new(&row.rollout_path)
        .components()
        .next()
        .is_some_and(|component| {
            matches!(
                component,
                std::path::Component::Normal(name)
                    if name == std::ffi::OsStr::new("archived_sessions")
            )
        })
}

// SQLite can update shared-memory files even for a read-only connection.
// Query a private DB/WAL copy so scanning cannot mutate any source sidecars.
fn with_source_database<T>(
    source: &Path,
    read: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    let root = std::env::temp_dir().join(format!("3r-session-scan-{}", Uuid::new_v4()));
    fs::create_dir(&root).map_err(|_| "session_import_write_failed".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .map_err(|_| "session_import_write_failed".to_string())?;
    }
    let result = (|| {
        let parent = source.parent().ok_or("session_import_source_unreadable")?;
        let name = source
            .file_name()
            .ok_or("session_import_source_unreadable")?
            .to_string_lossy();
        for suffix in ["", "-wal"] {
            let original = parent.join(format!("{name}{suffix}"));
            if suffix == "-wal"
                && fs::symlink_metadata(&original)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            {
                continue;
            }
            checked_file(parent, &original)?;
            let before = fs::metadata(&original).map_err(|_| "session_import_source_unreadable")?;
            if before.len() > 512 * 1024 * 1024 {
                return Err("session_import_database_too_large".to_string());
            }
            let mut input =
                File::open(&original).map_err(|_| "session_import_source_unreadable")?;
            let mut output = private_file(&root.join(format!("state.sqlite{suffix}")))?;
            std::io::copy(&mut input, &mut output).map_err(|_| "session_import_read_failed")?;
            let after = fs::metadata(&original).map_err(|_| "session_import_source_unreadable")?;
            if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
                return Err("session_import_source_changed".to_string());
            }
        }
        let connection = Connection::open(root.join("state.sqlite"))
            .map_err(|_| "session_import_source_unreadable".to_string())?;
        connection
            .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
            .map_err(|_| "session_import_source_unreadable".to_string())?;
        read(&connection)
    })();
    let _ = fs::remove_dir_all(root);
    result
}

fn checked_file(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "session_import_source_path_rejected")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("session_import_source_path_rejected".to_string());
        }
        current.push(component);
        let metadata =
            fs::symlink_metadata(&current).map_err(|_| "session_import_source_unreadable")?;
        if metadata.file_type().is_symlink() {
            return Err("session_import_source_path_rejected".to_string());
        }
    }
    #[cfg_attr(not(unix), allow(unused_variables))]
    let metadata = fs::metadata(path).map_err(|_| "session_import_source_unreadable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.is_file() && metadata.nlink() != 1 {
            return Err("session_import_source_path_rejected".to_string());
        }
    }
    Ok(())
}

fn create_private_child(root: &Path, path: &Path) -> Result<(), String> {
    if !path.exists() {
        fs::create_dir(path).map_err(|_| "session_import_write_failed")?;
    }
    checked_file(root, path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| "session_import_write_failed")?;
    }
    Ok(())
}

fn private_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_| "session_import_write_failed".to_string())
}

fn thread_columns(connection: &Connection) -> Result<Vec<String>, String> {
    let statement = connection
        .prepare("SELECT * FROM threads LIMIT 0")
        .map_err(|_| "session_import_schema_mismatch".to_string())?;
    Ok(statement
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect())
}

fn copy_thread_row(
    source: &Connection,
    destination: &Connection,
    thread_id: &str,
    target_thread_id: &str,
    rollout_path: &str,
) -> Result<(), String> {
    let mut statement = source
        .prepare("SELECT * FROM threads WHERE id=?1")
        .map_err(|_| "session_import_source_unreadable")?;
    let columns = statement
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let values = statement
        .query_row([thread_id], |row| {
            columns
                .iter()
                .enumerate()
                .map(|(index, column)| match column.as_str() {
                    "id" => Ok(Value::Text(target_thread_id.to_string())),
                    "rollout_path" => Ok(Value::Text(rollout_path.to_string())),
                    "model_provider" => Ok(Value::Text("rcodex".to_string())),
                    "project_id" | "thread_section_id" => Ok(Value::Null),
                    _ => row.get::<_, Value>(index),
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|_| "session_import_source_unreadable")?;
    let placeholders = (1..=values.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let columns = columns
        .iter()
        .map(|column| format!("\"{}\"", column.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");
    destination
        .execute(
            &format!("INSERT INTO threads ({columns}) VALUES ({placeholders})"),
            rusqlite::params_from_iter(values),
        )
        .map_err(|_| "session_import_write_failed".to_string())?;
    Ok(())
}

fn copy_rollout_rewriting_provider(
    source: &Path,
    destination: &Path,
    source_thread_id: &str,
    target_thread_id: &str,
) -> Result<(), String> {
    let input = File::open(source).map_err(|_| "session_import_read_failed".to_string())?;
    let mut reader = BufReader::new(input);
    let mut header = Vec::new();
    reader
        .by_ref()
        .take(MAX_METADATA_LINE + 1)
        .read_until(b'\n', &mut header)
        .map_err(|_| "session_import_read_failed".to_string())?;
    if header.len() as u64 > MAX_METADATA_LINE {
        return Err("session_import_invalid_metadata".to_string());
    }
    let mut meta: serde_json::Value = serde_json::from_slice(&header)
        .map_err(|_| "session_import_invalid_metadata".to_string())?;
    if meta["type"] != "session_meta" || meta["payload"]["id"].as_str() != Some(source_thread_id) {
        return Err("session_import_invalid_metadata".to_string());
    }
    meta["payload"]["model_provider"] = serde_json::Value::String("rcodex".to_string());
    meta["payload"]["id"] = serde_json::Value::String(target_thread_id.to_string());
    let mut output = private_file(destination)?;
    serde_json::to_writer(&mut output, &meta).map_err(|_| "session_import_write_failed")?;
    output
        .write_all(if header.ends_with(b"\r\n") {
            b"\r\n"
        } else if header.ends_with(b"\n") {
            b"\n"
        } else {
            b""
        })
        .map_err(|_| "session_import_write_failed")?;
    // Copy every non-metadata byte unchanged, including CRLF and a final line
    // without a newline. No conversation body passes through the renderer.
    std::io::copy(&mut reader, &mut output).map_err(|_| "session_import_write_failed")?;
    output
        .sync_all()
        .map_err(|_| "session_import_write_failed".to_string())
}

fn rollouts_equivalent(source: &Path, target: &Path, source_thread_id: &str) -> bool {
    let Ok(source_bytes) = normalized_rollout_bytes(source, source_thread_id, source_thread_id)
    else {
        return false;
    };
    let Ok(target_bytes) = normalized_rollout_bytes(target, source_thread_id, source_thread_id)
    else {
        return false;
    };
    source_bytes == target_bytes
}

fn normalized_rollout_bytes(
    path: &Path,
    expected_thread_id: &str,
    normalized_thread_id: &str,
) -> Result<Vec<u8>, String> {
    let input = File::open(path).map_err(|_| "session_import_read_failed".to_string())?;
    let mut reader = BufReader::new(input);
    let mut header = Vec::new();
    reader
        .by_ref()
        .take(MAX_METADATA_LINE + 1)
        .read_until(b'\n', &mut header)
        .map_err(|_| "session_import_read_failed".to_string())?;
    if header.len() as u64 > MAX_METADATA_LINE {
        return Err("session_import_invalid_metadata".to_string());
    }
    let mut meta: serde_json::Value = serde_json::from_slice(&header)
        .map_err(|_| "session_import_invalid_metadata".to_string())?;
    if meta["type"] != "session_meta" || meta["payload"]["id"].as_str() != Some(expected_thread_id)
    {
        return Err("session_import_invalid_metadata".to_string());
    }
    meta["payload"]["id"] = serde_json::Value::String(normalized_thread_id.to_string());
    meta["payload"]["model_provider"] = serde_json::Value::String("rcodex".to_string());
    let mut output = Vec::new();
    serde_json::to_writer(&mut output, &meta)
        .map_err(|_| "session_import_read_failed".to_string())?;
    output.extend_from_slice(if header.ends_with(b"\r\n") {
        b"\r\n"
    } else if header.ends_with(b"\n") {
        b"\n"
    } else {
        b""
    });
    reader
        .read_to_end(&mut output)
        .map_err(|_| "session_import_read_failed".to_string())?;
    Ok(output)
}

fn assert_codex_stopped() -> Result<(), String> {
    #[cfg(unix)]
    {
        let output = std::process::Command::new("ps")
            .args(["-axo", "comm="])
            .output()
            .map_err(|_| "session_import_process_check_failed")?;
        if !output.status.success() {
            return Err("session_import_process_check_failed".to_string());
        }
        let processes = std::str::from_utf8(&output.stdout)
            .map_err(|_| "session_import_process_check_failed")?;
        if processes.lines().any(is_codex_writer) {
            return Err("session_import_busy".to_string());
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let output = crate::support::command_without_console("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .output()
            .map_err(|_| "session_import_process_check_failed")?;
        if !output.status.success() {
            return Err("session_import_process_check_failed".to_string());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        if text
            .lines()
            .filter_map(|line| line.split(',').next())
            .any(|name| is_codex_writer(name.trim_matches('"')))
        {
            return Err("session_import_busy".to_string());
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err("session_import_process_check_failed".to_string())
    }
}

fn is_codex_writer(name: &str) -> bool {
    let name = name.trim().replace('\\', "/").to_ascii_lowercase();
    let base = name.rsplit('/').next().unwrap_or(&name);
    matches!(base, "codex" | "codex.exe" | "chatgpt" | "chatgpt.exe")
        || name.contains("/codex.app/contents/macos/")
        || name.contains("/chatgpt.app/contents/macos/")
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCHEMA: &str = "CREATE TABLE threads(id TEXT PRIMARY KEY, cwd TEXT, title TEXT, rollout_path TEXT, model_provider TEXT, created_at INTEGER)";

    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        target: PathBuf,
        sqlite: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("3r-import-test-{}", Uuid::new_v4()));
            let source = root.join("source");
            let target = root.join("target");
            let sqlite = root.join("sqlite");
            fs::create_dir_all(source.join("sessions")).unwrap();
            fs::create_dir_all(source.join("archived_sessions")).unwrap();
            fs::create_dir_all(&target).unwrap();
            fs::create_dir_all(&sqlite).unwrap();
            for home in [&source, &sqlite] {
                Connection::open(home.join("state_5.sqlite"))
                    .unwrap()
                    .execute_batch(SCHEMA)
                    .unwrap();
            }
            fs::write(source.join("config.toml"), "do not read or modify").unwrap();
            fs::write(source.join("auth.json"), "private source sentinel").unwrap();
            Self {
                root,
                source,
                target,
                sqlite,
            }
        }
        fn add(&self, id: &str, body: &[u8]) {
            let header = format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"model_provider\":\"OpenAI\"}}}}\r\n");
            let mut bytes = header.into_bytes();
            bytes.extend(body);
            fs::write(self.source.join(format!("sessions/{id}.jsonl")), bytes).unwrap();
            Connection::open(self.source.join("state_5.sqlite"))
                .unwrap()
                .execute(
                    "INSERT INTO threads VALUES(?1, '/project', 'Demo', ?2, 'OpenAI', 1)",
                    rusqlite::params![id, format!("sessions/{id}.jsonl")],
                )
                .unwrap();
        }
        fn add_archived(&self, id: &str) {
            let path = format!("archived_sessions/{id}.jsonl");
            fs::write(
                self.source.join(&path),
                format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\"}}}}\n"),
            )
            .unwrap();
            Connection::open(self.source.join("state_5.sqlite"))
                .unwrap()
                .execute(
                    "INSERT INTO threads VALUES(?1, '/archived-project', 'Archived', ?2, 'OpenAI', 2)",
                    rusqlite::params![id, path],
                )
                .unwrap();
        }
        fn import(&self) -> Result<SessionImportResult, String> {
            import_project_sessions_after_stop_check(
                &self.source,
                &self.target,
                &self.sqlite,
                "/project",
            )
        }
        fn target_count(&self) -> i64 {
            Connection::open(self.sqlite.join("state_5.sqlite"))
                .unwrap()
                .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
                .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn import_is_one_way_preserves_body_bytes_and_skips_duplicates() {
        let fixture = Fixture::new();
        let body =
            b"{\"type\":\"event_msg\",\"payload\":{\"text\":\"keep\"}}\r\n{\"type\":\"event_msg\"}";
        fixture.add("thread-a", body);
        let source_before = fs::read(fixture.source.join("sessions/thread-a.jsonl")).unwrap();
        let db_before = fs::read(fixture.source.join("state_5.sqlite")).unwrap();
        let projects = list_importable_projects(&fixture.source).unwrap();
        assert_eq!(projects[0].sessions[0].provider, "OpenAI");
        assert_eq!(fixture.import().unwrap().copied, 1);
        let connection = Connection::open(fixture.sqlite.join("state_5.sqlite")).unwrap();
        let (path, provider): (String, String) = connection
            .query_row(
                "SELECT rollout_path, model_provider FROM threads WHERE id='thread-a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(Path::new(&path).starts_with(fixture.target.canonicalize().unwrap()));
        assert_eq!(provider, "rcodex");
        let copy = fs::read(path).unwrap();
        assert!(copy.ends_with(body));
        assert_eq!(fixture.import().unwrap().skipped, 1);
        assert_eq!(fixture.target_count(), 1);
        assert_eq!(
            fs::read(fixture.source.join("sessions/thread-a.jsonl")).unwrap(),
            source_before
        );
        assert_eq!(
            fs::read(fixture.source.join("state_5.sqlite")).unwrap(),
            db_before
        );
        assert_eq!(
            fs::read_to_string(fixture.source.join("config.toml")).unwrap(),
            "do not read or modify"
        );
        assert_eq!(
            fs::read_to_string(fixture.source.join("auth.json")).unwrap(),
            "private source sentinel"
        );
        assert!(!fixture.target.join("auth.json").exists());
    }

    #[test]
    fn changed_session_gets_a_new_isolated_copy_and_keeps_original() {
        let fixture = Fixture::new();
        fixture.add("thread-a", b"old body");
        assert_eq!(fixture.import().unwrap().copied, 1);
        fs::write(
            fixture.source.join("sessions/thread-a.jsonl"),
            b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread-a\",\"model_provider\":\"OpenAI\"}}\nnew body",
        )
        .unwrap();
        let result = fixture.import().unwrap();
        assert_eq!(result.copied, 1);
        assert_eq!(result.conflicted, 1);
        assert_eq!(result.skipped, 0);
        assert_eq!(fixture.target_count(), 2);
        let connection = Connection::open(fixture.sqlite.join("state_5.sqlite")).unwrap();
        let ids = connection
            .prepare("SELECT id FROM threads ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(ids.contains(&"thread-a".to_string()));
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().any(|id| id != "thread-a"));
    }

    #[test]
    fn archived_sessions_are_not_listed_or_imported() {
        let fixture = Fixture::new();
        fixture.add_archived("archived-thread");
        fixture.add("active-thread", b"body");

        let projects = list_importable_projects(&fixture.source).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].cwd, "/project");
        assert_eq!(projects[0].session_count, 1);
        assert_eq!(fixture.import().unwrap().copied, 1);
        assert_eq!(fixture.target_count(), 1);
        assert!(!fixture
            .target
            .join("sessions/imported/archived-thread.jsonl")
            .exists());
    }

    #[test]
    fn malformed_metadata_rolls_back_rows_and_copied_files() {
        let fixture = Fixture::new();
        fixture.add("thread-a", b"body");
        fixture.add("thread-b", b"body");
        fs::write(fixture.source.join("sessions/thread-b.jsonl"), b"invalid\n").unwrap();
        assert_eq!(
            fixture.import().unwrap_err(),
            "session_import_invalid_metadata"
        );
        assert_eq!(fixture.target_count(), 0);
        assert_eq!(
            fs::read_dir(fixture.target.join("sessions/imported"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn database_failure_rolls_back_files_and_rows() {
        let fixture = Fixture::new();
        fixture.add("thread-a", b"body");
        Connection::open(fixture.sqlite.join("state_5.sqlite")).unwrap().execute_batch(
            "CREATE TRIGGER reject_import BEFORE INSERT ON threads BEGIN SELECT RAISE(ABORT, 'fixture'); END;"
        ).unwrap();
        assert_eq!(fixture.import().unwrap_err(), "session_import_write_failed");
        assert_eq!(fixture.target_count(), 0);
        assert_eq!(
            fs::read_dir(fixture.target.join("sessions/imported"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn source_rollout_cannot_be_a_configuration_file() {
        let fixture = Fixture::new();
        fixture.add("thread-a", b"body");
        Connection::open(fixture.source.join("state_5.sqlite"))
            .unwrap()
            .execute("UPDATE threads SET rollout_path='auth.json'", [])
            .unwrap();
        assert_eq!(
            fixture.import().unwrap_err(),
            "session_import_source_path_rejected"
        );
        assert_eq!(fixture.target_count(), 0);
    }

    #[test]
    fn default_home_and_uninitialized_target_are_rejected() {
        let fixture = Fixture::new();
        fixture.add("thread-a", b"body");
        assert_eq!(
            import_project_sessions_after_stop_check(
                &fixture.source,
                &fixture.target,
                &fixture.source,
                "/project"
            )
            .unwrap_err(),
            "session_import_refuses_default_home"
        );
        fs::remove_file(fixture.sqlite.join("state_5.sqlite")).unwrap();
        assert_eq!(
            fixture.import().unwrap_err(),
            "session_import_target_database_not_ready"
        );
    }

    #[test]
    fn schema_mismatch_is_rejected_before_writing() {
        let fixture = Fixture::new();
        fixture.add("thread-a", b"body");
        Connection::open(fixture.sqlite.join("state_5.sqlite"))
            .unwrap()
            .execute_batch("ALTER TABLE threads ADD COLUMN unknown TEXT;")
            .unwrap();
        assert_eq!(
            fixture.import().unwrap_err(),
            "session_import_schema_mismatch"
        );
        assert_eq!(fixture.target_count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn destination_session_symlink_is_rejected() {
        let fixture = Fixture::new();
        fixture.add("thread-a", b"body");
        std::os::unix::fs::symlink(
            fixture.source.join("sessions"),
            fixture.target.join("sessions"),
        )
        .unwrap();
        assert_eq!(
            fixture.import().unwrap_err(),
            "session_import_source_path_rejected"
        );
        assert_eq!(fixture.target_count(), 0);
        assert_eq!(
            fs::read_dir(fixture.source.join("sessions"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn scanning_wal_database_preserves_source_and_includes_recent_threads() {
        let fixture = Fixture::new();
        let writer = Connection::open(fixture.source.join("state_5.sqlite")).unwrap();
        writer
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        fixture.add("thread-a", b"body");
        writer
            .execute("UPDATE threads SET title='Recent'", [])
            .unwrap();
        let before = ["state_5.sqlite", "state_5.sqlite-wal", "state_5.sqlite-shm"]
            .map(|name| fs::read(fixture.source.join(name)).unwrap());
        assert_eq!(
            list_importable_projects(&fixture.source).unwrap()[0].session_count,
            1
        );
        let after = ["state_5.sqlite", "state_5.sqlite-wal", "state_5.sqlite-shm"]
            .map(|name| fs::read(fixture.source.join(name)).unwrap());
        assert_eq!(before, after);
    }
}
