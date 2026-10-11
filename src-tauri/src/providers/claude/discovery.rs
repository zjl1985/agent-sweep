use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::UNIX_EPOCH;

use walkdir::WalkDir;

use crate::models::{DirUsage, ProviderCapabilities, ProviderScan, SessionEntry};

use super::{CLAUDE_PROVIDER_ID, CLAUDE_PROVIDER_NAME};

const MAX_LINE_BYTES: usize = 512 * 1024;
const MAX_HISTORY_ENTRIES: usize = 50_000;
const MAX_USER_MESSAGE_LINES: usize = 40;
const MAX_TITLE_CHARS: usize = 120;

pub(super) fn scan_claude_at(root: &Path) -> ProviderScan {
    let mut result = ProviderScan::empty();
    let titles = load_history_titles(&root.join("history.jsonl"));
    let projects_root = root.join("projects");
    let mut sessions: Vec<SessionEntry> = Vec::new();

    if projects_root.is_dir() {
        collect_sessions(&projects_root, root, &titles, &mut sessions);
    } else {
        result.issues.push(format!(
            "Projects directory not found: {}",
            projects_root.display()
        ));
    }

    for session in &sessions {
        result.active_count += 1;
        result.active_bytes += session.size_bytes;
    }
    result.sessions_bytes = result.active_bytes;
    result.total_bytes = dir_size(root);
    result.other_dirs = scan_overview(root);
    sessions.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
    });
    result.sessions = sessions;
    result
}

fn collect_sessions(
    projects_root: &Path,
    claude_root: &Path,
    titles: &HashMap<String, String>,
    out: &mut Vec<SessionEntry>,
) {
    let project_dirs = match fs::read_dir(projects_root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in project_dirs.filter_map(|entry| entry.ok()) {
        let project_path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let project_name = match project_path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };
        let workspace = decode_project_path(project_name);
        collect_project_sessions(
            &project_path,
            claude_root,
            workspace.as_deref(),
            titles,
            out,
        );
    }
}

fn collect_project_sessions(
    project_path: &Path,
    claude_root: &Path,
    workspace: Option<&str>,
    titles: &HashMap<String, String>,
    out: &mut Vec<SessionEntry>,
) {
    let entries = match fs::read_dir(project_path) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        if name.ends_with(".desktop-released.json") || name == "memory" {
            continue;
        }
        let Some(stem) = name.strip_suffix(".jsonl") else {
            continue;
        };
        if !is_uuid(stem) {
            continue;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if !metadata.file_type().is_file() {
            continue;
        }
        let sibling = project_path.join(stem);
        let mut size_bytes = metadata.len();
        if sibling.is_dir() {
            let sibling_meta = match fs::symlink_metadata(&sibling) {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            if sibling_meta.file_type().is_dir() && !sibling_meta.file_type().is_symlink() {
                size_bytes += dir_size(&sibling);
            }
        }
        let updated_at_ms = mtime_ms(&metadata);
        let session_id = stem.to_string();
        let title = titles
            .get(&session_id)
            .cloned()
            .or_else(|| read_first_user_title(&path))
            .unwrap_or_else(|| fallback_title(&workspace.map(str::to_string), Some(updated_at_ms)));
        let session_key = match path.strip_prefix(claude_root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => format!("projects/{name}"),
        };
        out.push(SessionEntry {
            provider_id: CLAUDE_PROVIDER_ID.to_string(),
            provider_name: CLAUDE_PROVIDER_NAME.to_string(),
            session_key,
            capabilities: ProviderCapabilities {
                archive: false,
                delete_trash: true,
                delete_permanent: true,
            },
            session_id,
            title,
            workspace: workspace.map(str::to_string),
            created_at_ms: Some(updated_at_ms),
            updated_at_ms,
            size_bytes,
            archived: false,
            file_name: name,
        });
    }
}

/// Heuristic: Claude encodes cwd as a leading `-` then `/` → `-`.
/// Paths that originally contained `-` cannot round-trip perfectly.
pub fn decode_project_path(encoded: &str) -> Option<String> {
    if encoded.is_empty() {
        return None;
    }
    if encoded == "-" {
        return Some("/".to_string());
    }
    let stripped = encoded.strip_prefix('-')?;
    if stripped.is_empty() {
        return Some("/".to_string());
    }
    Some(format!("/{}", stripped.replace('-', "/")))
}

fn is_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    let bytes = value.as_bytes();
    for (idx, byte) in bytes.iter().enumerate() {
        if idx == 8 || idx == 13 || idx == 18 || idx == 23 {
            if *byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn load_history_titles(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return map,
    };
    let mut reader = BufReader::new(file);
    for _ in 0..MAX_HISTORY_ENTRIES {
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
        let session_id = value
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let display = value.get("display").and_then(|v| v.as_str()).unwrap_or("");
        if session_id.is_empty() || display.is_empty() {
            continue;
        }
        let trimmed_display = display.trim();
        if trimmed_display.is_empty() || trimmed_display.starts_with('/') {
            continue;
        }
        map.insert(session_id.to_string(), truncate_title(trimmed_display));
    }
    map
}

fn read_first_user_title(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    for _ in 0..MAX_USER_MESSAGE_LINES {
        let line = next_bounded_line(&mut reader, MAX_LINE_BYTES)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        let text = extract_user_text(&value)?;
        let cleaned = text.trim();
        if cleaned.is_empty() || cleaned.starts_with('/') {
            continue;
        }
        return Some(truncate_title(cleaned));
    }
    None
}

fn extract_user_text(value: &serde_json::Value) -> Option<String> {
    let message = value.get("message")?;
    if let Some(text) = message.as_str() {
        return Some(text.to_string());
    }
    let content = message.get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    if let Some(parts) = content.as_array() {
        let mut chunks = Vec::new();
        for part in parts {
            if let Some(text) = part.as_str() {
                chunks.push(text.to_string());
            } else if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    chunks.push(text.to_string());
                }
            }
        }
        if !chunks.is_empty() {
            return Some(chunks.join(" "));
        }
    }
    None
}

