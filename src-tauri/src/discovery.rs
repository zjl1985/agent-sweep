use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use walkdir::WalkDir;

use crate::models::{DirUsage, ScanResult, SessionEntry};

/// Byte budget for a single metadata line so abnormally huge lines cannot
/// stall the scan (same approach as baton).
const MAX_LINE_BYTES: usize = 512 * 1024;
/// How many leading lines to scan when looking for session_meta.
const MAX_METADATA_LINES: usize = 8;
/// Maximum number of entries read from session_index.jsonl.
const MAX_INDEX_ENTRIES: usize = 10_000;

pub fn codex_home() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".codex"))
}

pub fn scan_all() -> ScanResult {
    let now_ms = chrono::Utc::now().timestamp_millis();
    match codex_home() {
        Some(home) => scan_codex_at(&home, now_ms),
        None => {
            let mut result = ScanResult::empty(now_ms);
            result
                .issues
                .push("Could not locate the user home directory".to_string());
            result
        }
    }
}

pub fn scan_codex_at(root: &Path, now_ms: i64) -> ScanResult {
    let mut result = ScanResult::empty(now_ms);
    let titles = load_title_index(&root.join("session_index.jsonl"));
    let mut sessions: Vec<SessionEntry> = Vec::new();

    let sessions_root = root.join("sessions");
    if sessions_root.is_dir() {
        collect_rollout_files(&sessions_root, 4, false, &titles, &mut sessions);
    } else {
        result
            .issues
            .push(format!("Sessions directory not found: {}", sessions_root.display()));
    }

    // A missing archive directory is normal, not an issue.
    let archived_root = root.join("archived_sessions");
    if archived_root.is_dir() {
        collect_rollout_files(&archived_root, 1, true, &titles, &mut sessions);
    }

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
    result.codex_total_bytes = dir_size(root);
    result.other_dirs = scan_overview(root);
    sessions.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
    });
    result.sessions = sessions;
    result
}

fn collect_rollout_files(
    root: &Path,
    depth: usize,
    archived: bool,
    titles: &HashMap<String, String>,
    out: &mut Vec<SessionEntry>,
) {
    for entry in WalkDir::new(root)
        .max_depth(depth)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name,
            None => continue,
        };
        if !(name.starts_with("rollout-") && name.ends_with(".jsonl")) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let updated_at_ms = mtime_ms(&metadata);
        let meta = read_session_meta(path);
        let session_id = meta
            .as_ref()
            .and_then(|meta| meta.session_id.clone())
            .or_else(|| session_id_from_file_name(name))
            .unwrap_or_else(|| name.to_string());
        let workspace = meta.as_ref().and_then(|meta| meta.cwd.clone());
        let created_at_ms = meta
            .as_ref()
            .and_then(|meta| meta.timestamp_ms)
            .or_else(|| created_ms_from_file_name(name))
            .or(Some(updated_at_ms));
        let title = titles
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| fallback_title(&workspace, created_at_ms));
        out.push(SessionEntry {
            session_id,
            title,
            workspace,
            created_at_ms,
            updated_at_ms,
            size_bytes: metadata.len(),
            archived,
            file_name: name.to_string(),
        });
    }
}

struct SessionMeta {
    session_id: Option<String>,
    cwd: Option<String>,
    timestamp_ms: Option<i64>,
}

/// Reads the first few lines of a rollout file to extract session_meta
/// (session_id / cwd / UTC timestamp), bounded by the byte budget.
fn read_session_meta(path: &Path) -> Option<SessionMeta> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    for _ in 0..MAX_METADATA_LINES {
        let line = next_bounded_line(&mut reader, MAX_LINE_BYTES)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("type").and_then(|kind| kind.as_str()) != Some("session_meta") {
            continue;
        }
        let payload = value.get("payload")?.as_object()?;
        // The timestamp lives at the line top level; the payload copy is
        // older, kept as a fallback.
        let timestamp_ms = value
            .get("timestamp")
            .and_then(|field| field.as_str())
            .and_then(parse_iso_ms)
            .or_else(|| {
                payload
                    .get("timestamp")
                    .and_then(|field| field.as_str())
                    .and_then(parse_iso_ms)
            });
        return Some(SessionMeta {
            session_id: payload
                .get("session_id")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            cwd: payload
                .get("cwd")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            timestamp_ms,
        });
    }
    None
}

/// Reads one line; gives up (returns None) past `cap` bytes so a single
/// giant line cannot stall the scan.
fn next_bounded_line(reader: &mut impl Read, cap: usize) -> Option<String> {
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut chunk = [0u8; 8 * 1024];
    loop {
        let read = reader.read(&mut chunk).ok()?;
        if read == 0 {
            return if buf.is_empty() {
                None
            } else {
                String::from_utf8(buf).ok()
            };
        }
        if let Some(pos) = chunk[..read]
            .iter()
            .position(|&byte| byte == b'\n')
        {
            buf.extend_from_slice(&chunk[..pos]);
            return String::from_utf8(buf).ok();
        }
        buf.extend_from_slice(&chunk[..read]);
        if buf.len() > cap {
            return None;
        }
    }
}

