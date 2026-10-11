use std::fs;
use std::path::{Component, Path, PathBuf};

use walkdir::WalkDir;

use crate::models::{DeleteMode, DeleteResult, OperationFailure};

use super::CLAUDE_PROVIDER_ID;

pub(super) fn failure(
    session_key: impl Into<String>,
    message: impl Into<String>,
) -> OperationFailure {
    OperationFailure {
        provider_id: CLAUDE_PROVIDER_ID.to_string(),
        session_key: session_key.into(),
        message: message.into(),
    }
}

pub(super) fn delete_at(
    session_keys: &[String],
    mode: DeleteMode,
    claude_root: &Path,
) -> DeleteResult {
    let mut result = DeleteResult {
        deleted_count: 0,
        freed_bytes: 0,
        failed: Vec::new(),
    };
    let projects_root = claude_root.join("projects");
    let allowed_root = match fs::canonicalize(&projects_root) {
        Ok(path) => path,
        Err(_) => {
            result.failed.push(failure(
                String::new(),
                "No projects/ directory found".to_string(),
            ));
            return result;
        }
    };

    for rel in session_keys {
        let validated = resolve_rel_path(rel)
            .map(|rel_path| claude_root.join(rel_path))
            .and_then(|path| validate_session_file(&path, &allowed_root));
        match validated {
            Ok(validated) => match remove_session(&validated, mode) {
                Ok(freed) => {
                    result.deleted_count += 1;
                    result.freed_bytes += freed;
                    if let Some(parent) = validated.jsonl.parent() {
                        prune_empty_parents(parent, &projects_root);
                    }
                }
                Err(error) => result.failed.push(failure(rel.clone(), error.to_string())),
            },
            Err(message) => result.failed.push(failure(rel.clone(), message)),
        }
    }
    result
}

struct ValidatedSession {
    jsonl: PathBuf,
    sibling_dir: Option<PathBuf>,
    size_bytes: u64,
}

