use std::fs;
use std::path::{Component, Path, PathBuf};

use walkdir::WalkDir;

use crate::models::{ArchiveResult, DeleteMode, DeleteResult, OperationFailure};

use super::KIMI_PROVIDER_ID;

pub(super) fn failure(
    session_key: impl Into<String>,
    message: impl Into<String>,
) -> OperationFailure {
    OperationFailure {
        provider_id: KIMI_PROVIDER_ID.to_string(),
        session_key: session_key.into(),
        message: message.into(),
    }
}

pub(super) fn archive_at(session_keys: &[String], root: &Path) -> ArchiveResult {
    let mut result = ArchiveResult {
        archived_count: 0,
        failed: Vec::new(),
    };
    let sessions_root = root.join("sessions");
    let allowed = match fs::canonicalize(&sessions_root) {
        Ok(path) => path,
        Err(_) => {
            result.failed.push(failure(
                String::new(),
                "No sessions/ directory found".to_string(),
            ));
            return result;
        }
    };

    for key in session_keys {
        let validated = resolve_session_dir(key, root, &allowed);
        match validated {
            Ok(session_dir) => match set_archived(&session_dir) {
                Ok(true) => result.archived_count += 1,
                Ok(false) => result
                    .failed
                    .push(failure(key.clone(), "Already archived".to_string())),
                Err(message) => result.failed.push(failure(key.clone(), message)),
            },
            Err(message) => result.failed.push(failure(key.clone(), message)),
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
    let sessions_root = root.join("sessions");
    let allowed = match fs::canonicalize(&sessions_root) {
        Ok(path) => path,
        Err(_) => {
            result.failed.push(failure(
                String::new(),
                "No sessions/ directory found".to_string(),
            ));
            return result;
        }
    };

    for key in session_keys {
        let validated = resolve_session_dir(key, root, &allowed);
        match validated {
            Ok(session_dir) => {
                let size = dir_size(&session_dir);
                match remove_dir(&session_dir, mode) {
                    Ok(()) => {
                        result.deleted_count += 1;
                        result.freed_bytes += size;
                        if let Some(parent) = session_dir.parent() {
                            prune_empty_parents(parent, &sessions_root);
                        }
                    }
                    Err(error) => result.failed.push(failure(key.clone(), error.to_string())),
                }
            }
            Err(message) => result.failed.push(failure(key.clone(), message)),
        }
    }
    result
}

fn resolve_session_dir(
    rel: &str,
    root: &Path,
    allowed: &Path,
) -> Result<PathBuf, String> {
    let path = Path::new(rel);
    if rel.is_empty() || path.is_absolute() {
        return Err("Invalid session path".to_string());
    }
    if !path
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("Invalid session path".to_string());
    }
    if !rel.starts_with("sessions/") {
        return Err("Path is outside the Kimi sessions directory".to_string());
    }
    let components: Vec<_> = path.components().collect();
    if components.len() < 3 {
        return Err("Invalid session path".to_string());
    }
    let wd = components[1]
        .as_os_str()
        .to_str()
        .unwrap_or("");
    let session = components[2]
        .as_os_str()
        .to_str()
        .unwrap_or("");
    if !wd.starts_with("wd_") || !session.starts_with("session_") {
        return Err("Invalid Kimi session directory layout".to_string());
    }
    if components.len() != 3 {
        return Err("Session key must point at the session directory".to_string());
    }

    let session_dir = root.join(path);
    let metadata = fs::symlink_metadata(&session_dir)
        .map_err(|error| format!("Session directory not found: {error}"))?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err("Target is not a real directory (symbolic link rejected)".to_string());
    }
    let canonical = fs::canonicalize(&session_dir)
        .map_err(|error| format!("Failed to resolve path: {error}"))?;
    if !canonical.starts_with(allowed) {
        return Err("Path is outside the Kimi sessions directory".to_string());
    }
    Ok(session_dir)
}

