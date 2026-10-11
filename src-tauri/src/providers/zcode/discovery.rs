use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::models::{DirUsage, ProviderCapabilities, ProviderScan, SessionEntry};

use super::{ZCODE_PROVIDER_ID, ZCODE_PROVIDER_NAME};

pub(super) fn scan_zcode_at(root: &Path) -> ProviderScan {
    let mut result = ProviderScan::empty();
    let db_path = root.join("v2/tasks-index.sqlite");
    if !db_path.is_file() {
        result.issues.push(format!(
            "ZCode tasks index not found: {}",
            db_path.display()
        ));
        result.total_bytes = dir_size(root);
        result.other_dirs = scan_overview(root);
        return result;
    }

    let sessions = match load_sessions(&db_path, root) {
        Ok(sessions) => sessions,
        Err(error) => {
            result
                .issues
                .push(format!("Failed to read ZCode tasks index: {error}"));
            result.total_bytes = dir_size(root);
            result.other_dirs = scan_overview(root);
            return result;
        }
    };

    for session in &sessions {
        if session.archived {
            result.archived_count += 1;
            result.archived_bytes += session.size_bytes;
        } else {
            result.active_count += 1;
            result.active_bytes += session.size_bytes;
        }
    }
    result.sessions_bytes = result.active_bytes + result.archived_bytes;
    result.total_bytes = dir_size(root);
    result.other_dirs = scan_overview(root);
    let mut sessions = sessions;
    sessions.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
    });
    result.sessions = sessions;
    result
}

fn load_sessions(db_path: &Path, root: &Path) -> Result<Vec<SessionEntry>, String> {
    let uri = format!("file:{}?mode=ro", db_path.display());
    let conn = rusqlite::Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|error| error.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT workspace_key, task_id, title, workspace_path, archived, created_at, updated_at
             FROM tasks
             WHERE deleted = 0",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(|error| error.to_string())?;

    // task_id values are almost unique but not guaranteed; key with workspace.
    let mut sessions = Vec::new();
    for row in rows {
        let (workspace_key, task_id, title, workspace_path, archived_flag, created, updated) =
            row.map_err(|error| error.to_string())?;
        let archived = archived_flag != 0;
        let size_bytes = artifact_size(root, &task_id);
        let title = if title.trim().is_empty() {
            fallback_title(Some(&workspace_path), Some(created))
        } else {
            title
        };
        let session_key = format!("{workspace_key}::{task_id}");
        sessions.push(SessionEntry {
            usage: None,
            provider_id: ZCODE_PROVIDER_ID.to_string(),
            provider_name: ZCODE_PROVIDER_NAME.to_string(),
            session_key,
            capabilities: ProviderCapabilities {
                archive: !archived,
                delete_trash: false,
                delete_permanent: false,
            },
            session_id: task_id.clone(),
            title,
            workspace: Some(workspace_path),
            created_at_ms: Some(created),
            updated_at_ms: updated,
            size_bytes,
            archived,
            file_name: task_id,
        });
    }
    Ok(sessions)
}

pub(super) fn artifact_paths(root: &Path, task_id: &str) -> Vec<PathBuf> {
    vec![
        root.join(format!("cli/rollout/model-io-{task_id}.jsonl")),
        root.join(format!("cli/exec/{task_id}")),
        root.join(format!("cli/exec/bash-startup/{task_id}")),
        root.join(format!("cli/agents/{task_id}")),
        root.join(format!("cli/image-cache/{task_id}")),
        root.join(format!("cli/transcripts/{task_id}")),
    ]
}

fn artifact_size(root: &Path, task_id: &str) -> u64 {
    artifact_paths(root, task_id)
        .into_iter()
        .map(|path| {
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(_) => return 0,
            };
            if meta.file_type().is_symlink() {
                return 0;
            }
            if meta.is_file() {
                meta.len()
            } else if meta.is_dir() {
                dir_size(&path)
            } else {
                0
            }
        })
        .sum()
}

