use std::fs;
use std::path::{Component, Path};

use crate::models::{ArchiveResult, DeleteMode, DeleteResult, OperationFailure};

use super::OPENCODE_PROVIDER_ID;

pub(super) fn failure(
    session_key: impl Into<String>,
    message: impl Into<String>,
) -> OperationFailure {
    OperationFailure {
        provider_id: OPENCODE_PROVIDER_ID.to_string(),
        session_key: session_key.into(),
        message: message.into(),
    }
}

use super::storage::{compact, database_bytes, has_events, open_writable, ORPHAN_GROUPS};

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains('\0')
        || Path::new(session_id)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Invalid session id".to_string());
    }
    if !session_id.starts_with("ses_") {
        return Err("OpenCode session ids must start with ses_".to_string());
    }
    Ok(())
}

pub(super) fn archive_at(session_keys: &[String], root: &Path) -> ArchiveResult {
    let mut result = ArchiveResult {
        archived_count: 0,
        failed: Vec::new(),
    };
    let db_path = root.join("opencode.db");
    let conn = match open_writable(&db_path) {
        Ok(conn) => conn,
        Err(error) => {
            result.failed.push(failure(String::new(), error));
            return result;
        }
    };
    let now_ms = chrono::Utc::now().timestamp_millis();
    for key in session_keys {
        if let Err(message) = validate_session_id(key) {
            result.failed.push(failure(key.clone(), message));
            continue;
        }
        let archived_at: Result<Option<i64>, rusqlite::Error> = conn.query_row(
            "SELECT time_archived FROM session WHERE id = ?1",
            [key],
            |row| row.get(0),
        );
        match archived_at {
            Ok(Some(_)) => {
                result
                    .failed
                    .push(failure(key.clone(), "Already archived".to_string()));
            }
            Ok(None) => {
                match conn.execute(
                    "UPDATE session SET time_archived = ?1, time_updated = ?1 WHERE id = ?2 AND time_archived IS NULL",
                    rusqlite::params![now_ms, key],
                ) {
                    Ok(1) => result.archived_count += 1,
                    Ok(_) => result
                        .failed
                        .push(failure(key.clone(), "Session not found".to_string())),
                    Err(error) => result
                        .failed
                        .push(failure(key.clone(), format!("Failed to archive: {error}"))),
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                result
                    .failed
                    .push(failure(key.clone(), "Session not found".to_string()));
            }
            Err(error) => result
                .failed
                .push(failure(key.clone(), format!("Failed to archive: {error}"))),
        }
    }
    result
}

/// SQLite rows cannot be recovered from OS Trash; reject that mode before writing.
pub(super) fn delete_at(session_keys: &[String], mode: DeleteMode, root: &Path) -> DeleteResult {
    let mut result = DeleteResult {
        deleted_count: 0,
        freed_bytes: 0,
        failed: Vec::new(),
    };
    if matches!(mode, DeleteMode::Trash) {
        result.failed.extend(session_keys.iter().map(|key| {
            failure(
                key.clone(),
                "Provider does not support moving sessions to trash",
            )
        }));
        return result;
    }
    let db_path = root.join("opencode.db");
    let conn = match open_writable(&db_path) {
        Ok(conn) => conn,
        Err(error) => {
            result.failed.push(failure(String::new(), error));
            return result;
        }
    };
    let before = database_bytes(root);
    let mut conn = conn;
    let mut artifact_bytes = 0;
    for key in session_keys {
        if let Err(message) = validate_session_id(key) {
            result.failed.push(failure(key.clone(), message));
            continue;
        }
        let deleted = (|| -> Result<(), String> {
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|error| error.to_string())?;
            let changed = tx
                .execute("DELETE FROM session WHERE id = ?1", [key])
                .map_err(|error| error.to_string())?;
            if changed == 0 {
                return Err("Session not found".to_string());
            }
            if has_events(&tx).map_err(|error| error.to_string())? {
                // OpenCode's durable aggregate ID is the session ID. Delete both
                // tables explicitly, as its own Event.remove implementation does.
                tx.execute("DELETE FROM event WHERE aggregate_id = ?1", [key])
                    .map_err(|error| error.to_string())?;
                tx.execute("DELETE FROM event_sequence WHERE aggregate_id = ?1", [key])
                    .map_err(|error| error.to_string())?;
            }
            tx.commit().map_err(|error| error.to_string())
        })();
        match deleted {
            Ok(()) => {
                result.deleted_count += 1;
                match remove_session_diff(key, root) {
                    Ok(bytes) => artifact_bytes += bytes,
                    Err(message) => result.failed.push(failure(
                        key.clone(),
                        format!("Session deleted, but diff cleanup failed: {message}"),
                    )),
                }
            }
            Err(message) => result.failed.push(failure(key.clone(), message)),
        }
    }
    if result.deleted_count > 0 {
        if let Err(message) = compact(&conn) {
            result.failed.push(failure(
                String::new(),
                format!("Sessions deleted, but space reclamation is incomplete: {message}"),
            ));
        }
    }
    // Measure the database and WAL, never present logical payload size as freed disk space.
    if result.deleted_count > 0 {
        result.freed_bytes = before.saturating_sub(database_bytes(root)) + artifact_bytes;
    }
    result
}

