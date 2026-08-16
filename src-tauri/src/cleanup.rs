use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::discovery::{codex_home, session_id_from_file_name};
use crate::models::{DeleteFailure, DeleteMode, DeleteRequest, DeleteResult};

pub fn delete(request: DeleteRequest) -> DeleteResult {
    match codex_home() {
        Some(home) => delete_at(request, &home),
        None => DeleteResult {
            deleted_count: 0,
            freed_bytes: 0,
            failed: vec![DeleteFailure {
                session_id: String::new(),
                message: "Could not locate the user home directory".to_string(),
            }],
        },
    }
}

/// The frontend only sends session ids; the backend re-resolves them to files
/// inside the session roots and validates before deleting — paths never leave
/// the Rust side (same locator idea as baton).
pub fn delete_at(request: DeleteRequest, codex_root: &Path) -> DeleteResult {
    let mut result = DeleteResult {
        deleted_count: 0,
        freed_bytes: 0,
        failed: Vec::new(),
    };
    let sessions_root = codex_root.join("sessions");
    let archived_root = codex_root.join("archived_sessions");
    let allowed_roots: Vec<PathBuf> = [&sessions_root, &archived_root]
        .into_iter()
        .filter_map(|root| fs::canonicalize(root).ok())
        .collect();
    if allowed_roots.is_empty() {
        result.failed.push(DeleteFailure {
            session_id: String::new(),
            message: "No sessions/ or archived_sessions/ directory found".to_string(),
        });
        return result;
    }

    let wanted: HashSet<String> = request.session_ids.iter().cloned().collect();
    let targets = resolve_targets(&sessions_root, &archived_root, &wanted);

    for session_id in &request.session_ids {
        let path = match targets.get(session_id) {
            Some(path) => path,
            None => {
                result.failed.push(DeleteFailure {
                    session_id: session_id.clone(),
                    message: "No rollout file found for this session id".to_string(),
                });
                continue;
            }
        };
        match validate_session_file(path, &allowed_roots) {
            Ok(size) => match remove(path, request.mode) {
                Ok(()) => {
                    result.deleted_count += 1;
                    result.freed_bytes += size;
                    if let Some(parent) = path.parent() {
                        prune_empty_parents(parent, &sessions_root);
                    }
                }
                Err(error) => result.failed.push(DeleteFailure {
                    session_id: session_id.clone(),
                    message: error.to_string(),
                }),
            },
            Err(message) => result.failed.push(DeleteFailure {
                session_id: session_id.clone(),
                message,
            }),
        }
    }
    result
}

/// Walks the tree and derives ids from file names only; no file contents are
/// read, so resolution is fast.
fn resolve_targets(
    sessions_root: &Path,
    archived_root: &Path,
    wanted: &HashSet<String>,
) -> HashMap<String, PathBuf> {
    let mut targets = HashMap::new();
    for (root, depth) in [(sessions_root, 4usize), (archived_root, 1usize)] {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root)
            .max_depth(depth)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            // Symbolic links are included in resolution so the validation
            // layer rejects them with an explicit reason instead of a vague
            // "not found".
            if !(entry.file_type().is_file() || entry.file_type().is_symlink()) {
                continue;
            }
            let name = match entry.path().file_name().and_then(|name| name.to_str()) {
                Some(name) => name,
                None => continue,
            };
            if let Some(id) = session_id_from_file_name(name) {
                if wanted.contains(&id) {
                    targets
                        .entry(id)
                        .or_insert_with(|| entry.path().to_path_buf());
                }
            }
        }
    }
    targets
}

