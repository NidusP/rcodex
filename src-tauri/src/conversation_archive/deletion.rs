//! Permanent scope deletion without changing the archive policy for other spaces.

use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use super::{private_directory_tree, set_private_directory, valid_stage, ConversationArchive};
use crate::support::unix_now_seconds;

struct ScopeDeletion {
    scope: String,
    trash: PathBuf,
    sources: Vec<PathBuf>,
}

impl ConversationArchive {
    /// Erases only the supplied exact scope IDs. A durable tombstone prevents
    /// queued events in another writer connection from bringing a scope back.
    /// File-purge failures retain the scope's quarantine for an explicit retry.
    pub fn delete_scopes(&self, scope_ids: &[String]) -> Result<(), String> {
        let scopes: BTreeSet<_> = scope_ids.iter().cloned().collect();
        if scopes.is_empty() {
            return Ok(());
        }
        if scopes
            .iter()
            .any(|id| id.is_empty() || id.len() > 4096 || id.chars().any(char::is_control))
        {
            return Err("conversation_archive_invalid_scope".into());
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "conversation_archive_busy")?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("conversation_archive_scope_delete_failed:{error}"))?;
        // Preflight every source and previous quarantine before the first move,
        // purge, or policy change. A corrupted row must not erase another scope.
        let plans = collect_plans(&transaction, &self.root, &scopes)?;
        reject_cross_scope_references(&transaction, &plans)?;
        for scope in &scopes {
            transaction.execute("INSERT OR IGNORE INTO archive_deleted_scopes(scope_id,deleted_at) VALUES (?1,?2)", params![scope, unix_now_seconds()])
                .map_err(|error| format!("conversation_archive_scope_delete_failed:{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("conversation_archive_scope_delete_failed:{error}"))?;

        // Keep tombstones even when a filesystem operation fails: the user has
        // requested permanent deletion and a late writer must not add more data
        // while the caller preserves its workspace record for a retry.
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("conversation_archive_scope_delete_failed:{error}"))?;
        let mut moved = Vec::new();
        let mut batches = Vec::new();
        let stage_result = stage_deletion(&self.root, &plans, &mut moved, &mut batches);
        if let Err(error) = stage_result {
            let restored = restore_moves(&moved, &batches);
            return Err(with_restore_error(error, restored));
        }
        let delete_result = (|| {
            for scope in &scopes {
                transaction.execute("DELETE FROM archive_stages WHERE request_id IN (SELECT request_id FROM archive_requests WHERE scope_id=?1)", [scope])?;
                transaction.execute("DELETE FROM archive_requests WHERE scope_id=?1", [scope])?;
                transaction.execute(
                    "DELETE FROM archive_conversations WHERE scope_id=?1",
                    [scope],
                )?;
                transaction.execute("DELETE FROM archive_scopes WHERE scope_id=?1", [scope])?;
            }
            transaction.execute("UPDATE archive_settings SET stored_bytes=(SELECT COALESCE(SUM(stored_bytes),0) FROM archive_stages) WHERE id=1", [])?;
            transaction.commit()
        })();
        if let Err(error) = delete_result {
            let restored = restore_moves(&moved, &batches);
            return Err(with_restore_error(
                format!("conversation_archive_scope_delete_failed:{error}"),
                restored,
            ));
        }
        drop(connection);
        for plan in plans {
            let trash = checked_path(&self.root, &plan.trash)?;
            match fs::symlink_metadata(&trash) {
                Ok(_) => {
                    validate_tree(&self.root, &plan.trash)?;
                    fs::remove_dir_all(&trash).map_err(|error| {
                        format!("conversation_archive_scope_purge_failed:{error}")
                    })?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("conversation_archive_scope_purge_failed:{error}"))
                }
            }
        }
        Ok(())
    }
}

pub(super) fn scope_deleted(connection: &Connection, scope: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM archive_deleted_scopes WHERE scope_id=?1)",
            [scope],
            |row| row.get(0),
        )
        .map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))
}

pub(super) fn request_writable(connection: &Connection, request: &str) -> Result<bool, String> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM archive_requests request WHERE request_id=?1 AND capture_status='recording'
         AND NOT EXISTS(SELECT 1 FROM archive_deleted_scopes deleted WHERE deleted.scope_id=request.scope_id))",
        [request], |row| row.get(0)
    ).map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))
}