pub(super) fn reclaim_at(root: &Path) -> Result<crate::models::StorageCleanupResult, String> {
    let mut conn = open_writable(&root.join("opencode.db"))?;
    let before = database_bytes(root);
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let mut removed_event_groups = 0;
    if has_events(&tx).map_err(|error| error.to_string())? {
        // Materialize before deleting events; the predicate depends on session.created.
        let ids: Vec<String> = {
            let mut stmt = tx
                .prepare(ORPHAN_GROUPS)
                .map_err(|error| error.to_string())?;
            let rows = stmt
                .query_map([], |row| row.get(0))
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<_, _>>()
                .map_err(|error| error.to_string())?
        };
        for id in ids {
            tx.execute("DELETE FROM event WHERE aggregate_id = ?1", [&id])
                .map_err(|error| error.to_string())?;
            tx.execute("DELETE FROM event_sequence WHERE aggregate_id = ?1", [&id])
                .map_err(|error| error.to_string())?;
            removed_event_groups += 1;
        }
    }
    tx.commit().map_err(|error| error.to_string())?;
    let warning = compact(&conn).err();
    Ok(crate::models::StorageCleanupResult {
        removed_event_groups,
        freed_bytes: before.saturating_sub(database_bytes(root)),
        warning,
    })
}

fn remove_session_diff(session_id: &str, root: &Path) -> Result<u64, String> {
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let mut parent = root.to_path_buf();
    for directory in ["storage", "session_diff"] {
        parent.push(directory);
        match fs::symlink_metadata(&parent) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err("Session diff directory must not be a symbolic link".to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.to_string()),
        }
    }
    let path = parent.join(format!("{session_id}.json"));
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.file_type().is_file() {
        return Err("Session diff is not a regular file".to_string());
    }
    if !fs::canonicalize(&path)
        .map_err(|error| error.to_string())?
        .starts_with(canonical_root)
    {
        return Err("Session diff path escapes the OpenCode root".to_string());
    }
    fs::remove_file(path).map_err(|error| error.to_string())?;
    Ok(metadata.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::opencode::discovery::{scan_opencode_at, tests::create_fixture_db};

    #[test]
    fn delete_cascades_messages_and_parts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = create_fixture_db(&root.join("opencode.db"));
        conn.execute(
            "INSERT INTO session (id, directory, title, time_created, time_updated)
             VALUES ('ses_del', '/tmp/demo', 'Delete me', 1, 2)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES ('m1', 'ses_del', 1, 1, 'abcd')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('p1', 'm1', 'ses_del', 1, 1, 'ef')",
            [],
        )
        .unwrap();
        fs::create_dir_all(root.join("storage/session_diff")).unwrap();
        fs::write(
            root.join("storage/session_diff/ses_del.json"),
            "{\"diff\":true}",
        )
        .unwrap();
        drop(conn);

        let rejected = delete_at(&["ses_del".to_string()], DeleteMode::Trash, root);
        assert_eq!(rejected.deleted_count, 0);
        assert_eq!(rejected.failed.len(), 1);
        assert_eq!(scan_opencode_at(root).sessions.len(), 1);
        assert!(root.join("storage/session_diff/ses_del.json").exists());

        let result = delete_at(&["ses_del".to_string()], DeleteMode::Permanent, root);
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert_eq!(result.deleted_count, 1);
        assert!(result.freed_bytes >= 13);
        assert!(!root.join("storage/session_diff/ses_del.json").exists());

        let scan = scan_opencode_at(root);
        assert!(scan.sessions.is_empty());
    }

    #[test]
    fn archive_sets_time_archived_and_rejects_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = create_fixture_db(&root.join("opencode.db"));
        conn.execute(
            "INSERT INTO session (id, directory, title, time_created, time_updated)
             VALUES ('ses_arch', '/tmp/demo', 'Archive me', 1, 2)",
            [],
        )
        .unwrap();
        drop(conn);

        let first = archive_at(&["ses_arch".to_string()], root);
        assert_eq!(first.archived_count, 1);
        assert!(first.failed.is_empty());
        let second = archive_at(&["ses_arch".to_string()], root);
        assert_eq!(second.archived_count, 0);
        assert!(second.failed[0].message.contains("Already archived"));

        let scan = scan_opencode_at(root);
        assert!(scan.sessions[0].archived);
        assert!(!scan.sessions[0].capabilities.archive);
    }

    #[test]
    fn delete_rejects_invalid_ids() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        create_fixture_db(&root.join("opencode.db"));
        let result = delete_at(
            &[
                "../ses_x".to_string(),
                "session_without_prefix".to_string(),
                String::new(),
            ],
            DeleteMode::Permanent,
            root,
        );
        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.failed.len(), 3);
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    use crate::providers::opencode::discovery::{scan_opencode_at, tests::create_fixture_db};

    fn events(conn: &rusqlite::Connection) {
        conn.execute_batch("CREATE TABLE event_sequence (aggregate_id TEXT PRIMARY KEY, seq INTEGER NOT NULL);
            CREATE TABLE event (id TEXT PRIMARY KEY, aggregate_id TEXT NOT NULL REFERENCES event_sequence(aggregate_id) ON DELETE CASCADE, type TEXT NOT NULL, data TEXT NOT NULL);").unwrap();
    }

    fn insert_session(conn: &rusqlite::Connection, id: &str) {
        conn.execute("INSERT INTO session (id,directory,title,time_created,time_updated) VALUES (?1,'/tmp','test',1,2)", [id]).unwrap();
    }

    fn insert_events(conn: &rusqlite::Connection, id: &str, kind: &str) {
        conn.execute("INSERT INTO event_sequence VALUES (?1,0)", [id])
            .unwrap();
        conn.execute(
            "INSERT INTO event VALUES (?1,?1,?2,?3)",
            rusqlite::params![id, kind, "中".repeat(100_000)],
        )
        .unwrap();
    }

    #[test]
    fn delete_removes_only_selected_events_and_reports_measured_wal_reduction() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = create_fixture_db(&root.join("opencode.db"));
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        events(&conn);
        for id in ["ses_delete", "ses_keep"] {
            insert_session(&conn, id);
            insert_events(&conn, id, "session.created.1");
        }
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        let before = database_bytes(root);
        let result = delete_at(&["ses_delete".into()], DeleteMode::Permanent, root);
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.freed_bytes, before - database_bytes(root));
        assert!(result.freed_bytes > 250_000);
        for table in ["event", "event_sequence"] {
            let count: i64 = conn
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE aggregate_id='ses_keep'"),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1);
            let count: i64 = conn
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE aggregate_id='ses_delete'"),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 0);
        }
        assert_eq!(scan_opencode_at(root).sessions.len(), 1);
    }

    #[test]
    fn reclaim_preserves_live_sessions_and_unknown_streams() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = create_fixture_db(&root.join("opencode.db"));
        events(&conn);
        insert_session(&conn, "ses_keep");
        insert_events(&conn, "ses_keep", "session.created.1");
        insert_events(&conn, "ses_orphan", "session.created.1");
        insert_events(&conn, "other_aggregate", "other.created.1");
        insert_events(&conn, "ses_unknown", "unknown.created.1");
        let scan = scan_opencode_at(root);
        assert_eq!(scan.sessions_bytes, 300_000); // UTF-8 bytes, includes durable events
        assert_eq!(
            scan.other_dirs
                .iter()
                .find(|d| d.name == "Deleted-session events")
                .unwrap()
                .size_bytes,
            300_000
        );
        assert_eq!(
            scan.sessions_bytes + scan.other_dirs.iter().map(|d| d.size_bytes).sum::<u64>(),
            scan.total_bytes
        );
        let before = database_bytes(root);
        let result = reclaim_at(root).unwrap();
        assert_eq!(result.removed_event_groups, 1);
        assert!(result.warning.is_none());
        assert_eq!(result.freed_bytes, before - database_bytes(root));
        assert!(result.freed_bytes > 250_000);
        let count: i64 = conn
            .query_row("SELECT count(*) FROM event", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
        assert_eq!(scan_opencode_at(root).sessions_bytes, 300_000);
        assert_eq!(reclaim_at(root).unwrap().removed_event_groups, 0);
    }

    #[test]
    fn event_failure_rolls_back_session_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let conn = create_fixture_db(&dir.path().join("opencode.db"));
        events(&conn);
        insert_session(&conn, "ses_keep");
        insert_events(&conn, "ses_keep", "session.created.1");
        conn.execute_batch("CREATE TRIGGER reject_delete BEFORE DELETE ON event BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
        let result = delete_at(&["ses_keep".into()], DeleteMode::Permanent, dir.path());
        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.freed_bytes, 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(scan_opencode_at(dir.path()).sessions.len(), 1);
    }

    #[test]
    fn busy_checkpoint_reports_incomplete_reclamation_without_false_freed_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = create_fixture_db(&root.join("opencode.db"));
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        events(&conn);
        insert_events(&conn, "ses_orphan", "session.created.1");
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        let reader = rusqlite::Connection::open(root.join("opencode.db")).unwrap();
        reader.execute_batch("BEGIN; SELECT * FROM event;").unwrap();
        let result = reclaim_at(root).unwrap();
        assert_eq!(result.removed_event_groups, 1);
        assert!(result.warning.as_ref().unwrap().contains("busy"));
        assert_eq!(result.freed_bytes, 0);
        reader.execute_batch("ROLLBACK;").unwrap();
        assert!(reclaim_at(root).unwrap().warning.is_none());
    }

    #[test]
    fn missing_database_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        assert!(reclaim_at(dir.path()).is_err());
        assert!(!dir.path().join("opencode.db").exists());
    }
    #[test]
    #[ignore = "Manual regression against an explicitly prepared disposable SQLite snapshot"]
    fn reclaim_disposable_snapshot() {
        let root = std::path::PathBuf::from(
            std::env::var("AGENT_SWEEP_OPENCODE_TEST_COPY").expect("snapshot directory"),
        );
        let root = fs::canonicalize(root).unwrap();
        assert!(root.starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
        assert!(root.join(".agent-sweep-test-copy").is_file());
        let before = scan_opencode_at(&root);
        assert!(before.issues.is_empty(), "{:?}", before.issues);
        let result = reclaim_at(&root).unwrap();
        assert!(result.warning.is_none(), "{:?}", result.warning);
        let after = scan_opencode_at(&root);
        assert!(after.issues.is_empty());
        assert_eq!(before.sessions.len(), after.sessions.len());
        for session in &before.sessions {
            let kept = after
                .sessions
                .iter()
                .find(|s| s.session_id == session.session_id)
                .unwrap();
            assert_eq!(session.size_bytes, kept.size_bytes);
            assert_eq!(session.title, kept.title);
        }
        let conn = rusqlite::Connection::open(root.join("opencode.db")).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        eprintln!(
            "snapshot: {} sessions preserved; {} orphan groups removed; {} bytes reclaimed",
            after.sessions.len(),
            result.removed_event_groups,
            result.freed_bytes
        );
    }
}
