use std::fs;
use std::path::{Component, Path, PathBuf};

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

fn open_writable(db_path: &Path) -> Result<rusqlite::Connection, String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|error| error.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| error.to_string())?;
    Ok(conn)
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains('\0')
        || Path::new(session_id).components().any(|c| !matches!(c, Component::Normal(_)))
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
pub(super) fn delete_at(
    session_keys: &[String],
    mode: DeleteMode,
    root: &Path,
) -> DeleteResult {
    let mut result = DeleteResult {
        deleted_count: 0,
        freed_bytes: 0,
        failed: Vec::new(),
    };
    if matches!(mode, DeleteMode::Trash) {
        result.failed.extend(session_keys.iter().map(|key| {
            failure(key.clone(), "Provider does not support moving sessions to trash")
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
    let storage_root = root.join("storage");
    let allowed_storage = fs::canonicalize(&storage_root).ok();

    for key in session_keys {
        if let Err(message) = validate_session_id(key) {
            result.failed.push(failure(key.clone(), message));
            continue;
        }
        let size_bytes = session_size_bytes(&conn, key).unwrap_or(0);
        match conn.execute("DELETE FROM session WHERE id = ?1", [key]) {
            Ok(0) => {
                result
                    .failed
                    .push(failure(key.clone(), "Session not found".to_string()));
            }
            Ok(_) => {
                result.deleted_count += 1;
                result.freed_bytes += size_bytes;
                if let Some(allowed) = &allowed_storage {
                    let _ = remove_session_diff(key, &storage_root, allowed, mode);
                }
            }
            Err(error) => result
                .failed
                .push(failure(key.clone(), format!("Failed to delete: {error}"))),
        }
    }
    result
}

fn session_size_bytes(conn: &rusqlite::Connection, session_id: &str) -> Result<u64, rusqlite::Error> {
    let message_bytes: i64 = conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM message WHERE session_id = ?1",
        [session_id],
        |row| row.get(0),
    )?;
    let part_bytes: i64 = conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM part WHERE session_id = ?1",
        [session_id],
        |row| row.get(0),
    )?;
    Ok((message_bytes + part_bytes) as u64)
}

fn remove_session_diff(
    session_id: &str,
    storage_root: &Path,
    allowed_storage: &Path,
    mode: DeleteMode,
) -> Result<(), String> {
    let rel = PathBuf::from("session_diff").join(format!("{session_id}.json"));
    if rel
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Invalid session diff path".to_string());
    }
    let path = storage_root.join(&rel);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(()),
    };
    if !metadata.file_type().is_file() {
        return Err("Session diff is not a regular file".to_string());
    }
    let canonical =
        fs::canonicalize(&path).map_err(|error| format!("Failed to resolve path: {error}"))?;
    if !canonical.starts_with(allowed_storage) {
        return Err("Session diff path escapes storage/".to_string());
    }
    match mode {
        DeleteMode::Trash => trash::delete(&path)
            .map_err(|error| format!("Failed to move session diff to trash: {error}")),
        DeleteMode::Permanent => {
            fs::remove_file(&path).map_err(|error| format!("Failed to delete session diff: {error}"))
        }
    }
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

        let result = delete_at(
            &["ses_del".to_string()],
            DeleteMode::Permanent,
            root,
        );
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.freed_bytes, 6);
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