fn collect_plans(
    connection: &Connection,
    root: &Path,
    scopes: &BTreeSet<String>,
) -> Result<Vec<ScopeDeletion>, String> {
    let mut plans = Vec::new();
    for scope in scopes {
        let digest = format!("{:x}", Sha256::digest(scope.as_bytes()));
        let slug = &digest[..24];
        let saved_slug: Option<String> = connection
            .query_row(
                "SELECT scope_slug FROM archive_scopes WHERE scope_id=?1",
                [scope],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))?;
        if saved_slug.as_deref().is_some_and(|saved| saved != slug) {
            return Err("conversation_archive_scope_path_conflict".into());
        }
        let mut sources = BTreeSet::from([PathBuf::from("scopes").join(slug)]);
        let mut statement = connection.prepare("SELECT request_id,conversation_key,relative_dir FROM archive_requests WHERE scope_id=?1")
            .map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))?;
        let rows = statement
            .query_map([scope], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))?;
        for row in rows {
            let (request, conversation, saved) =
                row.map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))?;
            if uuid::Uuid::parse_str(&request).is_err() {
                return Err("conversation_archive_invalid_reference".into());
            }
            let legacy = PathBuf::from("requests").join(&request);
            let relative = saved.map(PathBuf::from).unwrap_or_else(|| legacy.clone());
            if relative == legacy {
                sources.insert(legacy);
            } else {
                if conversation
                    .as_deref()
                    .is_some_and(|key| uuid::Uuid::parse_str(key).is_err())
                {
                    return Err("conversation_archive_invalid_reference".into());
                }
                let expected = PathBuf::from("scopes")
                    .join(slug)
                    .join(if conversation.is_some() {
                        "conversations"
                    } else {
                        "unmatched"
                    })
                    .join(conversation.as_deref().unwrap_or(&request))
                    .join("requests")
                    .join(&request);
                if relative != expected {
                    return Err("conversation_archive_scope_path_conflict".into());
                }
            }
            let mut stages = connection
                .prepare("SELECT stage,relative_path FROM archive_stages WHERE request_id=?1")
                .map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))?;
            let stages = stages
                .query_map([&request], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))?;
            for stage in stages {
                let (stage, path) = stage
                    .map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))?;
                let expected = relative.join(format!("{stage}.body"));
                if !valid_stage(&stage) || PathBuf::from(&path) != expected {
                    return Err("conversation_archive_scope_path_conflict".into());
                }
                checked_path(root, &expected)?;
                checked_path(root, &expected.with_extension("body.part"))?;
            }
        }
        let trash = PathBuf::from(format!(".deleted-scope-{digest}"));
        for relative in sources.iter().chain(std::iter::once(&trash)) {
            validate_tree(root, relative)?;
        }
        plans.push(ScopeDeletion {
            scope: scope.clone(),
            trash,
            sources: sources.into_iter().collect(),
        });
    }
    Ok(plans)
}

fn reject_cross_scope_references(
    connection: &Connection,
    plans: &[ScopeDeletion],
) -> Result<(), String> {
    let mut statement = connection
        .prepare(
            "SELECT scope_id,COALESCE(relative_dir,'requests/' || request_id) FROM archive_requests
         UNION ALL SELECT request.scope_id,stage.relative_path FROM archive_stages stage
         JOIN archive_requests request ON request.request_id=stage.request_id",
        )
        .map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))?;
    for row in rows {
        let (scope, path) =
            row.map_err(|error| format!("conversation_archive_scope_read_failed:{error}"))?;
        let path = PathBuf::from(path);
        for plan in plans {
            if scope != plan.scope
                && (path.starts_with(&plan.trash)
                    || plan.sources.iter().any(|source| path.starts_with(source)))
            {
                return Err("conversation_archive_scope_path_conflict".into());
            }
        }
    }
    Ok(())
}

fn stage_deletion(
    root: &Path,
    plans: &[ScopeDeletion],
    moved: &mut Vec<(PathBuf, PathBuf)>,
    batches: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for plan in plans {
        let existing = plan
            .sources
            .iter()
            .filter_map(|source| {
                let path = root.join(source);
                match fs::symlink_metadata(&path) {
                    Ok(_) => Some(Ok(source)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => Some(Err(format!(
                        "conversation_archive_scope_delete_failed:{error}"
                    ))),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        if existing.is_empty() {
            continue;
        }
        let batch = root
            .join(&plan.trash)
            .join(format!("batch-{}", uuid::Uuid::new_v4()));
        private_directory_tree(root, &batch)?;
        set_private_directory(&batch)?;
        batches.push(batch.clone());
        for relative in existing {
            let source = checked_path(root, relative)?;
            let destination = batch.join(relative);
            private_directory_tree(
                root,
                destination
                    .parent()
                    .ok_or("conversation_archive_invalid_reference")?,
            )?;
            fs::rename(&source, &destination)
                .map_err(|error| format!("conversation_archive_scope_delete_failed:{error}"))?;
            moved.push((source, destination));
        }
    }
    Ok(())
}

fn restore_moves(moved: &[(PathBuf, PathBuf)], batches: &[PathBuf]) -> Result<(), String> {
    for (source, destination) in moved.iter().rev() {
        fs::rename(destination, source)
            .map_err(|error| format!("conversation_archive_scope_restore_failed:{error}"))?;
    }
    for batch in batches {
        fs::remove_dir_all(batch)
            .map_err(|error| format!("conversation_archive_scope_restore_failed:{error}"))?;
    }
    Ok(())
}

fn with_restore_error(error: String, restored: Result<(), String>) -> String {
    match restored {
        Ok(()) => error,
        Err(restore) => format!("{error};{restore}"),
    }
}

/// Reject every symlink/reparse component, including a replaced archive root.
fn checked_path(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("conversation_archive_invalid_reference".into());
    }
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("conversation_archive_scope_path_failed:{error}"))?;
    if linked(&metadata) || !metadata.is_dir() {
        return Err("conversation_archive_invalid_reference".into());
    }
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if linked(&metadata) || (!metadata.is_dir() && !metadata.is_file()) => {
                return Err("conversation_archive_invalid_reference".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(format!("conversation_archive_scope_path_failed:{error}")),
        }
    }
    Ok(root.join(relative))
}

fn validate_tree(root: &Path, relative: &Path) -> Result<(), String> {
    let path = checked_path(root, relative)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("conversation_archive_scope_path_failed:{error}")),
    };
    if !metadata.is_dir() || linked(&metadata) {
        return Err("conversation_archive_invalid_reference".into());
    }
    validate_children(&path)
}

fn validate_children(directory: &Path) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("conversation_archive_scope_path_failed:{error}"))?
    {
        let entry =
            entry.map_err(|error| format!("conversation_archive_scope_path_failed:{error}"))?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| format!("conversation_archive_scope_path_failed:{error}"))?;
        if linked(&metadata) || (!metadata.is_dir() && !metadata.is_file()) {
            return Err("conversation_archive_invalid_reference".into());
        }
        if metadata.is_dir() {
            validate_children(&entry.path())?;
        }
    }
    Ok(())
}

fn linked(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests;
