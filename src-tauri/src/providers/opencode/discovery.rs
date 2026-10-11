use std::fs;
use std::path::Path;

use walkdir::WalkDir;

use crate::models::{DirUsage, ProviderCapabilities, ProviderScan, SessionEntry};

use super::{OPENCODE_PROVIDER_ID, OPENCODE_PROVIDER_NAME};

pub(super) fn scan_opencode_at(root: &Path) -> ProviderScan {
    let mut result = ProviderScan::empty();
    let db_path = root.join("opencode.db");
    if !db_path.is_file() {
        result.issues.push(format!(
            "OpenCode database not found: {}",
            db_path.display()
        ));
        result.total_bytes = dir_size(root);
        result.other_dirs = scan_overview(root);
        return result;
    }

    let sessions = match load_sessions(&db_path) {
        Ok(sessions) => sessions,
        Err(error) => {
            result.issues.push(format!("Failed to read OpenCode database: {error}"));
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

fn load_sessions(db_path: &Path) -> Result<Vec<SessionEntry>, String> {
    let conn = open_readonly(db_path)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, directory, time_created, time_updated, time_archived
             FROM session",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })
        .map_err(|error| error.to_string())?;

    let mut sessions = Vec::new();
    for row in rows {
        let (id, title, directory, created, updated, archived_at) =
            row.map_err(|error| error.to_string())?;
        let archived = archived_at.is_some();
        let size_bytes = session_size_bytes(&conn, &id).unwrap_or(0);
        let title = if title.trim().is_empty() {
            fallback_title(Some(&directory), Some(created))
        } else {
            title
        };
        sessions.push(SessionEntry {
            provider_id: OPENCODE_PROVIDER_ID.to_string(),
            provider_name: OPENCODE_PROVIDER_NAME.to_string(),
            session_key: id.clone(),
            capabilities: ProviderCapabilities {
                archive: !archived,
                delete_trash: false,
                delete_permanent: true,
            },
            session_id: id.clone(),
            title,
            workspace: Some(directory),
            created_at_ms: Some(created),
            updated_at_ms: updated,
            size_bytes,
            archived,
            file_name: id,
        });
    }
    Ok(sessions)
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

pub(super) fn open_readonly(db_path: &Path) -> Result<rusqlite::Connection, String> {
    let uri = format!("file:{}?mode=ro", db_path.display());
    let conn = rusqlite::Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|error| error.to_string())?;
    Ok(conn)
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
    let db = root.join("opencode.db");
    let db_bytes = ["opencode.db", "opencode.db-wal", "opencode.db-shm"]
        .iter()
        .map(|name| file_size(&root.join(name)))
        .sum();
    dirs.push(DirUsage {
        provider_id: OPENCODE_PROVIDER_ID.to_string(),
        provider_name: OPENCODE_PROVIDER_NAME.to_string(),
        name: "Database".to_string(),
        size_bytes: db_bytes,
        note: "opencode.db".to_string(),
    });
    let _ = db;
    for (directory, label) in [
        ("snapshot", "Snapshots"),
        ("tool-output", "Tool output"),
        ("log", "Logs"),
        ("storage", "Storage"),
    ] {
        dirs.push(DirUsage {
            provider_id: OPENCODE_PROVIDER_ID.to_string(),
            provider_name: OPENCODE_PROVIDER_NAME.to_string(),
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
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE session (
               id TEXT PRIMARY KEY,
               project_id TEXT NOT NULL DEFAULT 'p',
               slug TEXT NOT NULL DEFAULT 's',
               directory TEXT NOT NULL,
               title TEXT NOT NULL,
               version TEXT NOT NULL DEFAULT '1',
               cost REAL NOT NULL DEFAULT 0,
               tokens_input INTEGER NOT NULL DEFAULT 0,
               tokens_output INTEGER NOT NULL DEFAULT 0,
               tokens_reasoning INTEGER NOT NULL DEFAULT 0,
               tokens_cache_read INTEGER NOT NULL DEFAULT 0,
               tokens_cache_write INTEGER NOT NULL DEFAULT 0,
               time_created INTEGER NOT NULL,
               time_updated INTEGER NOT NULL,
               time_archived INTEGER
             );
             CREATE TABLE message (
               id TEXT PRIMARY KEY,
               session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
               time_created INTEGER NOT NULL,
               time_updated INTEGER NOT NULL,
               data TEXT NOT NULL
             );
             CREATE TABLE part (
               id TEXT PRIMARY KEY,
               message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
               session_id TEXT NOT NULL,
               time_created INTEGER NOT NULL,
               time_updated INTEGER NOT NULL,
               data TEXT NOT NULL
             );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn scan_reads_sessions_sizes_and_archive_flag() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = root.join("opencode.db");
        let conn = create_fixture_db(&db);
        conn.execute(
            "INSERT INTO session (id, directory, title, time_created, time_updated, time_archived)
             VALUES (?1, ?2, ?3, 1000, 2000, NULL)",
            rusqlite::params![
                "ses_active",
                "/Users/zero/code/demo",
                "Active session",
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session (id, directory, title, time_created, time_updated, time_archived)
             VALUES (?1, ?2, ?3, 1000, 1500, 1800)",
            rusqlite::params![
                "ses_archived",
                "/Users/zero/code/demo",
                "Old session",
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES ('msg1', 'ses_active', 1, 1, 'hello')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('part1', 'msg1', 'ses_active', 1, 1, 'world')",
            [],
        )
        .unwrap();
        drop(conn);

        let result = scan_opencode_at(root);
        assert!(result.issues.is_empty(), "{:?}", result.issues);
        assert_eq!(result.sessions.len(), 2);
        assert_eq!(result.active_count, 1);
        assert_eq!(result.archived_count, 1);
        let active = result
            .sessions
            .iter()
            .find(|s| s.session_id == "ses_active")
            .unwrap();
        assert_eq!(active.title, "Active session");
        assert_eq!(active.size_bytes, 10); // hello + world
        assert!(active.capabilities.archive);
        let archived = result
            .sessions
            .iter()
            .find(|s| s.session_id == "ses_archived")
            .unwrap();
        assert!(!archived.capabilities.archive);
        assert!(archived.archived);
    }
}
