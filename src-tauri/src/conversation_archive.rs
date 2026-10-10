//! Local request archive for troubleshooting provider/session compatibility.
//!
//! The archive is deliberately separate from the Codex history files.  It
//! stores what R Codex actually observed at each forwarding boundary, while the
//! native Codex conversation remains the source of truth for messages.

use std::{
    fs,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::support::unix_now_seconds;

mod capture;
pub mod identity;
pub use capture::{ArchiveCapture, RequestCapture};

const MAX_STAGE_BYTES: usize = 128 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: i64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct ConversationArchive {
    root: PathBuf,
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveStatus {
    pub enabled: bool,
    pub root: String,
    pub request_count: i64,
    pub stage_count: i64,
    pub stored_bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRequestSummary {
    pub request_id: String,
    pub scope_id: String,
    pub conversation_key: Option<String>,
    pub created_at: i64,
    pub stages: i64,
    pub partial: bool,
    pub capture_status: String,
    pub correlation_status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveStageInfo {
    pub stage: String,
    pub observed_bytes: i64,
    pub stored_bytes: i64,
    pub partial: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRequestDetail {
    pub request: ArchiveRequestSummary,
    pub stages: Vec<ArchiveStageInfo>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveBodyPage {
    pub text: String,
    pub offset: u64,
    pub next_offset: u64,
    pub total_bytes: u64,
    pub truncated: bool,
}

impl ConversationArchive {
    pub fn open(app_data_dir: &Path) -> Result<Self, String> {
        let root = app_data_dir.join("conversation-history");
        fs::create_dir_all(&root)
            .map_err(|e| format!("conversation_archive_directory_failed:{e}"))?;
        set_private_directory(&root)?;
        let connection = Connection::open(root.join("archive.sqlite"))
            .map_err(|e| format!("conversation_archive_open_failed:{e}"))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA busy_timeout = 5000;
                 PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS archive_settings (
                    id INTEGER PRIMARY KEY CHECK (id = 1), enabled INTEGER NOT NULL DEFAULT 0,
                    stored_bytes INTEGER NOT NULL DEFAULT 0
                 );
                 INSERT OR IGNORE INTO archive_settings (id, enabled) VALUES (1, 0);
                 CREATE TABLE IF NOT EXISTS archive_requests (
                    request_id TEXT PRIMARY KEY,
                    scope_id TEXT NOT NULL,
                    conversation_key TEXT,
                    created_at INTEGER NOT NULL,
                    partial INTEGER NOT NULL DEFAULT 0,
                    relative_dir TEXT,
                    capture_status TEXT NOT NULL DEFAULT 'recording',
                    correlation_status TEXT NOT NULL DEFAULT 'pending',
                    metadata_json TEXT NOT NULL DEFAULT '{}'
                 );
                 CREATE TABLE IF NOT EXISTS archive_stages (
                    request_id TEXT NOT NULL,
                    stage TEXT NOT NULL,
                    relative_path TEXT NOT NULL,
                    observed_bytes INTEGER NOT NULL,
                    stored_bytes INTEGER NOT NULL,
                    sha256 TEXT NOT NULL,
                    partial INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (request_id, stage),
                    FOREIGN KEY (request_id) REFERENCES archive_requests(request_id) ON DELETE CASCADE
                 );
                 CREATE INDEX IF NOT EXISTS archive_requests_created
                    ON archive_requests(created_at DESC, request_id DESC);
                 CREATE INDEX IF NOT EXISTS archive_requests_conversation
                    ON archive_requests(scope_id, conversation_key, created_at DESC, request_id DESC);
                 CREATE TABLE IF NOT EXISTS archive_scopes (
                    scope_id TEXT PRIMARY KEY,
                    scope_slug TEXT NOT NULL UNIQUE,
                    created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS archive_conversations (
                    scope_id TEXT NOT NULL,
                    native_thread_id TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY(scope_id, native_thread_id)
                 );",
            )
            .map_err(|e| format!("conversation_archive_schema_failed:{e}"))?;
        // Older development archives predate the scoped layout. Keep their
        // rows readable without silently assigning them a native session.
        for (name, declaration) in [
            ("relative_dir", "TEXT"),
            ("capture_status", "TEXT NOT NULL DEFAULT 'recording'"),
            ("correlation_status", "TEXT NOT NULL DEFAULT 'pending'"),
            ("metadata_json", "TEXT NOT NULL DEFAULT '{}'"),
        ] {
            let exists: bool = connection
                .prepare("PRAGMA table_info(archive_requests)")
                .map_err(|e| e.to_string())?
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| e.to_string())?
                .any(|row| row.is_ok_and(|column| column == name));
            if !exists {
                connection
                    .execute_batch(&format!(
                        "ALTER TABLE archive_requests ADD COLUMN {name} {declaration}"
                    ))
                    .map_err(|e| format!("conversation_archive_schema_failed:{e}"))?;
            }
        }
        let has_stored_bytes = connection
            .prepare("PRAGMA table_info(archive_settings)")
            .map_err(|e| format!("conversation_archive_schema_failed:{e}"))?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("conversation_archive_schema_failed:{e}"))?
            .any(|row| row.is_ok_and(|name| name == "stored_bytes"));
        if !has_stored_bytes {
            connection.execute_batch(
                "ALTER TABLE archive_settings ADD COLUMN stored_bytes INTEGER NOT NULL DEFAULT 0;
                 UPDATE archive_settings SET stored_bytes=(SELECT COALESCE(SUM(stored_bytes),0) FROM archive_stages) WHERE id=1;"
            ).map_err(|e| format!("conversation_archive_schema_failed:{e}"))?;
        }
        set_private_file(&root.join("archive.sqlite"))?;
        Ok(Self {
            root,
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn status(&self) -> Result<ArchiveStatus, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let enabled: i64 = connection
            .query_row("SELECT enabled FROM archive_settings WHERE id=1", [], |r| {
                r.get(0)
            })
            .map_err(|e| format!("conversation_archive_settings_failed:{e}"))?;
        let request_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM archive_requests", [], |r| r.get(0))
            .map_err(|e| format!("conversation_archive_count_failed:{e}"))?;
        let stage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM archive_stages", [], |r| r.get(0))
            .map_err(|e| format!("conversation_archive_count_failed:{e}"))?;
        let stored_bytes: i64 = connection
            .query_row(
                "SELECT stored_bytes FROM archive_settings WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| format!("conversation_archive_count_failed:{e}"))?;
        Ok(ArchiveStatus {
            enabled: enabled != 0,
            root: self.root.display().to_string(),
            request_count,
            stage_count,
            stored_bytes,
        })
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<ArchiveStatus, String> {
        self.connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?
            .execute(
                "UPDATE archive_settings SET enabled=?1 WHERE id=1",
                [enabled as i64],
            )
            .map_err(|e| format!("conversation_archive_settings_failed:{e}"))?;
        self.status()
    }

    /// Starts an immutable request row.  The native conversation key is
    /// intentionally marked as unverified until the version-specific Codex
    /// history probe proves which field maps to its rollout file.
    pub fn begin_request(
        &self,
        request_id: &str,
        scope_id: &str,
        conversation_key: Option<&str>,
        correlation_status: &str,
    ) -> Result<bool, String> {
        if uuid::Uuid::parse_str(request_id).is_err()
            || conversation_key.is_some_and(|key| uuid::Uuid::parse_str(key).is_err())
        {
            return Err("conversation_archive_invalid_reference".into());
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| format!("conversation_archive_request_failed:{e}"))?;
        // Admission is decided by `ArchiveCapture::begin` before this event
        // enters the queue. Do not reread the mutable policy here: a user
        // stopping capture while this event is waiting must finish this
        // request as `partial`, rather than silently losing its index row.
        let scope_slug = format!("{:x}", Sha256::digest(scope_id.as_bytes()));
        let relative_dir = PathBuf::from("scopes")
            .join(&scope_slug[..24])
            .join(if conversation_key.is_some() {
                "conversations"
            } else {
                "unmatched"
            })
            .join(conversation_key.unwrap_or(request_id))
            .join("requests")
            .join(request_id);
        transaction.execute(
            "INSERT OR IGNORE INTO archive_scopes(scope_id,scope_slug,created_at) VALUES (?1,?2,?3)",
            params![scope_id, &scope_slug[..24], unix_now_seconds()],
        ).map_err(|e| format!("conversation_archive_request_failed:{e}"))?;
        if let Some(native_id) = conversation_key {
            transaction.execute(
                "INSERT OR IGNORE INTO archive_conversations(scope_id,native_thread_id,created_at) VALUES (?1,?2,?3)",
                params![scope_id, native_id, unix_now_seconds()],
            ).map_err(|e| format!("conversation_archive_request_failed:{e}"))?;
        }
        transaction.execute(
            "INSERT OR IGNORE INTO archive_requests (request_id,scope_id,conversation_key,created_at,relative_dir,correlation_status) VALUES (?1,?2,?3,?4,?5,?6)",
            params![request_id, scope_id, conversation_key, unix_now_seconds(), relative_dir.to_string_lossy().to_string(), correlation_status],
        ).map_err(|e| format!("conversation_archive_request_failed:{e}"))?;
        transaction
            .commit()
            .map_err(|e| format!("conversation_archive_request_failed:{e}"))?;
        Ok(true)
    }

    fn stage_path(&self, request_id: &str, stage: &str) -> Result<(PathBuf, PathBuf), String> {
        if uuid::Uuid::parse_str(request_id).is_err() || !valid_stage(stage) {
            return Err("conversation_archive_invalid_reference".into());
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let relative = relative_stage_path(&connection, request_id, stage)?;
        Ok((relative.clone(), self.root.join(relative)))
    }

    /// Append one bounded, already-redacted chunk. The SQL write lock encloses
    /// file creation and append, so a concurrent clear cannot unlink the
    /// archive and then have this request recreate it afterwards.
    fn append_stage_chunk(
        &self,
        request_id: &str,
        stage: &str,
        bytes: &[u8],
        observed: u64,
    ) -> Result<bool, String> {
        if !valid_stage(stage) || uuid::Uuid::parse_str(request_id).is_err() {
            return Err("conversation_archive_invalid_reference".into());
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        let active: Option<String> = tx
            .query_row(
                "SELECT capture_status FROM archive_requests WHERE request_id=?1",
                [request_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        if active.as_deref() != Some("recording") {
            return Err("conversation_archive_request_inactive".into());
        }
        let relative = relative_stage_path(&tx, request_id, stage)?;
        let path = self.root.join(&relative);
        let part = path.with_extension("body.part");
        let previous: Option<(i64, i64, i64)> = tx.query_row(
            "SELECT observed_bytes,stored_bytes,partial FROM archive_stages WHERE request_id=?1 AND stage=?2",
            params![request_id, stage], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        ).optional().map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        let (old_observed, old_stored, old_partial) = previous.unwrap_or((0, 0, 0));
        let archive_bytes: i64 = tx
            .query_row(
                "SELECT stored_bytes FROM archive_settings WHERE id=1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        let available = (MAX_STAGE_BYTES as i64 - old_stored)
            .min(MAX_ARCHIVE_BYTES - archive_bytes)
            .max(0) as usize;
        let saved = bytes.len().min(available);
        let clipped = saved < bytes.len();
        if saved > 0 || previous.is_none() {
            let parent = path.parent().ok_or("conversation_archive_path_failed")?;
            private_directory_tree(&self.root, parent)?;
            let mut file = open_archive_part(&self.root, &part)?;
            set_private_file(&part)?;
            let actual_size = file
                .metadata()
                .map_err(|e| format!("conversation_archive_write_failed:{e}"))?
                .len();
            if actual_size < old_stored as u64 {
                return Err("conversation_archive_stage_length_conflict".into());
            }
            if actual_size > old_stored as u64 {
                // A crash after append but before SQLite commit leaves a
                // stale tail. Truncate it before retrying the same event;
                // otherwise every restart would permanently mark the request
                // partial even though the queued event is still available.
                file.set_len(old_stored as u64)
                    .map_err(|e| format!("conversation_archive_write_failed:{e}"))?;
            }
            file.write_all(&bytes[..saved])
                .map_err(|e| format!("conversation_archive_write_failed:{e}"))?;
        }
        tx.execute(
            "INSERT INTO archive_stages(request_id,stage,relative_path,observed_bytes,stored_bytes,sha256,partial) VALUES (?1,?2,?3,?4,?5,'',?6) ON CONFLICT(request_id,stage) DO UPDATE SET observed_bytes=excluded.observed_bytes,stored_bytes=excluded.stored_bytes,partial=excluded.partial",
            params![request_id, stage, relative.to_string_lossy().as_ref(), old_observed.saturating_add(observed as i64), old_stored + saved as i64, (old_partial != 0 || clipped) as i64],
        ).map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        tx.execute(
            "UPDATE archive_settings SET stored_bytes=stored_bytes+?1 WHERE id=1",
            [saved as i64],
        )
        .map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        if clipped {
            tx.execute(
                "UPDATE archive_requests SET partial=1 WHERE request_id=?1",
                [request_id],
            )
            .map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        }
        tx.commit()
            .map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        Ok(clipped)
    }

    fn finalize_stage(&self, request_id: &str, stage: &str, partial: bool) -> Result<(), String> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        let relative = relative_stage_path(&tx, request_id, stage)?;
        let path = self.root.join(&relative);
        let part = path.with_extension("body.part");
        let existing: Option<(i64, i64)> = tx
            .query_row(
                "SELECT stored_bytes,partial FROM archive_stages WHERE request_id=?1 AND stage=?2",
                params![request_id, stage],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        let Some((stored, was_partial)) = existing else {
            return Ok(());
        };
        let mut file = open_archive_file(&self.root, &part)?;
        if file
            .metadata()
            .map_err(|e| format!("conversation_archive_read_failed:{e}"))?
            .len()
            != stored as u64
        {
            return Err("conversation_archive_stage_length_conflict".into());
        }
        file.sync_all()
            .map_err(|e| format!("conversation_archive_write_failed:{e}"))?;
        let digest = hash_reader(&mut file)?;
        drop(file);
        fs::rename(&part, &path).map_err(|e| format!("conversation_archive_publish_failed:{e}"))?;
        tx.execute(
            "UPDATE archive_stages SET sha256=?3,partial=?4 WHERE request_id=?1 AND stage=?2",
            params![
                request_id,
                stage,
                digest,
                (partial || was_partial != 0) as i64
            ],
        )
        .map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        tx.commit()
            .map_err(|e| format!("conversation_archive_stage_failed:{e}"))?;
        Ok(())
    }

    /// Direct in-process write used by archive tests. Gateway traffic goes
    /// through `ArchiveCapture` so a 128 MiB stage is never assembled in the
    /// worker as one buffer.
    #[cfg(test)]
    fn record_stage(&self, request_id: &str, stage: &str, body: &[u8]) -> Result<(), String> {
        let clipped = self.append_stage_chunk(request_id, stage, body, body.len() as u64)?;
        self.finalize_stage(request_id, stage, clipped)
    }

    pub fn list(&self, limit: u32) -> Result<Vec<ArchiveRequestSummary>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let mut statement = connection.prepare("SELECT request_id,scope_id,conversation_key,created_at,(SELECT COUNT(*) FROM archive_stages s WHERE s.request_id=r.request_id),partial,capture_status,correlation_status FROM archive_requests r ORDER BY created_at DESC LIMIT ?1")
            .map_err(|e| format!("conversation_archive_query_failed:{e}"))?;
        let rows = statement
            .query_map([limit.min(500)], |row| {
                Ok(ArchiveRequestSummary {
                    request_id: row.get(0)?,
                    scope_id: row.get(1)?,
                    conversation_key: row.get(2)?,
                    created_at: row.get(3)?,
                    stages: row.get(4)?,
                    partial: row.get::<_, i64>(5)? != 0,
                    capture_status: row.get(6)?,
                    correlation_status: row.get(7)?,
                })
            })
            .map_err(|e| format!("conversation_archive_query_failed:{e}"))?;
        rows.map(|r| r.map_err(|e| format!("conversation_archive_query_failed:{e}")))
            .collect()
    }

    pub fn clear(&self) -> Result<(), String> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| format!("conversation_archive_clear_failed:{e}"))?;
        let enabled: i64 = tx
            .query_row(
                "SELECT enabled FROM archive_settings WHERE id=1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("conversation_archive_clear_failed:{e}"))?;
        if enabled != 0 {
            return Err("conversation_archive_disable_before_clear".into());
        }
        let trash = self.root.join(format!(".trash-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&trash).map_err(|e| format!("conversation_archive_clear_failed:{e}"))?;
        set_private_directory(&trash)?;
        let mut moved = Vec::new();
        for name in ["scopes", "requests"] {
            let from = self.root.join(name);
            if from.exists() {
                if let Err(error) = fs::rename(&from, trash.join(name)) {
                    for previous in moved.iter().rev() {
                        let _ = fs::rename(trash.join(previous), self.root.join(previous));
                    }
                    let _ = fs::remove_dir(&trash);
                    return Err(format!("conversation_archive_clear_failed:{error}"));
                }
                moved.push(name);
            }
        }
        let deleted = tx.execute_batch("DELETE FROM archive_stages; DELETE FROM archive_requests; DELETE FROM archive_conversations; DELETE FROM archive_scopes; UPDATE archive_settings SET stored_bytes=0 WHERE id=1;")
            .and_then(|_| tx.commit());
        if let Err(error) = deleted {
            for previous in moved.iter().rev() {
                let _ = fs::rename(trash.join(previous), self.root.join(previous));
            }
            return Err(format!("conversation_archive_clear_failed:{error}"));
        }
        drop(connection);
        // The index is already empty. Failed trash deletion is retried at the
        // next writer startup; it cannot silently resurrect old requests.
        let _ = fs::remove_dir_all(trash);
        Ok(())
    }

    pub fn detail(&self, request_id: &str) -> Result<Option<ArchiveRequestDetail>, String> {
        if uuid::Uuid::parse_str(request_id).is_err() {
            return Err("conversation_archive_invalid_reference".into());
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let request = connection.query_row("SELECT request_id,scope_id,conversation_key,created_at,(SELECT COUNT(*) FROM archive_stages s WHERE s.request_id=r.request_id),partial,capture_status,correlation_status FROM archive_requests r WHERE request_id=?1", [request_id], |row| Ok(ArchiveRequestSummary {
            request_id: row.get(0)?, scope_id: row.get(1)?, conversation_key: row.get(2)?, created_at: row.get(3)?, stages: row.get(4)?, partial: row.get::<_, i64>(5)? != 0, capture_status: row.get(6)?, correlation_status: row.get(7)?,
        })).optional().map_err(|e| format!("conversation_archive_query_failed:{e}"))?;
        let Some(request) = request else {
            return Ok(None);
        };
        let metadata: String = connection
            .query_row(
                "SELECT metadata_json FROM archive_requests WHERE request_id=?1",
                [request_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("conversation_archive_query_failed:{e}"))?;
        let mut statement = connection.prepare("SELECT stage,observed_bytes,stored_bytes,partial FROM archive_stages WHERE request_id=?1 ORDER BY stage")
            .map_err(|e| format!("conversation_archive_query_failed:{e}"))?;
        let stages = statement
            .query_map([request_id], |row| {
                Ok(ArchiveStageInfo {
                    stage: row.get(0)?,
                    observed_bytes: row.get(1)?,
                    stored_bytes: row.get(2)?,
                    partial: row.get::<_, i64>(3)? != 0,
                })
            })
            .map_err(|e| format!("conversation_archive_query_failed:{e}"))?
            .map(|row| row.map_err(|e| format!("conversation_archive_query_failed:{e}")))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(ArchiveRequestDetail {
            request,
            stages,
            metadata: serde_json::from_str(&metadata).unwrap_or_default(),
        }))
    }

    pub fn body_page(
        &self,
        request_id: &str,
        stage: &str,
        offset: u64,
        limit: u32,
    ) -> Result<ArchiveBodyPage, String> {
        let (expected, path) = self.stage_path(request_id, stage)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let found = connection.query_row("SELECT relative_path,stored_bytes,partial FROM archive_stages WHERE request_id=?1 AND stage=?2", params![request_id, stage], |row| Ok((row.get::<_, String>(0)?,row.get::<_, i64>(1)?,row.get::<_, i64>(2)?)))
            .optional().map_err(|e| format!("conversation_archive_query_failed:{e}"))?;
        let Some((relative, total, partial)) = found else {
            return Err("conversation_archive_stage_missing".into());
        };
        if Path::new(&relative) != expected {
            return Err("conversation_archive_invalid_reference".into());
        }
        let mut file = open_archive_file(&self.root, &path)?;
        let offset = offset.min(total.max(0) as u64);
        file.seek(std::io::SeekFrom::Start(offset))
            .map_err(|e| format!("conversation_archive_read_failed:{e}"))?;
        let mut buffer = vec![0; limit.min(64 * 1024) as usize];
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("conversation_archive_read_failed:{e}"))?;
        buffer.truncate(count);
        Ok(ArchiveBodyPage {
            text: String::from_utf8_lossy(&buffer).into_owned(),
            offset,
            next_offset: offset + count as u64,
            total_bytes: total.max(0) as u64,
            truncated: partial != 0,
        })
    }

    pub fn mark_partial(&self, request_id: &str) -> Result<(), String> {
        self.connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?
            .execute(
                "UPDATE archive_requests SET partial=1,capture_status='partial' WHERE request_id=?1",
                [request_id],
            )
            .map_err(|e| format!("conversation_archive_request_failed:{e}"))?;
        Ok(())
    }

    fn finish_request(
        &self,
        request_id: &str,
        partial: bool,
        metadata: &serde_json::Value,
    ) -> Result<(), String> {
        self.connection.lock().map_err(|_| "conversation_archive_busy")?.execute(
            "UPDATE archive_requests SET partial=(partial OR ?2),capture_status=CASE WHEN partial=1 OR ?2=1 THEN 'partial' ELSE 'complete' END,metadata_json=?4 WHERE request_id=?1 AND capture_status='recording'",
            params![request_id, partial as i64, if partial { "partial" } else { "complete" }, metadata.to_string()],
        ).map_err(|e| format!("conversation_archive_finish_failed:{e}"))?;
        Ok(())
    }

    /// Only the sidecar writer calls this on startup. A UI reader must not
    /// label a still-live sidecar request as interrupted.
    fn recover_interrupted(&self) -> Result<(), String> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
        let mut statement = tx.prepare("SELECT s.request_id,s.stage,s.relative_path,s.stored_bytes FROM archive_stages s JOIN archive_requests r ON r.request_id=s.request_id WHERE r.capture_status='recording'")
            .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
        let rows: Vec<(String, String, String, i64)> = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?
            .map(|row| row.map_err(|e| format!("conversation_archive_recovery_failed:{e}")))
            .collect::<Result<_, _>>()?;
        drop(statement);
        for (id, stage, relative, previous_size) in rows {
            let expected = relative_stage_path(&tx, &id, &stage)?;
            if expected != PathBuf::from(relative) {
                continue;
            }
            let final_path = self.root.join(&expected);
            let part = final_path.with_extension("body.part");
            let actual = if part.exists() {
                Some((&part, true))
            } else if final_path.exists() {
                Some((&final_path, false))
            } else {
                None
            };
            if let Some((source, rename)) = actual {
                let mut file = open_archive_file(&self.root, source)?;
                file.sync_all()
                    .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
                let size = file
                    .metadata()
                    .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?
                    .len();
                let digest = hash_reader(&mut file)?;
                drop(file);
                if rename {
                    fs::rename(&part, &final_path)
                        .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
                }
                tx.execute("UPDATE archive_stages SET stored_bytes=?3,sha256=?4,partial=1 WHERE request_id=?1 AND stage=?2",
                    params![id,stage,size as i64,digest]).map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
                tx.execute(
                    "UPDATE archive_settings SET stored_bytes=MAX(0,stored_bytes+?1) WHERE id=1",
                    [size as i64 - previous_size],
                )
                .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
            } else {
                tx.execute(
                    "UPDATE archive_stages SET stored_bytes=0,partial=1 WHERE request_id=?1 AND stage=?2",
                    params![id, stage],
                )
                .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
                tx.execute(
                    "UPDATE archive_settings SET stored_bytes=MAX(0,stored_bytes-?1) WHERE id=1",
                    [previous_size],
                )
                .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
            }
        }
        tx.execute("UPDATE archive_requests SET partial=1,capture_status='interrupted' WHERE capture_status='recording'", [])
            .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
        tx.commit()
            .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
        for entry in fs::read_dir(&self.root)
            .map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?
        {
            let entry = entry.map_err(|e| format!("conversation_archive_recovery_failed:{e}"))?;
            if entry.file_name().to_string_lossy().starts_with(".trash-")
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
            {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
        Ok(())
    }
}

fn relative_stage_path(
    connection: &Connection,
    request_id: &str,
    stage: &str,
) -> Result<PathBuf, String> {
    if uuid::Uuid::parse_str(request_id).is_err() || !valid_stage(stage) {
        return Err("conversation_archive_invalid_reference".into());
    }
    let relative_dir: String = connection.query_row(
        "SELECT COALESCE(relative_dir,'requests/' || request_id) FROM archive_requests WHERE request_id=?1",
        [request_id], |row| row.get(0)
    ).map_err(|e| format!("conversation_archive_request_missing:{e}"))?;
    let relative = PathBuf::from(relative_dir).join(format!("{stage}.body"));
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("conversation_archive_invalid_reference".into());
    }
    Ok(relative)
}

fn private_directory_tree(root: &Path, parent: &Path) -> Result<(), String> {
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| "conversation_archive_invalid_reference")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err("conversation_archive_invalid_reference".into());
        };
        current.push(name);
        match fs::create_dir(&current) {
            Ok(()) => set_private_directory(&current)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&current)
                    .map_err(|e| format!("conversation_archive_directory_failed:{e}"))?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err("conversation_archive_invalid_reference".into());
                }
            }
            Err(error) => return Err(format!("conversation_archive_directory_failed:{error}")),
        }
    }
    Ok(())
}

fn hash_reader(file: &mut fs::File) -> Result<String, String> {
    file.rewind()
        .map_err(|e| format!("conversation_archive_read_failed:{e}"))?;
    let mut hasher = Sha256::new();
    hasher.update(b"3r-codex-archive-v1\0");
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("conversation_archive_read_failed:{e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[cfg(unix)]
fn open_archive_file(root: &Path, path: &Path) -> Result<fs::File, String> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "conversation_archive_invalid_reference")?;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .open(root)
        .map_err(|e| format!("conversation_archive_read_failed:{e}"))?;
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(name) = component else {
            return Err("conversation_archive_invalid_reference".into());
        };
        let name =
            CString::new(name.as_bytes()).map_err(|_| "conversation_archive_invalid_reference")?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if index + 1 == components.len() {
                0
            } else {
                libc::O_DIRECTORY
            };
        let fd = unsafe { libc::openat(file.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(format!(
                "conversation_archive_read_failed:{}",
                std::io::Error::last_os_error()
            ));
        }
        file = unsafe { fs::File::from_raw_fd(fd) };
    }
    if components.is_empty()
        || !file
            .metadata()
            .map_err(|e| format!("conversation_archive_read_failed:{e}"))?
            .is_file()
    {
        return Err("conversation_archive_invalid_reference".into());
    }
    Ok(file)
}

#[cfg(unix)]
fn open_archive_part(root: &Path, path: &Path) -> Result<fs::File, String> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "conversation_archive_invalid_reference")?;
    let components: Vec<_> = relative.components().collect();
    let mut directory =
        fs::File::open(root).map_err(|e| format!("conversation_archive_write_failed:{e}"))?;
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(name) = component else {
            return Err("conversation_archive_invalid_reference".into());
        };
        let name =
            CString::new(name.as_bytes()).map_err(|_| "conversation_archive_invalid_reference")?;
        let last = index + 1 == components.len();
        let flags = libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if last {
                libc::O_CREAT | libc::O_RDWR | libc::O_APPEND
            } else {
                libc::O_RDONLY | libc::O_DIRECTORY
            };
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
        if fd < 0 {
            return Err(format!(
                "conversation_archive_write_failed:{}",
                std::io::Error::last_os_error()
            ));
        }
        directory = unsafe { fs::File::from_raw_fd(fd) };
    }
    if components.is_empty()
        || !directory
            .metadata()
            .map_err(|e| format!("conversation_archive_write_failed:{e}"))?
            .is_file()
    {
        return Err("conversation_archive_invalid_reference".into());
    }
    Ok(directory)
}

#[cfg(not(unix))]
fn open_archive_part(root: &Path, path: &Path) -> Result<fs::File, String> {
    if !path.starts_with(root) {
        return Err("conversation_archive_invalid_reference".into());
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)
        .map_err(|e| format!("conversation_archive_write_failed:{e}"))
}

#[cfg(not(unix))]
fn open_archive_file(root: &Path, path: &Path) -> Result<fs::File, String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "conversation_archive_invalid_reference")?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| "conversation_archive_invalid_reference")?;
    if !canonical_path.starts_with(canonical_root) {
        return Err("conversation_archive_invalid_reference".into());
    }
    fs::File::open(canonical_path).map_err(|e| format!("conversation_archive_read_failed:{e}"))
}

fn valid_stage(stage: &str) -> bool {
    matches!(
        stage,
        "ingress" | "upstream-request" | "upstream-response" | "downstream-response" | "identity"
    )
}

fn set_private_file(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("conversation_archive_permission_failed:{e}"))?;
    Ok(())
}