fn fallback_title(workspace: Option<&str>, created_at_ms: Option<i64>) -> String {
    let date = created_at_ms
        .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms))
        .map(|datetime| datetime.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let directory = workspace
        .map(Path::new)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    match (directory, date.is_empty()) {
        (Some(directory), false) => format!("{directory} · {date}"),
        (Some(directory), true) => directory.to_string(),
        (None, false) => date,
        (None, true) => "Untitled session".to_string(),
    }
}

fn scan_overview(root: &Path) -> Vec<DirUsage> {
    let mut dirs = Vec::new();
    let db = root.join("v2/tasks-index.sqlite");
    let db_bytes = [
        "v2/tasks-index.sqlite",
        "v2/tasks-index.sqlite-wal",
        "v2/tasks-index.sqlite-shm",
    ]
    .iter()
    .map(|name| file_size(&root.join(name)))
    .sum();
    dirs.push(DirUsage {
        provider_id: ZCODE_PROVIDER_ID.to_string(),
        provider_name: ZCODE_PROVIDER_NAME.to_string(),
        name: "Tasks index".to_string(),
        size_bytes: db_bytes,
        note: "v2/tasks-index.sqlite".to_string(),
    });
    let _ = db;
    for (directory, label) in [
        ("v2/checkpoints", "Checkpoints"),
        ("cli/agents", "Agents"),
        ("cli/log", "Logs"),
        ("cli/artifacts", "Artifacts"),
        ("computer-use", "Computer Use"),
    ] {
        dirs.push(DirUsage {
            provider_id: ZCODE_PROVIDER_ID.to_string(),
            provider_name: ZCODE_PROVIDER_NAME.to_string(),
            name: label.to_string(),
            size_bytes: dir_size(&root.join(directory)),
            note: format!("{directory}/ (read-only)"),
        });
    }
    dirs
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.metadata().map(|meta| meta.len()).unwrap_or(0))
        .sum()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rusqlite::Connection;

    pub(crate) fn create_fixture_db(path: &Path) -> Connection {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
               workspace_key TEXT NOT NULL,
               workspace_path TEXT NOT NULL,
               task_id TEXT NOT NULL,
               title TEXT NOT NULL DEFAULT '',
               mode TEXT NOT NULL DEFAULT 'build',
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL,
               pinned INTEGER NOT NULL DEFAULT 0,
               archived INTEGER NOT NULL DEFAULT 0,
               deleted INTEGER NOT NULL DEFAULT 0,
               title_overridden INTEGER NOT NULL DEFAULT 0,
               meta_json TEXT NOT NULL DEFAULT '{}',
               searchable_text TEXT NOT NULL DEFAULT '',
               last_unread_at INTEGER NOT NULL DEFAULT 0,
               PRIMARY KEY (workspace_key, task_id)
             );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn scan_skips_deleted_and_sums_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = create_fixture_db(&root.join("v2/tasks-index.sqlite"));
        conn.execute(
            "INSERT INTO tasks (workspace_key, workspace_path, task_id, title, created_at, updated_at, archived, deleted)
             VALUES ('/Users/zero/code/demo', '/Users/zero/code/demo', 'sess_aaaa', 'Hello', 1000, 2000, 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (workspace_key, workspace_path, task_id, title, created_at, updated_at, archived, deleted)
             VALUES ('/Users/zero/code/demo', '/Users/zero/code/demo', 'sess_gone', 'Gone', 1000, 2000, 0, 1)",
            [],
        )
        .unwrap();
        drop(conn);
        fs::create_dir_all(root.join("cli/rollout")).unwrap();
        fs::write(
            root.join("cli/rollout/model-io-sess_aaaa.jsonl"),
            "abcde",
        )
        .unwrap();
        fs::create_dir_all(root.join("cli/exec/sess_aaaa")).unwrap();
        fs::write(root.join("cli/exec/sess_aaaa/out.txt"), "123").unwrap();

        let result = scan_zcode_at(root);
        assert!(result.issues.is_empty(), "{:?}", result.issues);
        assert_eq!(result.sessions.len(), 1);
        let session = &result.sessions[0];
        assert_eq!(session.session_key, "/Users/zero/code/demo::sess_aaaa");
        assert_eq!(session.session_id, "sess_aaaa");
        assert_eq!(session.size_bytes, 8);
        assert_eq!(session.title, "Hello");
    }
}
