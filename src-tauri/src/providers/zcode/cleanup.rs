use std::fs;
use std::path::{Component, Path, PathBuf};

use walkdir::WalkDir;

use crate::models::{ArchiveResult, DeleteMode, DeleteResult, OperationFailure};

use super::discovery::artifact_paths;
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
    let db_path = root.join("v2/tasks-index.sqlite");
    let conn = match open_writable(&db_path) {
        Ok(conn) => conn,
        Err(error) => {
            result.failed.push(failure(String::new(), error));
            return result;
        }
    };
    let cli_root = root.join("cli");
    let allowed_cli = match fs::canonicalize(&cli_root) {
        Ok(path) => path,
        Err(_) => {
            // Index-only deletes are still allowed when cli/ is absent.
            PathBuf::new()
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
        let exists: Result<i64, rusqlite::Error> = conn.query_row(
            "SELECT deleted FROM tasks WHERE workspace_key = ?1 AND task_id = ?2",
            rusqlite::params![workspace_key, task_id],
            |row| row.get(0),
        );
        match exists {
            Ok(1) => {
                result
                    .failed
                    .push(failure(key.clone(), "Session already deleted".to_string()));
                continue;
            }
            Ok(0) => {}
            Ok(_) => {}
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                result
                    .failed
                    .push(failure(key.clone(), "Session not found".to_string()));
                continue;
            }
            Err(error) => {
                result
                    .failed
                    .push(failure(key.clone(), format!("Failed to delete: {error}")));
                continue;
            }
        }

        let freed = match remove_artifacts(root, &task_id, &allowed_cli, mode) {
            Ok(bytes) => bytes,
            Err(message) => {
                result.failed.push(failure(key.clone(), message));
                continue;
            }
        };

        let index_result = match mode {
            DeleteMode::Trash => conn.execute(
                "UPDATE tasks SET deleted = 1, updated_at = ?1
                 WHERE workspace_key = ?2 AND task_id = ?3 AND deleted = 0",
                rusqlite::params![now_ms, workspace_key, task_id],
            ),
            DeleteMode::Permanent => conn.execute(
                "DELETE FROM tasks WHERE workspace_key = ?1 AND task_id = ?2",
                rusqlite::params![workspace_key, task_id],
            ),
        };
        match index_result {
            Ok(0) => result
                .failed
                .push(failure(key.clone(), "Session not found".to_string())),
            Ok(_) => {
                result.deleted_count += 1;
                result.freed_bytes += freed;
            }
            Err(error) => result
                .failed
                .push(failure(key.clone(), format!("Failed to delete: {error}"))),
        }
    }
    result
}

fn remove_artifacts(
    root: &Path,
    task_id: &str,
    allowed_cli: &Path,
    mode: DeleteMode,
) -> Result<u64, String> {
    if allowed_cli.as_os_str().is_empty() {
        return Ok(0);
    }
    let mut freed = 0u64;
    for path in artifact_paths(root, task_id) {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Refusing to follow symbolic link artifact: {}",
                path.display()
            ));
        }
        let canonical = fs::canonicalize(&path)
            .map_err(|error| format!("Failed to resolve artifact path: {error}"))?;
        if !canonical.starts_with(allowed_cli) {
            return Err("Artifact path escapes ~/.zcode/cli".to_string());
        }
        let size = if metadata.is_file() {
            metadata.len()
        } else {
            dir_size(&path)
        };
        match mode {
            DeleteMode::Trash => trash::delete(&path)
                .map_err(|error| format!("Failed to move artifact to trash: {error}"))?,
            DeleteMode::Permanent => {
                if metadata.is_file() {
                    fs::remove_file(&path)
                        .map_err(|error| format!("Failed to delete artifact: {error}"))?;
                } else {
                    fs::remove_dir_all(&path)
                        .map_err(|error| format!("Failed to delete artifact directory: {error}"))?;
                }
            }
        }
        freed += size;
    }
    Ok(freed)
}

fn dir_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.metadata().map(|meta| meta.len()).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::zcode::discovery::{scan_zcode_at, tests::create_fixture_db};

    #[test]
    fn delete_soft_marks_and_removes_artifacts_permanently() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = create_fixture_db(&root.join("v2/tasks-index.sqlite"));
        conn.execute(
            "INSERT INTO tasks (workspace_key, workspace_path, task_id, title, created_at, updated_at)
             VALUES ('/Users/zero/code/demo', '/Users/zero/code/demo', 'sess_bbbb', 'Bye', 1, 2)",
            [],
        )
        .unwrap();
        drop(conn);
        fs::create_dir_all(root.join("cli/rollout")).unwrap();
        fs::write(root.join("cli/rollout/model-io-sess_bbbb.jsonl"), "hello").unwrap();
        fs::create_dir_all(root.join("cli/agents/sess_bbbb")).unwrap();
        fs::write(root.join("cli/agents/sess_bbbb/a.txt"), "xy").unwrap();

        let key = "/Users/zero/code/demo::sess_bbbb".to_string();
        let result = delete_at(&[key], DeleteMode::Permanent, root);
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.freed_bytes, 7);
        assert!(!root.join("cli/rollout/model-io-sess_bbbb.jsonl").exists());
        assert!(!root.join("cli/agents/sess_bbbb").exists());
        assert!(scan_zcode_at(root).sessions.is_empty());
    }

    #[test]
    fn trash_soft_deletes_index_row() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = create_fixture_db(&root.join("v2/tasks-index.sqlite"));
        conn.execute(
            "INSERT INTO tasks (workspace_key, workspace_path, task_id, title, created_at, updated_at)
             VALUES ('/Users/zero/code/demo', '/Users/zero/code/demo', 'sess_cccc', 'Trash', 1, 2)",
            [],
        )
        .unwrap();
        drop(conn);
        fs::create_dir_all(root.join("cli")).unwrap();

        let key = "/Users/zero/code/demo::sess_cccc".to_string();
        let result = delete_at(&[key], DeleteMode::Trash, root);
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert_eq!(result.deleted_count, 1);
        assert!(scan_zcode_at(root).sessions.is_empty());

        let conn = rusqlite::Connection::open(root.join("v2/tasks-index.sqlite")).unwrap();
        let deleted: i64 = conn
            .query_row(
                "SELECT deleted FROM tasks WHERE task_id = 'sess_cccc'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(deleted, 1);
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