/// Pre-delete validation: only regular rollout-*.jsonl files directly under
/// sessions/ or archived_sessions/. symlink_metadata does not follow symbolic
/// links, and the canonicalize prefix check prevents path escapes.
fn validate_session_file(path: &Path, allowed_roots: &[PathBuf]) -> Result<u64, String> {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    if !(name.starts_with("rollout-") && name.ends_with(".jsonl")) {
        return Err("Only rollout-*.jsonl session files can be deleted".to_string());
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("File not found: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("Target is not a regular file (symbolic link rejected)".to_string());
    }
    let canonical =
        fs::canonicalize(path).map_err(|error| format!("Failed to resolve path: {error}"))?;
    if !allowed_roots.iter().any(|root| canonical.starts_with(root)) {
        return Err("Path is outside the Codex session directories".to_string());
    }
    Ok(metadata.len())
}

fn remove(path: &Path, mode: DeleteMode) -> anyhow::Result<()> {
    match mode {
        DeleteMode::Trash => {
            trash::delete(path).map_err(|error| anyhow::anyhow!("Failed to move to trash: {error}"))
        }
        DeleteMode::Permanent => fs::remove_file(path)
            .map_err(|error| anyhow::anyhow!("Failed to delete: {error}")),
    }
}

/// After deleting a file, prunes empty DD/MM/YYYY directories bottom-up, but
/// never removes the sessions root itself.
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

    fn write_rollout(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn active(name: &str) -> String {
        format!("sessions/2026/08/14/{name}")
    }

    #[test]
    fn delete_permanently_removes_files_and_prunes_empty_date_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join(active(
            "rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-000000000001.jsonl",
        ));
        write_rollout(&file, "hello");
        let kept = root.join("sessions/2026/08/15/rollout-2026-08-15T09-00-00-01a00000-0000-0000-0000-000000000002.jsonl");
        write_rollout(&kept, "keep me");

        let result = delete_at(
            DeleteRequest {
                session_ids: vec!["01a00000-0000-0000-0000-000000000001".to_string()],
                mode: DeleteMode::Permanent,
            },
            root,
        );

        assert!(result.failed.is_empty());
        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.freed_bytes, 5);
        assert!(!file.exists());
        assert!(kept.exists());
        // Empty 08/14 is pruned; 08/15 keeps its file; the YYYY dir and the
        // root itself stay.
        assert!(!root.join("sessions/2026/08/14").exists());
        assert!(root.join("sessions/2026/08/15").exists());
        assert!(root.join("sessions/2026").exists());
        assert!(root.join("sessions").exists());
    }

    #[test]
    fn delete_accounts_for_multiple_files_across_roots() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let a = root.join(active(
            "rollout-2026-01-01T00-00-00-01a00000-0000-0000-0000-00000000000a.jsonl",
        ));
        let b = root.join("archived_sessions/rollout-2026-01-02T00-00-00-01a00000-0000-0000-0000-00000000000b.jsonl");
        write_rollout(&a, "12345");
        write_rollout(&b, "12345678");

        let result = delete_at(
            DeleteRequest {
                session_ids: vec![
                    "01a00000-0000-0000-0000-00000000000a".to_string(),
                    "01a00000-0000-0000-0000-00000000000b".to_string(),
                ],
                mode: DeleteMode::Permanent,
            },
            root,
        );

        assert!(result.failed.is_empty());
        assert_eq!(result.deleted_count, 2);
        assert_eq!(result.freed_bytes, 13);
        assert!(!a.exists());
        assert!(!b.exists());
        // The archive root itself is kept.
        assert!(root.join("archived_sessions").exists());
    }

    #[test]
    fn delete_reports_unknown_session_ids() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sessions")).unwrap();

        let result = delete_at(
            DeleteRequest {
                session_ids: vec!["01a00000-0000-0000-0000-000000deadbeef".to_string()],
                mode: DeleteMode::Permanent,
            },
            root,
        );

        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.failed.len(), 1);
        assert!(result.failed[0].message.contains("No rollout file found"));
    }

    #[test]
    fn delete_ignores_files_that_are_not_rollout_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Same prefix but not valid session files; their ids never resolve,
        // so nothing is deleted.
        let notes = root.join(active("notes.txt"));
        let stray = root.join(active("rollout-short.jsonl"));
        write_rollout(&notes, "important");
        write_rollout(&stray, "stray");

        let result = delete_at(
            DeleteRequest {
                session_ids: vec![
                    "notes.txt".to_string(),
                    "short".to_string(),
                ],
                mode: DeleteMode::Permanent,
            },
            root,
        );

        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.failed.len(), 2);
        assert!(notes.exists());
        assert!(stray.exists());
    }

    #[test]
    fn delete_never_follows_rollout_shaped_symbolic_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = dir.path().join("precious.txt");
        fs::write(&target, "precious").unwrap();
        let link_dir = root.join(active(""));
        fs::create_dir_all(&link_dir).unwrap();
        let link = link_dir.join("rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-00000000000d.jsonl");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let result = delete_at(
            DeleteRequest {
                session_ids: vec!["01a00000-0000-0000-0000-00000000000d".to_string()],
                mode: DeleteMode::Permanent,
            },
            root,
        );

        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.failed.len(), 1);
        assert!(result.failed[0].message.contains("symbolic link rejected"));
        // The external file behind the symbolic link is intact.
        assert!(target.exists());
        let contents = fs::read_to_string(&target).unwrap();
        assert_eq!(contents, "precious");
    }

    #[test]
    fn delete_reports_missing_roots() {
        let dir = tempfile::tempdir().unwrap();
        let result = delete_at(
            DeleteRequest {
                session_ids: vec!["01a00000-0000-0000-0000-00000000000c".to_string()],
                mode: DeleteMode::Permanent,
            },
            dir.path(),
        );
        assert_eq!(result.deleted_count, 0);
        assert!(result.failed[0].message.contains("No sessions/ or archived_sessions"));
    }
}
