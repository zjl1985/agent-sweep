use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::models::{ArchiveResult, DeleteMode, DeleteResult, OperationFailure};

use super::CODEX_PROVIDER_ID;

fn get_rollout_companion_paths(
    codex_root: &Path,
    path: &Path,
    session_id: Option<&str>,
) -> Vec<PathBuf> {
    let mut files = vec![path.to_path_buf()];
    let s = path.to_string_lossy();
    if s.ends_with(".jsonl.zst") {
        if let Some(base) = s.strip_suffix(".zst") {
            files.push(PathBuf::from(base));
        }
    } else if s.ends_with(".jsonl") {
        files.push(PathBuf::from(format!("{s}.zst")));
    }
    if let Some(id) = session_id {
        let img_dir = codex_root.join("generated_images").join(id);
        if img_dir.is_dir() {
            files.push(img_dir);
        }
    }
    files
}

fn cleanup_sqlite_records(codex_root: &Path, session_id: &str) {
    let history_db = codex_root.join("thread_history_1.sqlite");
    if history_db.is_file() {
        if let Ok(conn) = rusqlite::Connection::open(&history_db) {
            let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
            let _ = conn.execute(
                "DELETE FROM thread_items WHERE thread_id = ?1",
                rusqlite::params![session_id],
            );
            let _ = conn.execute(
                "DELETE FROM thread_realtime_items WHERE thread_id = ?1",
                rusqlite::params![session_id],
            );
            let _ = conn.execute(
                "DELETE FROM thread_turns WHERE thread_id = ?1",
                rusqlite::params![session_id],
            );
            let _ = conn.execute(
                "DELETE FROM thread_history_projection_state WHERE thread_id = ?1",
                rusqlite::params![session_id],
            );
        }
    }

    let state_db = codex_root.join("state_5.sqlite");
    if state_db.is_file() {
        if let Ok(conn) = rusqlite::Connection::open(&state_db) {
            let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
            let _ = conn.execute(
                "DELETE FROM thread_dynamic_tools WHERE thread_id = ?1",
                rusqlite::params![session_id],
            );
            let _ = conn.execute(
                "DELETE FROM thread_read_receipts WHERE thread_id = ?1",
                rusqlite::params![session_id],
            );
            let _ = conn.execute(
                "DELETE FROM threads WHERE id = ?1",
                rusqlite::params![session_id],
            );
        }
    }
}

pub(super) fn failure(
    session_key: impl Into<String>,
    message: impl Into<String>,
) -> OperationFailure {
    OperationFailure {
        provider_id: CODEX_PROVIDER_ID.to_string(),
        session_key: session_key.into(),
        message: message.into(),
    }
}

/// The adapter only accepts opaque keys previously reported by the scan. For
/// Codex those keys are paths relative to the Codex home; every key is
/// validated again here before any filesystem operation.
pub(super) fn delete_at(
    session_keys: &[String],
    mode: DeleteMode,
    codex_root: &Path,
) -> DeleteResult {
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
        result.failed.push(failure(
            String::new(),
            "No sessions/ or archived_sessions/ directory found".to_string(),
        ));
        return result;
    }

    for rel in session_keys {
        let validated = resolve_rel_path(rel)
            .map(|rel_path| codex_root.join(rel_path))
            .and_then(|path| validate_session_file(&path, &allowed_roots).map(|_| path));
        match validated {
            Ok(path) => {
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                let session_id = super::discovery::session_id_from_file_name(file_name);
                let companion_paths =
                    get_rollout_companion_paths(codex_root, &path, session_id.as_deref());

                match super::super::files::remove_group(
                    codex_root,
                    &companion_paths,
                    mode,
                ) {
                    Ok(size) => {
                        result.deleted_count += 1;
                        if mode == DeleteMode::Permanent {
                            result.freed_bytes += size;
                            if let Some(id) = &session_id {
                                cleanup_sqlite_records(codex_root, id);
                            }
                        }
                        if let Some(parent) = path.parent() {
                            prune_empty_parents(parent, &sessions_root);
                        }
                    }
                    Err((bytes, error)) => {
                        result.freed_bytes += bytes;
                        result.failed.push(failure(rel.clone(), error));
                    }
                }
            }
            Err(message) => result.failed.push(failure(rel.clone(), message)),
        }
    }
    result
}

