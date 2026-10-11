mod cleanup;
mod discovery;

use std::path::{Path, PathBuf};

use crate::models::{ArchiveResult, DeleteMode, DeleteResult, ProviderCapabilities, ProviderScan};

use super::SessionProvider;

pub const ZCODE_PROVIDER_ID: &str = "zcode";
pub const ZCODE_PROVIDER_NAME: &str = "ZCode";

pub struct ZCodeProvider {
    root: Option<PathBuf>,
}

impl ZCodeProvider {
    pub fn new() -> Self {
        Self {
            root: dirs::home_dir().map(|home| home.join(".zcode")),
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
        }
    }

    fn root(&self) -> Result<&Path, String> {
        self.root
            .as_deref()
            .ok_or_else(|| "Could not locate the user home directory".to_string())
    }

    fn unavailable_archive(&self, message: String) -> ArchiveResult {
        ArchiveResult {
            archived_count: 0,
            failed: vec![cleanup::failure(String::new(), message)],
        }
    }

    fn unavailable_delete(&self, message: String) -> DeleteResult {
        DeleteResult {
            deleted_count: 0,
            freed_bytes: 0,
            failed: vec![cleanup::failure(String::new(), message)],
        }
    }
}

impl SessionProvider for ZCodeProvider {
    fn id(&self) -> &'static str {
        ZCODE_PROVIDER_ID
    }

    fn name(&self) -> &'static str {
        ZCODE_PROVIDER_NAME
    }

    fn root_label(&self) -> String {
        self.root
            .as_ref()
            .map(|root| root.display().to_string())
            .unwrap_or_else(|| "~/.zcode".to_string())
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            archive: true,
            delete_trash: true,
            delete_permanent: true,
        }
    }

    fn scan(&self, _now_ms: i64) -> ProviderScan {
        match self.root() {
            Ok(root) => discovery::scan_zcode_at(root),
            Err(message) => {
                let mut scan = ProviderScan::empty();
                scan.issues.push(message);
                scan
            }
        }
    }

    fn archive(&self, session_keys: &[String]) -> ArchiveResult {
        match self.root() {
            Ok(root) => cleanup::archive_at(session_keys, root),
            Err(message) => self.unavailable_archive(message),
        }
    }

    fn delete(&self, session_keys: &[String], mode: DeleteMode) -> DeleteResult {
        match self.root() {
            Ok(root) => cleanup::delete_at(session_keys, mode, root),
            Err(message) => self.unavailable_delete(message),
        }
    }
}