fn resolve_rel_path(rel: &str) -> Result<&Path, String> {
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
    if !rel.starts_with("projects/") {
        return Err("Path is outside the Claude projects directory".to_string());
    }
    Ok(path)
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

fn validate_session_file(path: &Path, allowed_root: &Path) -> Result<ValidatedSession, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let Some(stem) = name.strip_suffix(".jsonl") else {
        return Err("Only *.jsonl session files can be deleted".to_string());
    };
    if !is_uuid(stem) {
        return Err("Only UUID-named session jsonl files can be deleted".to_string());
    }
    if name.ends_with(".desktop-released.json") {
        return Err("Desktop-released artifacts cannot be deleted as sessions".to_string());
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("File not found: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("Target is not a regular file (symbolic link rejected)".to_string());
    }
    let canonical =
        fs::canonicalize(path).map_err(|error| format!("Failed to resolve path: {error}"))?;
    if !canonical.starts_with(allowed_root) {
        return Err("Path is outside the Claude projects directory".to_string());
    }

    let mut size_bytes = metadata.len();
    let sibling_candidate = path.with_file_name(stem);
    let sibling_dir = match fs::symlink_metadata(&sibling_candidate) {
        Ok(meta) if meta.file_type().is_dir() && !meta.file_type().is_symlink() => {
            let sibling_canonical = fs::canonicalize(&sibling_candidate)
                .map_err(|error| format!("Failed to resolve sibling directory: {error}"))?;
            if !sibling_canonical.starts_with(allowed_root) {
                return Err("Sibling directory is outside the Claude projects directory".to_string());
            }
            size_bytes += dir_size(&sibling_candidate);
            Some(sibling_candidate)
        }
        _ => None,
    };

    Ok(ValidatedSession {
        jsonl: path.to_path_buf(),
        sibling_dir,
        size_bytes,
    })
}

fn remove_session(session: &ValidatedSession, mode: DeleteMode) -> anyhow::Result<u64> {
    let size = session.size_bytes;
    if let Some(sibling) = &session.sibling_dir {
        remove_path(sibling, mode, true)?;
    }
    remove_path(&session.jsonl, mode, false)?;
    Ok(size)
}

fn remove_path(path: &Path, mode: DeleteMode, is_dir: bool) -> anyhow::Result<()> {
    match mode {
        DeleteMode::Trash => trash::delete(path)
            .map_err(|error| anyhow::anyhow!("Failed to move to trash: {error}")),
        DeleteMode::Permanent => {
            if is_dir {
                fs::remove_dir_all(path)
                    .map_err(|error| anyhow::anyhow!("Failed to delete directory: {error}"))
            } else {
                fs::remove_file(path)
                    .map_err(|error| anyhow::anyhow!("Failed to delete: {error}"))
            }
        }
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
    fn delete_permanently_removes_jsonl_and_sibling_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let uuid = "946808f4-74d1-463d-a5d7-b7e95f9c3794";
        let key = format!("projects/-Users-zero-code-demo/{uuid}.jsonl");
        let jsonl = root.join(&key);
        write_file(&jsonl, "hello");
        write_file(
            &root.join(format!("projects/-Users-zero-code-demo/{uuid}/tool-results/a.txt")),
            "12345",
        );
        write_file(
            &root.join("projects/-Users-zero-code-demo/memory/keep.txt"),
            "keep",
        );

        let result = delete_at(&[key], DeleteMode::Permanent, root);
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.freed_bytes, 10);
        assert!(!jsonl.exists());
        assert!(!root
            .join(format!("projects/-Users-zero-code-demo/{uuid}"))
            .exists());
        assert!(root
            .join("projects/-Users-zero-code-demo/memory/keep.txt")
            .exists());
        assert!(root.join("projects").exists());
    }

    #[test]
    fn delete_rejects_path_escape_and_non_uuid() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("projects")).unwrap();
        write_file(&root.join("plugins/evil.jsonl"), "nope");
        write_file(
            &root.join("projects/-Users-zero-code-demo/notes.jsonl"),
            "nope",
        );

        let result = delete_at(
            &[
                "projects/../../plugins/evil.jsonl".to_string(),
                "plugins/evil.jsonl".to_string(),
                "/etc/passwd".to_string(),
                "projects/-Users-zero-code-demo/notes.jsonl".to_string(),
            ],
            DeleteMode::Permanent,
            root,
        );
        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.failed.len(), 4);
        assert!(root.join("plugins/evil.jsonl").exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_never_follows_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        let uuid = "849a5102-2a39-4ea5-b43b-b595181255d7";
        let project = root.join("projects/-Users-zero-code-demo");
        fs::create_dir_all(&project).unwrap();
        let target = outside.path().join("precious.jsonl");
        fs::write(&target, "precious").unwrap();
        let link = project.join(format!("{uuid}.jsonl"));
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let result = delete_at(
            &[format!("projects/-Users-zero-code-demo/{uuid}.jsonl")],
            DeleteMode::Permanent,
            root,
        );
        assert_eq!(result.deleted_count, 0);
        assert!(result.failed[0].message.contains("symbolic link rejected"));
        assert_eq!(fs::read_to_string(target).unwrap(), "precious");
    }

    #[cfg(unix)]
    #[test]
    fn delete_rejects_parent_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        let uuid = "5cc15361-02d7-4251-ae19-137129971039";
        fs::create_dir_all(root.join("projects")).unwrap();
        fs::write(outside.path().join(format!("{uuid}.jsonl")), "precious").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("projects/escape")).unwrap();
        let key = format!("projects/escape/{uuid}.jsonl");
        let result = delete_at(&[key.clone()], DeleteMode::Permanent, root);
        assert_eq!(result.deleted_count, 0);
        assert!(result.failed[0]
            .message
            .contains("outside the Claude projects directory"));
        assert_eq!(
            fs::read_to_string(outside.path().join(format!("{uuid}.jsonl"))).unwrap(),
            "precious"
        );
    }
}
