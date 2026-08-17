use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub session_id: String,
    pub title: String,
    pub workspace: Option<String>,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: i64,
    pub size_bytes: u64,
    pub archived: bool,
    pub file_name: String,
    /// Path relative to the Codex home (e.g. `sessions/2026/08/15/rollout-….jsonl`).
    /// Unique per file, unlike `session_id` — resumed sessions share one id
    /// across several rollout files.
    pub rel_path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirUsage {
    pub name: String,
    pub size_bytes: u64,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    /// Sorted by size_bytes, descending.
    pub sessions: Vec<SessionEntry>,
    pub active_count: u64,
    pub archived_count: u64,
    pub active_bytes: u64,
    pub archived_bytes: u64,
    pub sessions_bytes: u64,
    pub codex_total_bytes: u64,
    pub other_dirs: Vec<DirUsage>,
    pub issues: Vec<String>,
    pub scanned_at_ms: i64,
}

impl ScanResult {
    pub fn empty(now_ms: i64) -> Self {
        Self {
            sessions: Vec::new(),
            active_count: 0,
            archived_count: 0,
            active_bytes: 0,
            archived_bytes: 0,
            sessions_bytes: 0,
            codex_total_bytes: 0,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRequest {
    /// Paths relative to the Codex home, as reported by the scan.
    pub paths: Vec<String>,
    pub mode: DeleteMode,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFailure {
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub deleted_count: u32,
    pub freed_bytes: u64,
    pub failed: Vec<DeleteFailure>,
}