fn set_private_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("conversation_archive_permission_failed:{e}"))?;
    Ok(())
}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

impl std::fmt::Debug for ConversationArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConversationArchive")
            .field("root", &self.root)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use std::{
        thread,
        time::{Duration, Instant},
    };

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("rcodex-archive-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn store(&self) -> ConversationArchive {
            ConversationArchive::open(&self.0).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn wait_for(store: &ConversationArchive, id: &str, status: &str) -> ArchiveRequestDetail {
        let started = Instant::now();
        loop {
            if let Some(detail) = store.detail(id).unwrap() {
                if detail.request.capture_status == status {
                    return detail;
                }
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "archive request {id} never reached {status}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn last_handle_finalizes_four_stages_and_keeps_the_secret_off_disk() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.set_enabled(true).unwrap();
        let writer = ArchiveCapture::start(store.clone());
        let request = writer
            .begin(
                "default".into(),
                &HeaderMap::new(),
                br#"{"input":"hello"}"#,
                None,
                vec!["top-secret-value".into()],
            )
            .unwrap();
        let id = request.id().to_owned();
        let duplicate = request.clone();
        request.stage("ingress", b"ignored duplicate");
        request.stage("upstream-request", b"request");
        request.stage("upstream-response", b"before top-");
        request.stage("upstream-response", b"secret-value after");
        duplicate.stage("downstream-response", b"done");
        request.metadata(serde_json::json!({"outcome":"success","inputTokens":12}));
        drop(request);
        assert_ne!(
            store
                .detail(&id)
                .unwrap()
                .map(|d| d.request.capture_status)
                .as_deref(),
            Some("complete")
        );
        drop(duplicate);
        let detail = wait_for(&store, &id, "complete");
        assert_eq!(detail.stages.len(), 4);
        assert_eq!(detail.metadata["inputTokens"], 12);
        assert!(!detail.request.partial);
        let page = store.body_page(&id, "upstream-response", 0, 4096).unwrap();
        assert_eq!(page.text, "before [redacted] after");
        assert!(!page.text.contains("top-secret-value"));
        assert_eq!(
            store.body_page(&id, "ingress", 0, 4096).unwrap().text,
            r#"{"input":"hello"}"#
        );
    }

    #[test]
    fn crash_recovery_publishes_partial_file_and_marks_interrupted() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.set_enabled(true).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        assert!(store
            .begin_request(&id, "default", None, "pending")
            .unwrap());
        store
            .append_stage_chunk(&id, "ingress", b"partial-text", 12)
            .unwrap();
        assert!(store
            .stage_path(&id, "ingress")
            .unwrap()
            .1
            .with_extension("body.part")
            .exists());
        store.recover_interrupted().unwrap();
        let detail = store.detail(&id).unwrap().unwrap();
        assert_eq!(detail.request.capture_status, "interrupted");
        assert!(detail.request.partial);
        assert!(detail.stages[0].partial);
        assert_eq!(
            store.body_page(&id, "ingress", 0, 1024).unwrap().text,
            "partial-text"
        );
    }

    #[test]
    fn clear_is_serialized_with_active_writer_and_old_request_cannot_reappear() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.set_enabled(true).unwrap();
        let writer = ArchiveCapture::start(store.clone());
        let capture = writer
            .begin("default".into(), &HeaderMap::new(), b"hello", None, vec![])
            .unwrap();
        let id = capture.id().to_owned();
        let started = Instant::now();
        while store.detail(&id).unwrap().is_none() {
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(10));
        }
        store.set_enabled(false).unwrap();
        writer.set_enabled(false);
        store.clear().unwrap();
        capture.stage("upstream-response", b"late");
        drop(capture);
        thread::sleep(Duration::from_millis(150));
        assert_eq!(store.status().unwrap().request_count, 0);
        assert!(store.detail(&id).unwrap().is_none());
        assert!(!store.root.join("scopes").exists());
    }

    #[test]
    fn conversation_index_groups_verified_native_identity_without_path_injection() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.set_enabled(true).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let native = uuid::Uuid::new_v4().to_string();
        assert!(store
            .begin_request(&id, "default/../../x", Some(&native), "verified")
            .unwrap());
        store.record_stage(&id, "ingress", b"hello").unwrap();
        store
            .finish_request(&id, false, &serde_json::json!({}))
            .unwrap();
        let detail = store.detail(&id).unwrap().unwrap();
        assert_eq!(
            detail.request.conversation_key.as_deref(),
            Some(native.as_str())
        );
        assert_eq!(detail.request.correlation_status, "verified");
        assert_eq!(
            store.body_page(&id, "ingress", 0, 10).unwrap().text,
            "hello"
        );
        assert!(store.body_page(&id, "../../elsewhere", 0, 10).is_err());
        let relative = store.stage_path(&id, "ingress").unwrap().0;
        assert!(relative
            .to_string_lossy()
            .contains(&format!("conversations/{native}")));
        assert!(!relative.to_string_lossy().contains("../../"));
    }

    #[test]
    fn dropped_events_counts_disabled_and_failed_registration() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let writer = ArchiveCapture::start(store.clone());
        assert_eq!(writer.dropped_events(), 0);
        assert!(writer
            .begin("default".into(), &HeaderMap::new(), b"{}", None, vec![])
            .is_none());
        assert_eq!(writer.dropped_events(), 1);

        store.set_enabled(true).unwrap();
        writer.set_enabled(true);
        assert!(writer
            .begin(
                "default".into(),
                &HeaderMap::new(),
                b"{}",
                None,
                vec!["x".repeat(8193)],
            )
            .is_none());
        assert_eq!(writer.dropped_events(), 2);
    }
}
