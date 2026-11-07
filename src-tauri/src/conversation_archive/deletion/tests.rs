use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rcodex-archive-scope-delete-{}",
            uuid::Uuid::new_v4()
        ));
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

fn seed(store: &ConversationArchive, scope: &str, data: &[u8], legacy: bool) -> String {
    let request = uuid::Uuid::new_v4().to_string();
    let native = uuid::Uuid::new_v4().to_string();
    assert!(store
        .begin_request(&request, scope, Some(&native), "verified")
        .unwrap());
    if legacy {
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE archive_requests SET relative_dir=NULL WHERE request_id=?1",
                [&request],
            )
            .unwrap();
    }
    store.record_stage(&request, "ingress", data).unwrap();
    store
        .finish_request(&request, false, &serde_json::json!({}))
        .unwrap();
    request
}

fn count(store: &ConversationArchive, table: &str, scope: &str) -> i64 {
    store
        .connection
        .lock()
        .unwrap()
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE scope_id=?1"),
            [scope],
            |row| row.get(0),
        )
        .unwrap()
}

fn trash_for(store: &ConversationArchive, scope: &str) -> PathBuf {
    store.root.join(format!(
        ".deleted-scope-{:x}",
        Sha256::digest(scope.as_bytes())
    ))
}

#[test]
fn deleting_one_scope_preserves_other_bodies_policy_and_byte_accounting() {
    let fixture = Fixture::new();
    let store = fixture.store();
    store.set_enabled(true).unwrap();
    let a = seed(&store, "space-a", b"private A", false);
    let b = seed(&store, "space-b", b"preserve B content", false);
    let a_dir = store
        .stage_path(&a, "ingress")
        .unwrap()
        .1
        .parent()
        .unwrap()
        .to_path_buf();
    let b_path = store.stage_path(&b, "ingress").unwrap().1;
    let b_bytes = fs::read(&b_path).unwrap();
    store.delete_scopes(&["space-a".into()]).unwrap();
    assert!(!a_dir.exists());
    assert!(store.detail(&a).unwrap().is_none());
    assert_eq!(fs::read(&b_path).unwrap(), b_bytes);
    assert_eq!(
        store.body_page(&b, "ingress", 0, 100).unwrap().text,
        "preserve B content"
    );
    assert_eq!(count(&store, "archive_scopes", "space-a"), 0);
    assert_eq!(count(&store, "archive_conversations", "space-a"), 0);
    assert_eq!(count(&store, "archive_conversations", "space-b"), 1);
    let status = store.status().unwrap();
    assert!(status.enabled);
    assert_eq!(status.request_count, 1);
    assert_eq!(status.stage_count, 1);
    assert_eq!(status.stored_bytes, b_bytes.len() as i64);
    store
        .delete_scopes(&["space-a".into(), "space-a".into()])
        .unwrap();
    assert_eq!(store.status().unwrap().stored_bytes, b_bytes.len() as i64);
    assert!(!trash_for(&store, "space-a").exists());
}

#[test]
fn existing_separate_writer_cannot_resurrect_deleted_scope() {
    let fixture = Fixture::new();
    let store = fixture.store();
    store.set_enabled(true).unwrap();
    // This writer opens before deletion, like the independently running sidecar.
    let writer = fixture.store();
    let request = uuid::Uuid::new_v4().to_string();
    assert!(writer
        .begin_request(&request, "space-a", None, "unmatched")
        .unwrap());
    writer
        .append_stage_chunk(&request, "ingress", b"early", 5)
        .unwrap();
    let partial = writer
        .stage_path(&request, "ingress")
        .unwrap()
        .1
        .with_extension("body.part");
    assert!(partial.exists());
    let b = seed(&writer, "space-b", b"B", false);
    store.delete_scopes(&["space-a".into()]).unwrap();
    assert!(!partial.exists());
    assert!(writer
        .append_stage_chunk(&request, "ingress", b"late", 4)
        .is_err());
    assert!(writer.finalize_stage(&request, "ingress", false).is_err());
    writer
        .finish_request(&request, false, &serde_json::json!({}))
        .unwrap();
    writer.mark_partial(&request).unwrap();
    let late_id = uuid::Uuid::new_v4().to_string();
    assert!(!writer
        .begin_request(&late_id, "space-a", None, "unmatched")
        .unwrap());
    // SQLite enforces the tombstone even for an older direct INSERT path.
    assert!(writer
        .connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO archive_requests(request_id,scope_id,created_at) VALUES (?1,'space-a',0)",
            [&late_id]
        )
        .is_err());
    assert_eq!(store.status().unwrap().request_count, 1);
    assert_eq!(store.body_page(&b, "ingress", 0, 10).unwrap().text, "B");
    let reopened = fixture.store();
    assert!(!reopened
        .begin_request(&late_id, "space-a", None, "unmatched")
        .unwrap());
    assert!(writer
        .begin_request(&late_id, "space-c", None, "unmatched")
        .unwrap());
    writer
        .record_stage(&late_id, "ingress", b"other space still captures")
        .unwrap();
}