pub(super) fn archive_at(session_keys: &[String], codex_root: &Path) -> ArchiveResult {
    let mut result = ArchiveResult {
        archived_count: 0,
        failed: Vec::new(),
    };
    let sessions_root = codex_root.join("sessions");
    let archived_root = codex_root.join("archived_sessions");
    let allowed_roots: Vec<PathBuf> = [&sessions_root, &archived_root]
        .into_iter()
        .filter_map(|root| fs::canonicalize(root).ok())
        .collect();
    if allowed_roots.is_empty() {
        result.failed.push(failure(
            String::new(),
            "No sessions/ or archived_sessions/ directory found".to_string(),
        ));
        return result;
    }
    if let Err(error) = fs::create_dir_all(&archived_root) {
        result.failed.push(failure(
            String::new(),
            format!("Failed to create archived_sessions/: {error}"),
        ));
        return result;
    }

    for rel in session_keys {
        let validated = resolve_rel_path(rel)
            .map(|rel_path| codex_root.join(rel_path))
            .and_then(|path| validate_session_file(&path, &allowed_roots).map(|_| path));
        let path = match validated {
            Ok(path) => path,
            Err(message) => {
                result.failed.push(failure(rel.clone(), message));
                continue;
            }
        };
        if rel.starts_with("archived_sessions/") {
            result
                .failed
                .push(failure(rel.clone(), "Already archived".to_string()));
            continue;
        }
        let name = path.file_name().expect("validated rollout file");
        let target = archived_root.join(name);
        if target.exists() {
            result.failed.push(failure(
                rel.clone(),
                "An archived session with the same file name already exists".to_string(),
            ));
            continue;
        }

        match fs::rename(&path, &target) {
            Ok(()) => {
                result.archived_count += 1;
                let companion_paths = get_rollout_companion_paths(codex_root, &path, None);
                for comp in companion_paths {
                    if comp != path && comp.exists() && comp.is_file() {
                        if let Some(comp_name) = comp.file_name() {
                            let comp_target = archived_root.join(comp_name);
                            if !comp_target.exists() {
                                let _ = fs::rename(&comp, &comp_target);
                            }
                        }
                    }
                }
                if let Some(parent) = path.parent() {
                    prune_empty_parents(parent, &sessions_root);
                }
            }
            Err(error) => result
                .failed
                .push(failure(rel.clone(), format!("Failed to archive: {error}"))),
        }
    }
    result
}

/// Turns a scan-reported relative path back into a path safe to join onto the
/// Codex home: no absolute paths, no `.`/`..` components, and it must stay
/// under sessions/ or archived_sessions/.
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
    if !(rel.starts_with("sessions/") || rel.starts_with("archived_sessions/")) {
        return Err("Path is outside the Codex session directories".to_string());
    }
    Ok(path)
}