/// `rollout-2026-08-15T10-54-49-<uuid>.jsonl` → `<uuid>`.
/// The timestamp part is a fixed 19 chars + one dash, and a dashed UUID v7
/// is 36 chars, for a total of 56.
pub fn session_id_from_file_name(name: &str) -> Option<String> {
    let stem = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    if stem.len() == 56 {
        Some(stem[20..].to_string())
    } else {
        None
    }
}

/// The filename timestamp is local time; only used as the created-at
/// fallback when the first-line metadata is missing.
fn created_ms_from_file_name(name: &str) -> Option<i64> {
    use chrono::TimeZone;
    let stem = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    if stem.len() < 19 {
        return None;
    }
    let naive =
        chrono::NaiveDateTime::parse_from_str(&stem[..19], "%Y-%m-%dT%H-%M-%S").ok()?;
    chrono::Local
        .from_local_datetime(&naive)
        .single()
        .map(|datetime| datetime.timestamp_millis())
}

fn parse_iso_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|datetime| datetime.timestamp_millis())
}

fn fallback_title(workspace: &Option<String>, created_at_ms: Option<i64>) -> String {
    let date = created_at_ms
        .map(format_short_date)
        .unwrap_or_default();
    let directory = workspace
        .as_deref()
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

fn format_short_date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|datetime| datetime.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

fn load_title_index(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return map,
    };
    let mut reader = BufReader::new(file);
    for _ in 0..MAX_INDEX_ENTRIES {
        let line = match next_bounded_line(&mut reader, MAX_LINE_BYTES) {
            Some(line) => line,
            None => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let id = value.get("id").and_then(|value| value.as_str());
        let name = value.get("thread_name").and_then(|value| value.as_str());
        if let (Some(id), Some(name)) = (id, name) {
            // Later entries win (the index is append-mostly, newest last).
            map.insert(id.to_string(), name.to_string());
        }
    }
    map
}

fn scan_overview(codex_root: &Path) -> Vec<DirUsage> {
    let mut dirs = Vec::new();
    let logs_bytes = [
        "logs_2.sqlite",
        "logs_2.sqlite-wal",
        "logs_2.sqlite-shm",
    ]
    .iter()
    .map(|file| file_size(&codex_root.join(file)))
    .sum();
    dirs.push(DirUsage {
        name: "Log database".to_string(),
        size_bytes: logs_bytes,
        note: "logs_2.sqlite".to_string(),
    });
    let read_only = [
        ("plugins", "Plugins"),
        ("generated_images", "Generated images"),
        ("computer-use", "Computer Use"),
        ("sqlite", "Legacy database"),
        ("cache", "Cache"),
    ];
    for (directory, name) in read_only {
        dirs.push(DirUsage {
            name: name.to_string(),
            size_bytes: dir_size(&codex_root.join(directory)),
            note: format!("{directory}/ (read-only)"),
        });
    }
    dirs
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
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

fn mtime_ms(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn session_id_from_file_name_extracts_uuid() {
        let name = "rollout-2026-08-15T10-54-49-01a00357-efb0-7db0-8d4f-272a55f4431c.jsonl";
        assert_eq!(
            session_id_from_file_name(name).as_deref(),
            Some("01a00357-efb0-7db0-8d4f-272a55f4431c")
        );
        assert_eq!(session_id_from_file_name("notes.txt"), None);
    }

    #[test]
    fn created_ms_from_file_name_uses_local_time() {
        let name = "rollout-2026-08-15T10-54-49-01a00357-efb0-7db0-8d4f-272a55f4431c.jsonl";
        let ms = created_ms_from_file_name(name).unwrap();
        let formatted = format_short_date(ms);
        assert_eq!(formatted, "2026-08-15");
    }

    #[test]
    fn read_session_meta_parses_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-x.jsonl");
        let meta_line = r#"{"timestamp":"2026-08-15T02:54:49.001Z","type":"session_meta","payload":{"session_id":"01a00357-efb0-7db0-8d4f-272a55f4431c","cwd":"/Users/zero/code/baton"}}"#;
        write_file(
            &path,
            &format!("{meta_line}
{{\"type\":\"event_msg\"}}
"),
        );
        let meta = read_session_meta(&path).unwrap();
        assert_eq!(
            meta.session_id.as_deref(),
            Some("01a00357-efb0-7db0-8d4f-272a55f4431c")
        );
        assert_eq!(meta.cwd.as_deref(), Some("/Users/zero/code/baton"));
        assert_eq!(meta.timestamp_ms, Some(1_786_762_489_001));
    }

    #[test]
    fn read_session_meta_tolerates_missing_meta() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-y.jsonl");
        write_file(&path, "{\"type\":\"event_msg\"}
");
        assert!(read_session_meta(&path).is_none());
    }

    #[test]
    fn read_session_meta_gives_up_on_oversized_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-big.jsonl");
        let mut file = File::create(&path).unwrap();
        let huge = "x".repeat(MAX_LINE_BYTES + 1);
        writeln!(file, "{huge}").unwrap();
        drop(file);
        assert!(read_session_meta(&path).is_none());
    }

    #[test]
    fn scan_codex_at_collects_and_sorts_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let name_a = "rollout-2026-08-15T10-54-49-01a00357-efb0-7db0-8d4f-272a55f4431c.jsonl";
        let name_b = "rollout-2026-08-16T08-24-21-01a007f4-86f8-7903-b3f3-201554f50580.jsonl";
        let name_c = "rollout-2026-07-01T09-00-00-01a00000-0000-0000-0000-000000000000.jsonl";
        let meta_b = r#"{"timestamp":"2026-08-16T00:24:21.001Z","type":"session_meta","payload":{"session_id":"01a007f4-86f8-7903-b3f3-201554f50580","cwd":"/Users/zero/code/baton"}}"#;
        write_file(
            &root.join("sessions/2026/08/15").join(name_a),
            "{\"type\":\"event_msg\"}
",
        );
        write_file(
            &root.join("sessions/2026/08/16").join(name_b),
            &format!("{meta_b}
"),
        );
        write_file(&root.join("archived_sessions").join(name_c), "junk
");
        write_file(
            &root.join("session_index.jsonl"),
            "{\"id\":\"01a007f4-86f8-7903-b3f3-201554f50580\",\"thread_name\":\"Fix login bug\"}
",
        );

        let result = scan_codex_at(root, 0);
        assert!(result.issues.is_empty());
        assert_eq!(result.sessions.len(), 3);
        assert_eq!(result.active_count, 2);
        assert_eq!(result.archived_count, 1);
        assert!(result.sessions_bytes > 0);

        // Sorted by size, descending.
        let sizes: Vec<u64> = result.sessions.iter().map(|s| s.size_bytes).collect();
        assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]));

        let by_id: HashMap<&str, &SessionEntry> = result
            .sessions
            .iter()
            .map(|session| (session.session_id.as_str(), session))
            .collect();
        let b = by_id["01a007f4-86f8-7903-b3f3-201554f50580"];
        assert_eq!(b.title, "Fix login bug");
        assert_eq!(b.workspace.as_deref(), Some("/Users/zero/code/baton"));
        assert_eq!(b.created_at_ms, Some(1_786_839_861_001));
        assert!(!b.archived);
        let c = by_id["01a00000-0000-0000-0000-000000000000"];
        assert!(c.archived);
        // Without an index entry or metadata, fall back to the filename
        // timestamp for the title.
        assert!(c.title.contains("2026-07-01"));
    }

    #[test]
    #[ignore = "Manual diagnostic: run on a machine with real Codex data"]
    fn scan_real_codex_home() {
        let result = scan_all();
        println!(
            "sessions={} active={} archived={} bytes={} codex_total={} issues={:?}",
            result.sessions.len(),
            result.active_count,
            result.archived_count,
            result.sessions_bytes,
            result.codex_total_bytes,
            result.issues
        );
        for session in result.sessions.iter().take(5) {
            println!(
                "  {:>10}  {}  {}",
                session.size_bytes, session.title, session.file_name
            );
        }
        assert!(
            !result.sessions.is_empty(),
            "this machine should have Codex session data"
        );
    }

    #[test]
    fn scan_codex_at_reports_missing_sessions_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = scan_codex_at(dir.path(), 0);
        assert_eq!(result.sessions.len(), 0);
        assert_eq!(result.issues.len(), 1);
        assert!(result.issues[0].contains("Sessions directory not found"));
    }

    #[test]
    fn scan_overview_sums_sqlite_sidecar_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_file(&root.join("logs_2.sqlite"), "aaaa");
        write_file(&root.join("logs_2.sqlite-wal"), "bb");
        write_file(&root.join("cache/a.bin"), "cccccc");
        let overview = scan_overview(root);
        let logs = &overview[0];
        assert_eq!(logs.name, "Log database");
        assert_eq!(logs.size_bytes, 6);
        let cache = overview
            .iter()
            .find(|entry| entry.name == "Cache")
            .unwrap();
        assert_eq!(cache.size_bytes, 6);
    }
}