#[test]
fn legacy_request_folders_are_removed_individually() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let a = seed(&store, "space-a", b"A legacy", true);
    let b = seed(&store, "space-b", b"B legacy", true);
    let a_dir = store.root.join("requests").join(&a);
    let b_dir = store.root.join("requests").join(&b);
    assert!(a_dir.exists() && b_dir.exists());
    store.delete_scopes(&["space-a".into()]).unwrap();
    assert!(!a_dir.exists());
    assert!(b_dir.exists());
    assert_eq!(
        store.body_page(&b, "ingress", 0, 100).unwrap().text,
        "B legacy"
    );
    assert_eq!(store.status().unwrap().stored_bytes, 8);
}

#[test]
fn malicious_stage_path_fails_before_removing_any_selected_scope() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let a = seed(&store, "space-a", b"A", false);
    let b = seed(&store, "space-b", b"B", false);
    let a_path = store.stage_path(&a, "ingress").unwrap().1;
    let (b_relative, b_path) = store.stage_path(&b, "ingress").unwrap();
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE archive_stages SET relative_path='../outside' WHERE request_id=?1",
            [&b],
        )
        .unwrap();
    assert!(store
        .delete_scopes(&["space-a".into(), "space-b".into()])
        .is_err());
    assert_eq!(fs::read(&a_path).unwrap(), b"A");
    assert_eq!(fs::read(&b_path).unwrap(), b"B");
    assert_eq!(store.status().unwrap().request_count, 2);
    assert!(!scope_deleted(&store.connection.lock().unwrap(), "space-a").unwrap());
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE archive_stages SET relative_path=?2 WHERE request_id=?1",
            params![b, b_relative.to_string_lossy()],
        )
        .unwrap();
    store.delete_scopes(&["space-a".into()]).unwrap();
    assert_eq!(store.body_page(&b, "ingress", 0, 10).unwrap().text, "B");
}

#[test]
fn selected_request_cannot_point_into_another_scopes_directory() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let a = seed(&store, "space-a", b"A", false);
    let b = seed(&store, "space-b", b"B", false);
    let a_path = store.stage_path(&a, "ingress").unwrap().1;
    let b_relative = store.stage_path(&b, "ingress").unwrap().0;
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE archive_requests SET relative_dir=?2 WHERE request_id=?1",
            params![a, b_relative.parent().unwrap().to_string_lossy()],
        )
        .unwrap();
    assert!(store.delete_scopes(&["space-a".into()]).is_err());
    assert_eq!(fs::read(a_path).unwrap(), b"A");
    assert_eq!(store.body_page(&b, "ingress", 0, 100).unwrap().text, "B");
}

#[test]
fn unselected_scope_referencing_a_selected_body_blocks_deletion() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let a = seed(&store, "space-a", b"A", false);
    let b = seed(&store, "space-b", b"B", false);
    let (a_relative, a_path) = store.stage_path(&a, "ingress").unwrap();
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE archive_stages SET relative_path=?2 WHERE request_id=?1",
            params![b, a_relative.to_string_lossy()],
        )
        .unwrap();
    assert!(store.delete_scopes(&["space-a".into()]).is_err());
    assert_eq!(fs::read(a_path).unwrap(), b"A");
    assert_eq!(store.status().unwrap().request_count, 2);
}

