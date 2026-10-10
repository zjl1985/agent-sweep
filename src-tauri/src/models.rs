use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub archive: bool,
    pub delete_trash: bool,
    pub delete_permanent: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub provider_id: String,
    pub provider_name: String,
    /// Provider-owned opaque identifier used for archive and delete operations.
    pub session_key: String,
    pub capabilities: ProviderCapabilities,
    pub session_id: String,
    pub title: String,
    pub workspace: Option<String>,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: i64,
    pub size_bytes: u64,
    pub archived: bool,
    pub file_name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirUsage {
    pub provider_id: String,
    pub provider_name: String,
    pub name: String,
    pub size_bytes: u64,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub root_label: String,
    pub total_bytes: u64,
    pub session_count: u64,
    pub capabilities: ProviderCapabilities,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderScan {
    pub sessions: Vec<SessionEntry>,
    pub active_count: u64,
    pub archived_count: u64,
    pub active_bytes: u64,
    pub archived_bytes: u64,
    pub sessions_bytes: u64,
    pub total_bytes: u64,
    pub other_dirs: Vec<DirUsage>,
    pub issues: Vec<String>,
}

impl ProviderScan {
    pub fn empty() -> Self {
        Self {
            sessions: Vec::new(),
            active_count: 0,
            archived_count: 0,
            active_bytes: 0,
            archived_bytes: 0,
            sessions_bytes: 0,
            total_bytes: 0,
            other_dirs: Vec::new(),
            issues: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    /// Sorted by size_bytes, descending across all providers.
    pub sessions: Vec<SessionEntry>,
    pub providers: Vec<ProviderSummary>,
    pub session_count: u64,
    pub active_count: u64,
    pub archived_count: u64,
    pub active_bytes: u64,
    pub archived_bytes: u64,
    pub sessions_bytes: u64,
    pub total_bytes: u64,
    pub other_dirs: Vec<DirUsage>,
    pub issues: Vec<String>,
    pub scanned_at_ms: i64,
}

impl ScanResult {
    pub fn empty(now_ms: i64) -> Self {
        Self {
            sessions: Vec::new(),
            providers: Vec::new(),
            session_count: 0,
            active_count: 0,
            archived_count: 0,
            active_bytes: 0,
            archived_bytes: 0,
            sessions_bytes: 0,
            total_bytes: 0,
            other_dirs: Vec::new(),
            issues: Vec::new(),
            scanned_at_ms: now_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeleteMode {
    Trash,
    Permanent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OperationTarget {
    pub provider_id: String,
    pub session_key: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRequest {
    pub targets: Vec<OperationTarget>,
    pub mode: DeleteMode,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationFailure {
    pub provider_id: String,
    pub session_key: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRequest {
    pub targets: Vec<OperationTarget>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveResult {
    pub archived_count: u32,
    pub failed: Vec<OperationFailure>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub deleted_count: u32,
    pub freed_bytes: u64,
    pub failed: Vec<OperationFailure>,
}
