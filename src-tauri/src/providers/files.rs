//! Shared conservative handling of explicitly session-owned files.
use crate::models::DeleteMode;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub(super) fn size(path: &Path) -> Result<u64, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Ok(0);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Err("Unsupported session artifact".into());
    }
    let mut bytes = 0;
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        bytes += size(&entry.map_err(|e| e.to_string())?.path())?;
    }
    Ok(bytes)
}

pub(super) fn validate(root: &Path, path: &Path) -> Result<Option<PathBuf>, String> {
    let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
    if rel.as_os_str().is_empty() {
        return Err("Provider root cannot be deleted".into());
    }
    let mut current = root.to_path_buf();
    for component in rel.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err("Invalid artifact path".into());
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err("Symbolic link artifact rejected".into())
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        }
    }
    if !fs::canonicalize(&current)
        .map_err(|e| e.to_string())?
        .starts_with(fs::canonicalize(root).map_err(|e| e.to_string())?)
    {
        return Err("Artifact escapes provider root".into());
    }
    size(&current)?; // Fail before deletion if any part cannot be inventoried.
    Ok(Some(current))
}

pub(super) fn remove(path: &Path, mode: DeleteMode) -> Result<u64, (u64, String)> {
    let before = size(path).map_err(|e| (0, e))?;
    let result = match mode {
        DeleteMode::Trash => trash::delete(path).map_err(|e| e.to_string()),
        DeleteMode::Permanent if path.is_dir() => {
            fs::remove_dir_all(path).map_err(|e| e.to_string())
        }
        DeleteMode::Permanent => fs::remove_file(path).map_err(|e| e.to_string()),
    };
    match result {
        Ok(()) => Ok(if mode == DeleteMode::Permanent {
            before
        } else {
            0
        }),
        Err(error) => {
            let removed = if mode == DeleteMode::Permanent {
                before.saturating_sub(size(path).unwrap_or(before))
            } else {
                0
            };
            Err((removed, error))
        }
    }
}

/// Stage a whole session before disposal. A failed move restores previous moves.
/// The manifest makes an interrupted staging operation recoverable.
pub(super) fn remove_group(
    root: &Path,
    paths: &[PathBuf],
    mode: DeleteMode,
) -> Result<u64, (u64, String)> {
    let mut inventory = Vec::new();
    for path in paths {
        if let Some(path) = validate(root, path).map_err(|e| (0, e))? {
            if !inventory
                .iter()
                .any(|(p, _): &(PathBuf, u64)| path.starts_with(p))
            {
                inventory.retain(|(p, _)| !p.starts_with(&path));
                let bytes = size(&path).map_err(|e| (0, e))?;
                inventory.push((path, bytes));
            }
        }
    }
    if inventory.is_empty() {
        return Ok(0);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = root.join(format!("AgentSweep-session-{}-{stamp}", std::process::id()));
    fs::create_dir(&stage).map_err(|e| (0, e.to_string()))?;
    let manifest: Vec<_> = inventory
        .iter()
        .enumerate()
        .map(|(i, (path, _))| serde_json::json!({"original":path,"stored":format!("{i}")}))
        .collect();
    if let Err(error) = fs::write(
        stage.join("restore.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    ) {
        let _ = fs::remove_dir_all(&stage);
        return Err((0, error.to_string()));
    }
    let mut moved = Vec::new();
    for (i, (path, _)) in inventory.iter().enumerate() {
        let target = stage.join(i.to_string());
        if let Err(error) = fs::rename(path, &target) {
            return Err((0, rollback(&moved, &stage, &error.to_string())));
        }
        moved.push((path.clone(), target));
    }
    if mode == DeleteMode::Trash {
        if let Err(error) = trash::delete(&stage) {
            return Err((0, rollback(&moved, &stage, &error.to_string())));
        }
        return Ok(0); // Trash occupies disk until emptied.
    }
    let mut freed = 0;
    for (i, (_, bytes)) in inventory.iter().enumerate() {
        match remove(&stage.join(i.to_string()), mode) {
            Ok(_) => freed += bytes,
            Err((partial, error)) => {
                return Err((
                    freed + partial,
                    format!(
                        "Partial cleanup; remaining files and restore.json are in {}: {error}",
                        stage.display()
                    ),
                ))
            }
        }
    }
    fs::remove_file(stage.join("restore.json"))
        .and_then(|_| fs::remove_dir(&stage))
        .map_err(|e| {
            (
                freed,
                format!("Session deleted; staging cleanup failed: {e}"),
            )
        })?;
    Ok(freed)
}

fn rollback(moved: &[(PathBuf, PathBuf)], stage: &Path, error: &str) -> String {
    for (original, stored) in moved.iter().rev() {
        if original.exists() || fs::rename(stored, original).is_err() {
            return format!(
                "{error}; recovery files retained in {} (see restore.json)",
                stage.display()
            );
        }
    }
    let _ = fs::remove_dir_all(stage);
    format!("{error}; session moves rolled back")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_all_artifacts_before_removing_any() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let main = root.path().join("main.jsonl");
        fs::write(&main, "keep").unwrap();
        let bad = outside.path().join("outside");
        fs::write(&bad, "keep").unwrap();
        assert!(remove_group(
            root.path(),
            &[main.clone(), bad.clone()],
            DeleteMode::Permanent
        )
        .is_err());
        assert!(main.exists());
        assert!(bad.exists());
    }
    #[test]
    fn removes_entire_group_and_counts_only_file_bytes() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("main");
        let extra = root.path().join("extra");
        fs::write(&main, "你好").unwrap();
        fs::create_dir(&extra).unwrap();
        fs::write(extra.join("child"), "abc").unwrap();
        assert_eq!(
            remove_group(root.path(), &[main, extra], DeleteMode::Permanent).unwrap(),
            9
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
    #[test]
    fn rollback_restores_previously_staged_files() {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        fs::create_dir(&stage).unwrap();
        let original = root.path().join("original");
        let stored = stage.join("0");
        fs::write(&stored, "keep").unwrap();
        let message = rollback(&[(original.clone(), stored)], &stage, "move failed");
        assert!(message.contains("rolled back"));
        assert_eq!(fs::read_to_string(original).unwrap(), "keep");
    }
}
