use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use walkdir::WalkDir;

use crate::models::{DirUsage, ProviderCapabilities, ProviderScan, SessionEntry};

use super::{KIMI_PROVIDER_ID, KIMI_PROVIDER_NAME};

const MAX_TITLE_CHARS: usize = 120;

pub(super) fn scan_kimi_at(root: &Path) -> ProviderScan {
    let mut result = ProviderScan::empty();
    let sessions_root = root.join("sessions");
    let mut sessions: Vec<SessionEntry> = Vec::new();

    if sessions_root.is_dir() {
        collect_sessions(&sessions_root, root, &mut sessions);
    } else {
        result.issues.push(format!(
            "Sessions directory not found: {}",
            sessions_root.display()
        ));
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

fn collect_sessions(sessions_root: &Path, kimi_root: &Path, out: &mut Vec<SessionEntry>) {
    for entry in WalkDir::new(sessions_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name() != "state.json" {
            continue;
        }
        let state_path = entry.path();
        let session_dir = match state_path.parent() {
            Some(parent) => parent,
            None => continue,
        };
        let session_dir_name = match session_dir.file_name().and_then(|n| n.to_str()) {
            Some(name) if name.starts_with("session_") => name,
            _ => continue,
        };
        // Skip index-cache / index-dirty marker dirs anywhere in the walk.
        if session_dir
            .components()
            .any(|component| matches!(component.as_os_str().to_str(), Some(".index-cache" | ".index-dirty")))
        {
            continue;
        }
        let workspace_dir = match session_dir.parent() {
            Some(parent) => parent,
            None => continue,
        };
        let workspace_name = match workspace_dir.file_name().and_then(|n| n.to_str()) {
            Some(name) if name.starts_with("wd_") => name,
            _ => continue,
        };
        let _ = (session_dir_name, workspace_name);

        let state = match read_state(state_path) {
            Some(state) => state,
            None => continue,
        };
        let metadata = match fs::symlink_metadata(session_dir) {
            Ok(meta) if meta.file_type().is_dir() && !meta.file_type().is_symlink() => meta,
            _ => continue,
        };
        let size_bytes = dir_size(session_dir);
        let updated_at_ms = state
            .updated_at
            .unwrap_or_else(|| mtime_ms(&metadata));
        let created_at_ms = state.created_at.or(Some(updated_at_ms));
        let title = state
            .last_prompt
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(truncate_title)
            .or_else(|| {
                state
                    .title
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(truncate_title)
            })
            .unwrap_or_else(|| fallback_title(state.cwd.as_ref(), created_at_ms));
        let session_key = match session_dir.strip_prefix(kimi_root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        let session_id = state
            .id
            .unwrap_or_else(|| session_dir_name.to_string());
        let archived = state.archived;
        out.push(SessionEntry {
            usage: None,
            provider_id: KIMI_PROVIDER_ID.to_string(),
            provider_name: KIMI_PROVIDER_NAME.to_string(),
            session_key,
            capabilities: ProviderCapabilities {
                archive: !archived,
                delete_trash: true,
                delete_permanent: true,
            },
            session_id,
            title,
            workspace: state.cwd,
            created_at_ms,
            updated_at_ms,
            size_bytes,
            archived,
            file_name: session_dir_name.to_string(),
        });
    }
}

struct StateInfo {
    id: Option<String>,
    cwd: Option<String>,
    created_at: Option<i64>,
    updated_at: Option<i64>,
    archived: bool,
    last_prompt: Option<String>,
    title: Option<String>,
}

fn read_state(path: &Path) -> Option<StateInfo> {
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(StateInfo {
        id: value
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        cwd: value
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        created_at: value.get("createdAt").and_then(|v| v.as_i64()),
        updated_at: value.get("updatedAt").and_then(|v| v.as_i64()),
        archived: value
            .get("archived")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        last_prompt: value
            .get("lastPrompt")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        title: value
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
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

fn fallback_title(workspace: Option<&String>, created_at_ms: Option<i64>) -> String {
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
    for (name, label) in [
        ("file-history", "File history"),
        ("cache", "Cache"),
        ("logs", "Logs"),
        ("user-history", "User history"),
        ("search-index", "Search index"),
        ("telemetry", "Telemetry"),
        ("image-upload-cache", "Image upload cache"),
    ] {
        dirs.push(DirUsage {
            provider_id: KIMI_PROVIDER_ID.to_string(),
            provider_name: KIMI_PROVIDER_NAME.to_string(),
            name: label.to_string(),
            size_bytes: dir_size(&root.join(name)),
            note: format!("{name}/ (read-only)"),
        });
    }
    let index = root.join("session_index.jsonl");
    dirs.push(DirUsage {
        provider_id: KIMI_PROVIDER_ID.to_string(),
        provider_name: KIMI_PROVIDER_NAME.to_string(),
        name: "Session index".to_string(),
        size_bytes: file_size(&index),
        note: "session_index.jsonl".to_string(),
    });
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
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn scan_reads_state_json_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let session_dir =
            root.join("sessions/wd_demo_abc123/session_32fc7d76-aa6d-4829-a683-b247410da508");
        write_file(
            &session_dir.join("state.json"),
            r#"{"id":"session_32fc7d76-aa6d-4829-a683-b247410da508","cwd":"/Users/zero/code/demo","createdAt":1000,"updatedAt":2000,"archived":false,"lastPrompt":"Do the thing"}"#,
        );
        write_file(&session_dir.join("agents/main/x.txt"), "payload");
        write_file(
            &root.join("sessions/wd_demo_abc123/.index-cache/state.json"),
            r#"{"id":"skip","archived":false}"#,
        );

        let result = scan_kimi_at(root);
        assert!(result.issues.is_empty());
        assert_eq!(result.sessions.len(), 1);
        let session = &result.sessions[0];
        assert_eq!(
            session.session_key,
            "sessions/wd_demo_abc123/session_32fc7d76-aa6d-4829-a683-b247410da508"
        );
        assert_eq!(session.title, "Do the thing");
        assert_eq!(session.workspace.as_deref(), Some("/Users/zero/code/demo"));
        assert!(session.size_bytes >= 7);
        assert!(!session.archived);
        assert!(session.capabilities.archive);
    }
}
