use std::path::{Component, Path};

use crate::models::{ArchiveResult, DeleteMode, DeleteResult, OperationFailure};

use super::ZCODE_PROVIDER_ID;

pub(super) fn failure(
    session_key: impl Into<String>,
    message: impl Into<String>,
) -> OperationFailure {
    OperationFailure {
        provider_id: ZCODE_PROVIDER_ID.to_string(),
        session_key: session_key.into(),
        message: message.into(),
    }
}

fn open_writable(db_path: &Path) -> Result<rusqlite::Connection, String> {
    rusqlite::Connection::open(db_path).map_err(|error| error.to_string())
}

/// Session keys are `workspace_key::task_id`. workspace_key may itself contain
/// `::`, so split on the final occurrence.
fn parse_session_key(key: &str) -> Result<(String, String), String> {
    let (workspace_key, task_id) = key
        .rsplit_once("::")
        .ok_or_else(|| "Invalid ZCode session key".to_string())?;
    if workspace_key.is_empty() || task_id.is_empty() {
        return Err("Invalid ZCode session key".to_string());
    }
    if !task_id.starts_with("sess_") {
        return Err("ZCode task ids must start with sess_".to_string());
    }
    if task_id.contains('/') || task_id.contains('\\') || task_id.contains('\0') {
        return Err("Invalid ZCode task id".to_string());
    }
    if Path::new(task_id)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Invalid ZCode task id".to_string());
    }
    Ok((workspace_key.to_string(), task_id.to_string()))
}

pub(super) fn archive_at(session_keys: &[String], root: &Path) -> ArchiveResult {
    let mut result = ArchiveResult {
        archived_count: 0,
        failed: Vec::new(),
    };
    let db_path = root.join("v2/tasks-index.sqlite");
    let conn = match open_writable(&db_path) {
        Ok(conn) => conn,
        Err(error) => {
            result.failed.push(failure(String::new(), error));
            return result;
        }
    };
    let now_ms = chrono::Utc::now().timestamp_millis();
    for key in session_keys {
        let (workspace_key, task_id) = match parse_session_key(key) {
            Ok(parts) => parts,
            Err(message) => {
                result.failed.push(failure(key.clone(), message));
                continue;
            }
        };
        let archived: Result<i64, rusqlite::Error> = conn.query_row(
            "SELECT archived FROM tasks WHERE workspace_key = ?1 AND task_id = ?2 AND deleted = 0",
            rusqlite::params![workspace_key, task_id],
            |row| row.get(0),
        );
        match archived {
            Ok(flag) if flag != 0 => {
                result
                    .failed
                    .push(failure(key.clone(), "Already archived".to_string()));
            }
            Ok(_) => {
                match conn.execute(
                    "UPDATE tasks SET archived = 1, updated_at = ?1
                     WHERE workspace_key = ?2 AND task_id = ?3 AND deleted = 0 AND archived = 0",
                    rusqlite::params![now_ms, workspace_key, task_id],
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

pub(super) fn delete_at(session_keys: &[String], _mode: DeleteMode, _root: &Path) -> DeleteResult {
    DeleteResult {
        deleted_count: 0,
        freed_bytes: 0,
        failed: session_keys.iter().map(|key| failure(key.clone(), "ZCode deletion is unavailable: the task index and session database require coordinated cleanup")).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::zcode::discovery::{scan_zcode_at, tests::create_fixture_db};

    #[test]
    fn deletion_is_rejected_without_touching_index_or_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = create_fixture_db(&root.join("v2/tasks-index.sqlite"));
        conn.execute("INSERT INTO tasks (workspace_key,workspace_path,task_id,title,created_at,updated_at) VALUES ('/demo','/demo','sess_keep','Keep',1,2)", []).unwrap();
        std::fs::create_dir_all(root.join("cli/rollout")).unwrap();
        let artifact = root.join("cli/rollout/model-io-sess_keep.jsonl");
        std::fs::write(&artifact, "keep").unwrap();
        for mode in [DeleteMode::Trash, DeleteMode::Permanent] {
            let result = delete_at(&["/demo::sess_keep".into()], mode, root);
            assert_eq!(result.deleted_count, 0);
            assert_eq!(result.freed_bytes, 0);
            assert_eq!(result.failed.len(), 1);
            assert_eq!(std::fs::read_to_string(&artifact).unwrap(), "keep");
            assert_eq!(scan_zcode_at(root).sessions.len(), 1);
        }
    }

    #[test]
    fn archive_sets_flag() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = create_fixture_db(&root.join("v2/tasks-index.sqlite"));
        conn.execute(
            "INSERT INTO tasks (workspace_key, workspace_path, task_id, title, created_at, updated_at)
             VALUES ('/Users/zero/code/demo', '/Users/zero/code/demo', 'sess_dddd', 'Arch', 1, 2)",
            [],
        )
        .unwrap();
        drop(conn);

        let key = "/Users/zero/code/demo::sess_dddd".to_string();
        let result = archive_at(&[key.clone()], root);
        assert_eq!(result.archived_count, 1);
        let scan = scan_zcode_at(root);
        assert!(scan.sessions[0].archived);
        assert!(!scan.sessions[0].capabilities.archive);
        let again = archive_at(&[key], root);
        assert!(again.failed[0].message.contains("Already archived"));
    }
}