fn set_archived(session_dir: &Path) -> Result<bool, String> {
    let state_path = session_dir.join("state.json");
    let text = fs::read_to_string(&state_path)
        .map_err(|error| format!("Failed to read state.json: {error}"))?;
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("Failed to parse state.json: {error}"))?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "state.json root must be an object".to_string())?;
    if obj.get("archived").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(false);
    }
    obj.insert("archived".to_string(), serde_json::Value::Bool(true));
    let updated = serde_json::to_string(&value)
        .map_err(|error| format!("Failed to serialize state.json: {error}"))?;
    fs::write(&state_path, updated)
        .map_err(|error| format!("Failed to write state.json: {error}"))?;
    Ok(true)
}

fn remove_dir(path: &Path, mode: DeleteMode) -> anyhow::Result<()> {
    match mode {
        DeleteMode::Trash => {
            trash::delete(path).map_err(|error| anyhow::anyhow!("Failed to move to trash: {error}"))
        }
        DeleteMode::Permanent => fs::remove_dir_all(path)
            .map_err(|error| anyhow::anyhow!("Failed to delete: {error}")),
    }
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

fn prune_empty_parents(start: &Path, stop_root: &Path) {
    let stop = match fs::canonicalize(stop_root) {
        Ok(path) => path,
        Err(_) => return,
    };
    let mut current = match fs::canonicalize(start) {
        Ok(path) => path,
        Err(_) => return,
    };
    while current != stop && current.starts_with(&stop) {
        let is_empty = match fs::read_dir(&current) {
            Ok(mut entries) => entries.next().is_none(),
            Err(_) => break,
        };
        if !is_empty {
            break;
        }
        if fs::remove_dir(&current).is_err() {
            break;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn delete_removes_session_dir_and_prunes_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let key = "sessions/wd_demo_abc123/session_32fc7d76-aa6d-4829-a683-b247410da508";
        write_file(&root.join(key).join("state.json"), r#"{"archived":false}"#);
        write_file(&root.join(key).join("data.bin"), "12345");
        write_file(
            &root.join("sessions/wd_other_def456/session_aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa/state.json"),
            r#"{"archived":false}"#,
        );

        let result = delete_at(&[key.to_string()], DeleteMode::Permanent, root);
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.freed_bytes, 5 + r#"{"archived":false}"#.len() as u64);
        assert!(!root.join(key).exists());
        assert!(!root.join("sessions/wd_demo_abc123").exists());
        assert!(root.join("sessions").exists());
        assert!(root
            .join("sessions/wd_other_def456/session_aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
            .exists());
    }

    #[test]
    fn archive_sets_flag_in_state_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let key = "sessions/wd_demo_abc123/session_32fc7d76-aa6d-4829-a683-b247410da508";
        write_file(
            &root.join(key).join("state.json"),
            r#"{"id":"session_32fc7d76-aa6d-4829-a683-b247410da508","archived":false}"#,
        );
        let result = archive_at(&[key.to_string()], root);
        assert_eq!(result.archived_count, 1);
        let state: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(key).join("state.json")).unwrap())
                .unwrap();
        assert_eq!(state["archived"], true);
        let again = archive_at(&[key.to_string()], root);
        assert_eq!(again.archived_count, 0);
        assert!(again.failed[0].message.contains("Already archived"));
    }

    #[cfg(unix)]
    #[test]
    fn delete_rejects_symlink_session_dir() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sessions/wd_demo_abc123")).unwrap();
        fs::write(outside.path().join("state.json"), "{}").unwrap();
        std::os::unix::fs::symlink(
            outside.path(),
            root.join("sessions/wd_demo_abc123/session_32fc7d76-aa6d-4829-a683-b247410da508"),
        )
        .unwrap();
        let result = delete_at(
            &["sessions/wd_demo_abc123/session_32fc7d76-aa6d-4829-a683-b247410da508".to_string()],
            DeleteMode::Permanent,
            root,
        );
        assert_eq!(result.deleted_count, 0);
        assert!(result.failed[0].message.contains("symbolic link rejected"));
        assert!(outside.path().join("state.json").exists());
    }
}