fn truncate_title(text: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= MAX_TITLE_CHARS {
            out.push('…');
            break;
        }
        if ch == '\n' || ch == '\r' {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn fallback_title(workspace: &Option<String>, created_at_ms: Option<i64>) -> String {
    let date = created_at_ms.map(format_short_date).unwrap_or_default();
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

fn next_bounded_line(reader: &mut impl BufRead, cap: usize) -> Option<String> {
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(_) => return None,
        };
        if available.is_empty() {
            return if buf.is_empty() {
                None
            } else {
                String::from_utf8(buf).ok()
            };
        }
        if let Some(pos) = available.iter().position(|&byte| byte == b'\n') {
            buf.extend_from_slice(&available[..pos]);
            reader.consume(pos + 1);
            if buf.len() > cap {
                return None;
            }
            return String::from_utf8(buf).ok();
        }
        if buf.len() + available.len() > cap {
            // Drain the oversized line so the next call starts cleanly.
            let take = available.len();
            reader.consume(take);
            loop {
                let available = match reader.fill_buf() {
                    Ok(available) => available,
                    Err(_) => break,
                };
                if available.is_empty() {
                    break;
                }
                if let Some(pos) = available.iter().position(|&byte| byte == b'\n') {
                    reader.consume(pos + 1);
                    break;
                }
                let take = available.len();
                reader.consume(take);
            }
            return None;
        }
        buf.extend_from_slice(available);
        let take = available.len();
        reader.consume(take);
    }
}

fn scan_overview(root: &Path) -> Vec<DirUsage> {
    let mut dirs = Vec::new();
    let history = root.join("history.jsonl");
    dirs.push(DirUsage {
        provider_id: CLAUDE_PROVIDER_ID.to_string(),
        provider_name: CLAUDE_PROVIDER_NAME.to_string(),
        name: "History".to_string(),
        size_bytes: file_size(&history),
        note: "history.jsonl".to_string(),
    });
    let read_only = [
        ("plugins", "Plugins"),
        ("file-history", "File history"),
        ("cache", "Cache"),
        ("backups", "Backups"),
        ("session-env", "Session env"),
        ("shell-snapshots", "Shell snapshots"),
        ("paste-cache", "Paste cache"),
        ("telemetry", "Telemetry"),
        ("debug", "Debug"),
        ("downloads", "Downloads"),
        ("stats-cache.json", "Stats cache"),
    ];
    for (name, label) in read_only {
        let path = root.join(name);
        let size = if path.is_file() {
            file_size(&path)
        } else {
            dir_size(&path)
        };
        dirs.push(DirUsage {
            provider_id: CLAUDE_PROVIDER_ID.to_string(),
            provider_name: CLAUDE_PROVIDER_NAME.to_string(),
            name: label.to_string(),
            size_bytes: size,
            note: format!("{name} (read-only)"),
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

    fn write_file(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn decode_project_path_heuristic() {
        assert_eq!(
            decode_project_path("-Users-zero-code-gd-skills").as_deref(),
            Some("/Users/zero/code/gd/skills")
        );
        assert_eq!(decode_project_path("-").as_deref(), Some("/"));
        assert_eq!(decode_project_path(""), None);
    }

    #[test]
    fn is_uuid_accepts_dashed_hex() {
        assert!(is_uuid("946808f4-74d1-463d-a5d7-b7e95f9c3794"));
        assert!(!is_uuid("not-a-uuid"));
        assert!(!is_uuid("946808f4-74d1-463d-a5d7-b7e95f9c3794.jsonl"));
    }

    #[test]
    fn scan_collects_uuid_jsonl_and_sibling_dir_size() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let project = root.join("projects/-Users-zero-code-demo");
        let uuid = "946808f4-74d1-463d-a5d7-b7e95f9c3794";
        write_file(
            &project.join(format!("{uuid}.jsonl")),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Hello from user\"}}\n",
        );
        write_file(&project.join(uuid).join("tool-results/a.txt"), "12345");
        write_file(
            &project.join("baae02c9-dda8-4a8b-a0ad-95a1e794944e.desktop-released.json"),
            "{}",
        );
        write_file(&project.join("memory/x.txt"), "ignore");
        write_file(
            &root.join("history.jsonl"),
            &format!(
                "{{\"sessionId\":\"{uuid}\",\"display\":\"/exit\"}}\n{{\"sessionId\":\"{uuid}\",\"display\":\"Prefer this title\"}}\n"
            ),
        );

        let result = scan_claude_at(root);
        assert!(result.issues.is_empty());
        assert_eq!(result.sessions.len(), 1);
        let session = &result.sessions[0];
        assert_eq!(session.session_id, uuid);
        assert_eq!(
            session.session_key,
            format!("projects/-Users-zero-code-demo/{uuid}.jsonl")
        );
        assert_eq!(session.title, "Prefer this title");
        assert_eq!(session.workspace.as_deref(), Some("/Users/zero/code/demo"));
        assert!(session.size_bytes > 5, "expected jsonl + sibling size, got {}", session.size_bytes);
        assert_eq!(session.size_bytes, fs::metadata(project.join(format!("{uuid}.jsonl"))).unwrap().len() + 5);
        assert!(!session.archived);
        assert!(!session.capabilities.archive);
    }

    #[test]
    fn scan_falls_back_to_user_message_title() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let uuid = "5cc15361-02d7-4251-ae19-137129971039";
        write_file(
            &root
                .join("projects/-Users-zero-code-demo")
                .join(format!("{uuid}.jsonl")),
            "{\"type\":\"mode\",\"mode\":\"default\"}\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"First real prompt\"}}\n",
        );
        let result = scan_claude_at(root);
        assert_eq!(result.sessions[0].title, "First real prompt");
    }

    #[cfg(unix)]
    #[test]
    fn scan_skips_symlink_session_files() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        let project = root.join("projects/-Users-zero-code-demo");
        fs::create_dir_all(&project).unwrap();
        let uuid = "849a5102-2a39-4ea5-b43b-b595181255d7";
        write_file(&outside.path().join("external.jsonl"), "precious");
        std::os::unix::fs::symlink(
            outside.path().join("external.jsonl"),
            project.join(format!("{uuid}.jsonl")),
        )
        .unwrap();
        let result = scan_claude_at(root);
        assert!(result.sessions.is_empty());
    }
}
