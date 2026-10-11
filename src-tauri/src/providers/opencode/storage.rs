use rusqlite::{Connection, OpenFlags};
use std::{fs, path::Path, time::Duration};

pub(super) fn open_writable(path: &Path) -> Result<Connection, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("OpenCode database must be a regular file".to_string());
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| error.to_string())?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(|error| error.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| error.to_string())?;
    Ok(conn)
}

pub(super) fn has_events(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('event','event_sequence')",
        [], |row| row.get(0),
    )?;
    match count {
        0 => Ok(false),
        2 => Ok(true),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

// Match only known session aggregates, not other event streams or in-flight projections.
pub(super) const ORPHAN_GROUPS: &str = "SELECT DISTINCT e.aggregate_id FROM event e
    WHERE e.type = 'session.created.1' AND e.aggregate_id GLOB 'ses_*'
    AND NOT EXISTS (SELECT 1 FROM session s WHERE s.id = e.aggregate_id)";

pub(super) fn database_bytes(root: &Path) -> u64 {
    ["opencode.db", "opencode.db-wal", "opencode.db-shm"]
        .iter()
        .map(|name| fs::metadata(root.join(name)).map(|m| m.len()).unwrap_or(0))
        .sum()
}

pub(super) fn compact(conn: &Connection) -> Result<(), String> {
    checkpoint(conn)?;
    conn.execute_batch("VACUUM;")
        .map_err(|error| format!("Database compaction failed: {error}"))?;
    checkpoint(conn)
}

fn checkpoint(conn: &Connection) -> Result<(), String> {
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if busy != 0 {
        return Err(
            "Database is busy. Close OpenCode and retry reclaiming database space.".to_string(),
        );
    }
    Ok(())
}