#[cfg(unix)]
#[test]
fn symlink_escape_blocks_deletion_before_other_files_are_touched() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let store = fixture.store();
    let a = seed(&store, "space-a", b"A", false);
    let b = seed(&store, "space-b", b"B", false);
    let outside = fixture.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), b"must survive").unwrap();
    let a_path = store.stage_path(&a, "ingress").unwrap().1;
    let link = a_path.parent().unwrap().join("escape");
    symlink(&outside, &link).unwrap();
    assert!(store
        .delete_scopes(&["space-a".into(), "space-b".into()])
        .is_err());
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"must survive");
    assert_eq!(fs::read(&a_path).unwrap(), b"A");
    assert_eq!(store.body_page(&b, "ingress", 0, 10).unwrap().text, "B");
    fs::remove_file(link).unwrap();
    store.delete_scopes(&["space-a".into()]).unwrap();
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"must survive");
}

#[test]
fn previous_quarantine_is_retryable_after_index_was_already_removed() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let b = seed(&store, "space-b", b"B", false);
    // Represents a committed delete whose final physical purge did not finish.
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO archive_deleted_scopes(scope_id,deleted_at) VALUES ('space-a',0)",
            [],
        )
        .unwrap();
    let trash = trash_for(&store, "space-a");
    fs::create_dir_all(trash.join("old-batch/requests")).unwrap();
    fs::write(
        trash.join("old-batch/requests/body.part"),
        b"pending private data",
    )
    .unwrap();
    store.delete_scopes(&["space-a".into()]).unwrap();
    assert!(!trash.exists());
    assert_eq!(store.body_page(&b, "ingress", 0, 10).unwrap().text, "B");
    assert_eq!(store.status().unwrap().stored_bytes, 1);
    store.delete_scopes(&["space-a".into()]).unwrap();
    store.delete_scopes(&[]).unwrap();
}

#[test]
fn startup_recovery_does_not_publish_tombstoned_scopes_pending_retry() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let request = uuid::Uuid::new_v4().to_string();
    store
        .begin_request(&request, "space-a", None, "unmatched")
        .unwrap();
    store
        .append_stage_chunk(&request, "ingress", b"pending", 7)
        .unwrap();
    let final_path = store.stage_path(&request, "ingress").unwrap().1;
    let part_path = final_path.with_extension("body.part");
    // A tombstone was committed but a later filesystem/SQL step failed.
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO archive_deleted_scopes(scope_id,deleted_at) VALUES ('space-a',0)",
            [],
        )
        .unwrap();
    let writer = fixture.store();
    writer.recover_interrupted().unwrap();
    assert!(part_path.exists());
    assert!(!final_path.exists());
    assert_eq!(
        writer
            .detail(&request)
            .unwrap()
            .unwrap()
            .request
            .capture_status,
        "recording"
    );
    assert!(writer.finalize_stage(&request, "ingress", false).is_err());
    store.delete_scopes(&["space-a".into()]).unwrap();
    assert!(!part_path.exists());
    assert_eq!(store.status().unwrap().stored_bytes, 0);
}

#[test]
fn index_failure_restores_files_and_keeps_tombstone_until_retry() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let a = seed(&store, "space-a", b"A private", false);
    let b = seed(&store, "space-b", b"B preserved", false);
    let a_path = store.stage_path(&a, "ingress").unwrap().1;
    store.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_scope_delete BEFORE DELETE ON archive_requests WHEN OLD.scope_id='space-a' BEGIN SELECT RAISE(ABORT,'injected delete failure'); END;").unwrap();
    assert!(store.delete_scopes(&["space-a".into()]).is_err());
    assert_eq!(fs::read(&a_path).unwrap(), b"A private");
    assert_eq!(
        store.body_page(&b, "ingress", 0, 100).unwrap().text,
        "B preserved"
    );
    assert_eq!(store.status().unwrap().request_count, 2);
    assert_eq!(store.status().unwrap().stored_bytes, 20);
    let late = uuid::Uuid::new_v4().to_string();
    assert!(!store
        .begin_request(&late, "space-a", None, "unmatched")
        .unwrap());
    store
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_scope_delete;")
        .unwrap();
    store.delete_scopes(&["space-a".into()]).unwrap();
    assert!(!a_path.exists());
    assert_eq!(store.status().unwrap().stored_bytes, 11);
    assert_eq!(
        store.body_page(&b, "ingress", 0, 100).unwrap().text,
        "B preserved"
    );
}