/// Pre-delete validation: only regular rollout-*.jsonl files directly under
/// sessions/ or archived_sessions/. symlink_metadata does not follow symbolic
/// links, and the canonicalize prefix check prevents path escapes.
fn validate_session_file(path: &Path, allowed_roots: &[PathBuf]) -> Result<u64, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let is_rollout = name.starts_with("rollout-")
        && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"));
    if !is_rollout {
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
            &[active(
                "rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-000000000001.jsonl",
            )],
            DeleteMode::Permanent,
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
            &[
                    active("rollout-2026-01-01T00-00-00-01a00000-0000-0000-0000-00000000000a.jsonl"),
                    "archived_sessions/rollout-2026-01-02T00-00-00-01a00000-0000-0000-0000-00000000000b.jsonl".to_string(),
                ],
            DeleteMode::Permanent,
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
    fn delete_includes_compressed_companion_but_keeps_index() {
        let dir = tempfile::tempdir().unwrap();
        let key = active("rollout-test.jsonl");
        let file = dir.path().join(&key);
        write_rollout(&file, "body");
        fs::write(file.with_extension("jsonl.zst"), "packed").unwrap();
        fs::write(dir.path().join("session_index.jsonl"), "shared").unwrap();
        let result = delete_at(&[key], DeleteMode::Permanent, dir.path());
        assert!(result.failed.is_empty());
        assert_eq!(result.freed_bytes, 10);
        assert!(!file.with_extension("jsonl.zst").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("session_index.jsonl")).unwrap(),
            "shared"
        );
    }

    #[test]
    fn delete_cleans_up_sqlite_history_records() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let uuid = "01a00000-0000-0000-0000-000000000001";
        let key = active(&format!("rollout-2026-08-14T09-00-00-{uuid}.jsonl"));
        let file = root.join(&key);
        write_rollout(&file, "test rollout");

        let db_path = root.join("thread_history_1.sqlite");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE thread_turns (thread_id TEXT PRIMARY KEY);
             CREATE TABLE thread_items (thread_id TEXT);
             CREATE TABLE thread_realtime_items (thread_id TEXT);
             CREATE TABLE thread_history_projection_state (thread_id TEXT);
             INSERT INTO thread_turns VALUES ('01a00000-0000-0000-0000-000000000001');
             INSERT INTO thread_items VALUES ('01a00000-0000-0000-0000-000000000001');
             INSERT INTO thread_turns VALUES ('other-uuid');",
        )
        .unwrap();
        drop(conn);

        let result = delete_at(&[key], DeleteMode::Permanent, root);
        assert!(result.failed.is_empty());
        assert_eq!(result.deleted_count, 1);

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let turns_for_deleted: i64 = conn
            .query_row(
                "SELECT count(*) FROM thread_turns WHERE thread_id = ?1",
                rusqlite::params![uuid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(turns_for_deleted, 0);

        let remaining_turns: i64 = conn
            .query_row("SELECT count(*) FROM thread_turns", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining_turns, 1);
    }

    #[test]
    fn archive_moves_compressed_zst_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let key = active("rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-000000000001.jsonl.zst");
        let file = root.join(&key);
        write_rollout(&file, "compressed");

        let result = archive_at(&[key], root);
        assert!(result.failed.is_empty());
        assert_eq!(result.archived_count, 1);
        assert!(!file.exists());
        assert!(root
            .join("archived_sessions/rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-000000000001.jsonl.zst")
            .exists());
    }

    #[test]
    fn delete_removes_generated_images_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let uuid = "01a00000-0000-0000-0000-000000000001";
        let key = active(&format!("rollout-2026-08-14T09-00-00-{uuid}.jsonl"));
        let file = root.join(&key);
        write_rollout(&file, "test rollout");

        let img_dir = root.join("generated_images").join(uuid);
        fs::create_dir_all(&img_dir).unwrap();
        fs::write(img_dir.join("sample.png"), "image data bytes").unwrap();

        let result = delete_at(&[key], DeleteMode::Permanent, root);
        assert!(result.failed.is_empty());
        assert_eq!(result.deleted_count, 1);
        assert!(!file.exists());
        assert!(!img_dir.exists());
    }

    #[test]
    fn delete_reports_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sessions")).unwrap();

        let result = delete_at(
            &[active(
                "rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-0000000dead0.jsonl",
            )],
            DeleteMode::Permanent,
            root,
        );

        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.failed.len(), 1);
        assert!(result.failed[0].message.contains("File not found"));
    }

    #[test]
    fn delete_rejects_paths_outside_the_session_roots() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sessions")).unwrap();
        let outside = root.join("plugins/rollout-x.jsonl");
        write_rollout(&outside, "do not touch");

        let result = delete_at(
            &[
                "sessions/../../plugins/rollout-x.jsonl".to_string(),
                "archived_sessions/../plugins/rollout-x.jsonl".to_string(),
                "/etc/passwd".to_string(),
                "plugins/rollout-x.jsonl".to_string(),
            ],
            DeleteMode::Permanent,
            root,
        );

        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.failed.len(), 4);
        assert!(outside.exists());
    }

    #[test]
    fn delete_rejects_non_rollout_file_names() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let notes = root.join(active("notes.txt"));
        write_rollout(&notes, "important");

        let result = delete_at(&[active("notes.txt")], DeleteMode::Permanent, root);

        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.failed.len(), 1);
        assert!(result.failed[0]
            .message
            .contains("Only rollout-*.jsonl session files can be deleted"));
        assert!(notes.exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_never_follows_rollout_shaped_symbolic_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = dir.path().join("precious.txt");
        fs::write(&target, "precious").unwrap();
        let link_dir = root.join(active(""));
        fs::create_dir_all(&link_dir).unwrap();
        let link =
            link_dir.join("rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-00000000000d.jsonl");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let result = delete_at(
            &[active(
                "rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-00000000000d.jsonl",
            )],
            DeleteMode::Permanent,
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

    #[cfg(unix)]
    #[test]
    fn archive_rejects_rollout_symbolic_link() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("precious.jsonl");
        fs::write(&target, "precious").unwrap();
        fs::create_dir_all(dir.path().join(active(""))).unwrap();
        let key = active("rollout-link.jsonl");
        let link = dir.path().join(&key);
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let result = archive_at(&[key.clone()], dir.path());
        assert_eq!(result.archived_count, 0);
        assert_eq!(result.failed[0].provider_id, CODEX_PROVIDER_ID);
        assert_eq!(result.failed[0].session_key, key);
        assert!(result.failed[0].message.contains("symbolic link rejected"));
        assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(target).unwrap(), "precious");
    }

    #[cfg(unix)]
    #[test]
    fn operations_reject_parent_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("rollout-precious.jsonl");
        fs::write(&target, "precious").unwrap();
        fs::create_dir_all(dir.path().join("sessions")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("sessions/escape")).unwrap();
        let key = "sessions/escape/rollout-precious.jsonl".to_string();
        for mode in [DeleteMode::Permanent, DeleteMode::Trash] {
            let result = delete_at(&[key.clone()], mode, dir.path());
            assert_eq!(result.deleted_count, 0);
            assert_eq!(result.freed_bytes, 0);
            assert_eq!(result.failed[0].session_key, key);
            assert!(result.failed[0]
                .message
                .contains("outside the Codex session directories"));
        }
        let result = archive_at(&[key.clone()], dir.path());
        assert_eq!(result.archived_count, 0);
        assert_eq!(result.failed[0].session_key, key);
        assert!(result.failed[0]
            .message
            .contains("outside the Codex session directories"));
        assert_eq!(fs::read_to_string(target).unwrap(), "precious");
        assert!(!dir
            .path()
            .join("archived_sessions/rollout-precious.jsonl")
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn configured_session_root_symlink_remains_supported() {
        // The configured roots themselves are trusted and canonicalized;
        // the existing implementation permits symlinked Codex data roots.
        let dir = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        write_rollout(&data.path().join("rollout-active.jsonl"), "active");
        std::os::unix::fs::symlink(data.path(), dir.path().join("sessions")).unwrap();
        let archived = archive_at(&["sessions/rollout-active.jsonl".to_string()], dir.path());
        assert_eq!(archived.archived_count, 1);
        assert!(archived.failed.is_empty());
        assert!(data.path().is_dir());
        assert!(fs::symlink_metadata(dir.path().join("sessions"))
            .unwrap()
            .file_type()
            .is_symlink());
        let deleted = delete_at(
            &["archived_sessions/rollout-active.jsonl".to_string()],
            DeleteMode::Permanent,
            dir.path(),
        );
        assert_eq!(deleted.deleted_count, 1);
        assert_eq!(deleted.freed_bytes, 6);
        assert!(deleted.failed.is_empty());
    }

    #[test]
    fn delete_reports_missing_roots() {
        let dir = tempfile::tempdir().unwrap();
        let result = delete_at(
            &[active(
                "rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-00000000000c.jsonl",
            )],
            DeleteMode::Permanent,
            dir.path(),
        );
        assert_eq!(result.deleted_count, 0);
        assert!(result.failed[0]
            .message
            .contains("No sessions/ or archived_sessions"));
    }

    #[test]
    fn archive_moves_files_and_prunes_empty_date_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join(active(
            "rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-000000000001.jsonl",
        ));
        write_rollout(&file, "hello");
        let kept = root.join("sessions/2026/08/15/rollout-2026-08-15T09-00-00-01a00000-0000-0000-0000-000000000002.jsonl");
        write_rollout(&kept, "keep me");

        let result = archive_at(
            &[active(
                "rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-000000000001.jsonl",
            )],
            root,
        );

        assert!(result.failed.is_empty());
        assert_eq!(result.archived_count, 1);
        assert!(!file.exists());
        assert_eq!(
            fs::read_to_string(root.join("archived_sessions/rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-000000000001.jsonl")).unwrap(),
            "hello"
        );
        assert!(kept.exists());
        assert!(!root.join("sessions/2026/08/14").exists());
        assert!(root.join("sessions/2026").exists());
        assert!(root.join("sessions").exists());
    }

    #[test]
    fn archive_rejects_already_archived_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let archived = root.join("archived_sessions/rollout-2026-01-02T00-00-00-01a00000-0000-0000-0000-00000000000b.jsonl");
        write_rollout(&archived, "old");

        let result = archive_at(
            &["archived_sessions/rollout-2026-01-02T00-00-00-01a00000-0000-0000-0000-00000000000b.jsonl".to_string()],
            root,
        );

        assert_eq!(result.archived_count, 0);
        assert_eq!(result.failed.len(), 1);
        assert!(result.failed[0].message.contains("Already archived"));
        assert!(archived.exists());
    }

    #[test]
    fn archive_never_overwrites_an_existing_archived_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let name = "rollout-2026-08-14T09-00-00-01a00000-0000-0000-0000-000000000001.jsonl";
        let file = root.join(active(name));
        write_rollout(&file, "new");
        let existing = root.join("archived_sessions").join(name);
        write_rollout(&existing, "old");

        let result = archive_at(&[active(name)], root);

        assert_eq!(result.archived_count, 0);
        assert_eq!(result.failed.len(), 1);
        assert!(result.failed[0].message.contains("same file name"));
        // Both copies are intact.
        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        assert_eq!(fs::read_to_string(&existing).unwrap(), "old");
    }

    #[test]
    fn archive_rejects_paths_outside_the_session_roots() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sessions")).unwrap();
        let outside = root.join("plugins/rollout-x.jsonl");
        write_rollout(&outside, "do not touch");

        let result = archive_at(
            &[
                "sessions/../../plugins/rollout-x.jsonl".to_string(),
                "/etc/passwd".to_string(),
                "plugins/rollout-x.jsonl".to_string(),
            ],
            root,
        );

        assert_eq!(result.archived_count, 0);
        assert_eq!(result.failed.len(), 3);
        assert!(outside.exists());
    }
}
