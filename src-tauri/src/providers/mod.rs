mod usage;
mod files;
mod claude;
mod codex;
mod kimi;
mod opencode;
mod zcode;

use std::collections::HashMap;

use crate::models::{
    ArchiveRequest, ArchiveResult, DeleteMode, DeleteRequest, DeleteResult, OperationFailure,
    ProviderCapabilities, ProviderScan, ProviderSummary, ScanResult,
};

pub(crate) trait SessionProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn root_label(&self) -> String;
    fn capabilities(&self) -> ProviderCapabilities;
    fn scan(&self, now_ms: i64) -> ProviderScan;
    fn archive(&self, session_keys: &[String]) -> ArchiveResult;
    fn delete(&self, session_keys: &[String], mode: DeleteMode) -> DeleteResult;
}

pub(crate) struct ProviderRegistry {
    providers: Vec<Box<dyn SessionProvider>>,
}

impl ProviderRegistry {
    pub fn new(providers: Vec<Box<dyn SessionProvider>>) -> Self {
        Self { providers }
    }

    pub fn built_in() -> Self {
        Self::new(vec![
            Box::new(codex::CodexProvider::new()),
            Box::new(claude::ClaudeProvider::new()),
            Box::new(opencode::OpenCodeProvider::new()),
            Box::new(kimi::KimiProvider::new()),
            Box::new(zcode::ZCodeProvider::new()),
        ])
    }

    pub fn scan(&self) -> ScanResult {
        self.scan_at(chrono::Utc::now().timestamp_millis())
    }

    fn scan_at(&self, now_ms: i64) -> ScanResult {
        let mut result = ScanResult::empty(now_ms);
        for provider in &self.providers {
            let capabilities = provider.capabilities();
            let mut scan = provider.scan(now_ms);
            usage::enrich(provider.id(), std::path::Path::new(&provider.root_label()), &mut scan);
            let provider_id = provider.id().to_string();
            let provider_name = provider.name().to_string();
            for session in &mut scan.sessions {
                session.provider_id = provider_id.clone();
                session.provider_name = provider_name.clone();
                // A session may narrow its provider's supported actions (e.g.
                // an already archived session cannot be archived again).
                session.capabilities.archive &= capabilities.archive;
                session.capabilities.delete_trash &= capabilities.delete_trash;
                session.capabilities.delete_permanent &= capabilities.delete_permanent;
            }

            result.session_count += scan.sessions.len() as u64;
            result.active_count += scan.active_count;
            result.archived_count += scan.archived_count;
            result.active_bytes += scan.active_bytes;
            result.archived_bytes += scan.archived_bytes;
            result.sessions_bytes += scan.sessions_bytes;
            result.total_bytes += scan.total_bytes;
            for usage in &mut scan.other_dirs {
                usage.provider_id = provider_id.clone();
                usage.provider_name = provider_name.clone();
            }
            result.other_dirs.extend(scan.other_dirs);
            result.issues.extend(
                scan.issues
                    .iter()
                    .map(|issue| format!("{}: {issue}", provider.name())),
            );
            result.providers.push(ProviderSummary {
                id: provider_id,
                name: provider_name,
                root_label: provider.root_label(),
                total_bytes: scan.total_bytes,
                session_count: scan.sessions.len() as u64,
                capabilities,
                issues: scan.issues,
            });
            result.sessions.extend(scan.sessions);
        }
        result.sessions.sort_by(|a, b| {
            b.size_bytes
                .cmp(&a.size_bytes)
                .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
        });
        result
    }

    pub fn archive(&self, request: ArchiveRequest) -> ArchiveResult {
        let mut grouped: HashMap<&str, Vec<String>> = HashMap::new();
        let mut result = ArchiveResult {
            archived_count: 0,
            failed: Vec::new(),
        };
        for target in &request.targets {
            grouped
                .entry(&target.provider_id)
                .or_default()
                .push(target.session_key.clone());
        }
        for (provider_id, session_keys) in grouped {
            let Some(provider) = self.provider(provider_id) else {
                result
                    .failed
                    .extend(session_keys.into_iter().map(|session_key| {
                        failure(provider_id, session_key, "Unknown session provider")
                    }));
                continue;
            };
            if !provider.capabilities().archive {
                result
                    .failed
                    .extend(session_keys.into_iter().map(|session_key| {
                        failure(
                            provider_id,
                            session_key,
                            "Provider does not support archiving",
                        )
                    }));
                continue;
            }
            let provider_result = provider.archive(&session_keys);
            result.archived_count += provider_result.archived_count;
            result.failed.extend(provider_result.failed);
        }
        result
    }

    pub fn delete(&self, request: DeleteRequest) -> DeleteResult {
        let mut grouped: HashMap<&str, Vec<String>> = HashMap::new();
        let mut result = DeleteResult {
            deleted_count: 0,
            freed_bytes: 0,
            failed: Vec::new(),
        };
        for target in &request.targets {
            grouped
                .entry(&target.provider_id)
                .or_default()
                .push(target.session_key.clone());
        }
        for (provider_id, session_keys) in grouped {
            let Some(provider) = self.provider(provider_id) else {
                result
                    .failed
                    .extend(session_keys.into_iter().map(|session_key| {
                        failure(provider_id, session_key, "Unknown session provider")
                    }));
                continue;
            };
            let capabilities = provider.capabilities();
            let supported = match request.mode {
                DeleteMode::Trash => capabilities.delete_trash,
                DeleteMode::Permanent => capabilities.delete_permanent,
            };
            if !supported {
                let action = match request.mode {
                    DeleteMode::Trash => "moving sessions to trash",
                    DeleteMode::Permanent => "permanent deletion",
                };
                result
                    .failed
                    .extend(session_keys.into_iter().map(|session_key| {
                        failure(
                            provider_id,
                            session_key,
                            format!("Provider does not support {action}"),
                        )
                    }));
                continue;
            }
            let provider_result = provider.delete(&session_keys, request.mode);
            result.deleted_count += provider_result.deleted_count;
            result.freed_bytes += provider_result.freed_bytes;
            result.failed.extend(provider_result.failed);
        }
        result
    }

    fn provider(&self, id: &str) -> Option<&dyn SessionProvider> {
        self.providers
            .iter()
            .find(|provider| provider.id() == id)
            .map(Box::as_ref)
    }
}

fn failure(
    provider_id: impl Into<String>,
    session_key: impl Into<String>,
    message: impl Into<String>,
) -> OperationFailure {
    OperationFailure {
        provider_id: provider_id.into(),
        session_key: session_key.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DirUsage, OperationTarget, SessionEntry};

    use std::sync::{Arc, Mutex};

    type RecordedCalls = Arc<Mutex<Vec<(Option<DeleteMode>, Vec<String>)>>>;

    struct TestProvider {
        id: &'static str,
        capabilities: ProviderCapabilities,
        calls: Option<RecordedCalls>,
    }

    impl SessionProvider for TestProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        fn name(&self) -> &'static str {
            "Test Provider"
        }

        fn root_label(&self) -> String {
            "Test root".to_string()
        }

        fn capabilities(&self) -> ProviderCapabilities {
            self.capabilities
        }

        fn scan(&self, _now_ms: i64) -> ProviderScan {
            let mut scan = ProviderScan::empty();
            scan.sessions.push(SessionEntry {
            usage: None,
                provider_id: String::new(),
                provider_name: String::new(),
                session_key: "opaque-key".to_string(),
                capabilities: self.capabilities,
                session_id: "display-id".to_string(),
                title: "Session".to_string(),
                workspace: None,
                created_at_ms: None,
                updated_at_ms: 1,
                size_bytes: 42,
                archived: false,
                file_name: "ignored".to_string(),
            });
            scan.active_count = 1;
            scan.active_bytes = 42;
            scan.sessions_bytes = 42;
            scan.total_bytes = 100;
            scan.other_dirs.push(DirUsage {
                provider_id: "untrusted-id".to_string(),
                provider_name: "untrusted-name".to_string(),
                name: "Cache".to_string(),
                size_bytes: 58,
                note: "read-only".to_string(),
            });
            scan.issues.push("scan issue".to_string());
            scan
        }

        fn archive(&self, session_keys: &[String]) -> ArchiveResult {
            assert!(self.capabilities.archive);
            assert!(session_keys.iter().all(|key| key == "opaque-key"));
            if let Some(calls) = &self.calls {
                calls.lock().unwrap().push((None, session_keys.to_vec()));
            }
            ArchiveResult {
                archived_count: session_keys.len() as u32,
                failed: Vec::new(),
            }
        }

        fn delete(&self, session_keys: &[String], mode: DeleteMode) -> DeleteResult {
            assert!(match mode {
                DeleteMode::Trash => self.capabilities.delete_trash,
                DeleteMode::Permanent => self.capabilities.delete_permanent,
            });
            assert!(session_keys.iter().all(|key| key == "opaque-key"));
            if let Some(calls) = &self.calls {
                calls
                    .lock()
                    .unwrap()
                    .push((Some(mode), session_keys.to_vec()));
            }
            DeleteResult {
                deleted_count: session_keys.len() as u32,
                freed_bytes: 42 * session_keys.len() as u64,
                failed: Vec::new(),
            }
        }
    }

    fn target(provider_id: &str) -> OperationTarget {
        OperationTarget {
            provider_id: provider_id.to_string(),
            session_key: "opaque-key".to_string(),
        }
    }

    #[test]
    fn scan_adds_provider_metadata_and_preserves_opaque_session_key() {
        let capabilities = ProviderCapabilities {
            archive: true,
            delete_trash: false,
            delete_permanent: true,
        };
        let registry = ProviderRegistry::new(vec![Box::new(TestProvider {
            id: "test",
            calls: None,
            capabilities,
        })]);
        let result = registry.scan_at(7);

        assert_eq!(result.scanned_at_ms, 7);
        assert_eq!(result.total_bytes, 100);
        assert_eq!(result.session_count, 1);
        assert_eq!(result.providers.len(), 1);
        assert_eq!(result.providers[0].id, "test");
        assert_eq!(result.providers[0].name, "Test Provider");
        assert_eq!(result.providers[0].root_label, "Test root");
        assert_eq!(result.providers[0].capabilities, capabilities);
        assert_eq!(result.sessions[0].provider_id, "test");
        assert_eq!(result.sessions[0].provider_name, "Test Provider");
        assert_eq!(result.sessions[0].session_key, "opaque-key");
        assert_eq!(result.sessions[0].capabilities, capabilities);
        assert_eq!(result.other_dirs[0].provider_id, "test");
        assert_eq!(result.other_dirs[0].provider_name, "Test Provider");
        let usage = serde_json::to_value(&result.other_dirs[0]).unwrap();
        assert_eq!(usage["providerId"], "test");
        assert_eq!(usage["providerName"], "Test Provider");
        let session = serde_json::to_value(&result.sessions[0]).unwrap();
        assert_eq!(session["sessionKey"], "opaque-key");
        assert!(session.get("relPath").is_none());
        assert_eq!(result.issues, ["Test Provider: scan issue"]);
        assert_eq!(result.providers[0].issues, ["scan issue"]);
    }

    #[test]
    fn unknown_provider_operation_fails_without_dispatch() {
        let registry = ProviderRegistry::new(Vec::new());
        let result = registry.delete(DeleteRequest {
            targets: vec![target("missing")],
            mode: DeleteMode::Permanent,
        });

        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].provider_id, "missing");
        assert_eq!(result.failed[0].session_key, "opaque-key");
        assert!(result.failed[0].message.contains("Unknown"));
    }

    #[test]
    fn registry_aggregates_providers_and_routes_each_action() {
        let registry = ProviderRegistry::new(vec![
            Box::new(TestProvider {
                id: "first",
                calls: None,
                capabilities: ProviderCapabilities {
                    archive: true,
                    delete_trash: true,
                    delete_permanent: false,
                },
            }),
            Box::new(TestProvider {
                id: "second",
                calls: None,
                capabilities: ProviderCapabilities {
                    archive: false,
                    delete_trash: false,
                    delete_permanent: true,
                },
            }),
        ]);
        let scan = registry.scan_at(7);
        assert_eq!(scan.providers.len(), 2);
        assert_eq!(scan.session_count, 2);
        assert_eq!(scan.active_count, 2);
        assert_eq!(scan.active_bytes, 84);
        assert_eq!(scan.sessions_bytes, 84);
        assert_eq!(scan.total_bytes, 200);
        assert_eq!(scan.other_dirs[0].provider_id, "first");
        assert_eq!(scan.other_dirs[1].provider_id, "second");
        assert_eq!(scan.sessions[0].session_key, scan.sessions[1].session_key);
        assert_ne!(scan.sessions[0].provider_id, scan.sessions[1].provider_id);

        let targets = vec![target("first"), target("second"), target("missing")];
        let archived = registry.archive(ArchiveRequest {
            targets: targets.clone(),
        });
        assert_eq!(archived.archived_count, 1);
        assert_eq!(archived.failed.len(), 2);
        assert!(archived
            .failed
            .iter()
            .any(|failure| failure.provider_id == "missing"));
        for mode in [DeleteMode::Trash, DeleteMode::Permanent] {
            let deleted = registry.delete(DeleteRequest {
                targets: targets.clone(),
                mode,
            });
            assert_eq!(deleted.deleted_count, 1);
            assert_eq!(deleted.freed_bytes, 42);
            assert_eq!(deleted.failed.len(), 2);
            assert!(deleted
                .failed
                .iter()
                .all(|failure| failure.session_key == "opaque-key"));
        }
    }

    #[test]
    fn mixed_requests_route_identical_keys_independently_to_each_provider() {
        let first_calls = Arc::new(Mutex::new(Vec::new()));
        let second_calls = Arc::new(Mutex::new(Vec::new()));
        let capabilities = ProviderCapabilities {
            archive: true,
            delete_trash: true,
            delete_permanent: true,
        };
        let registry = ProviderRegistry::new(vec![
            Box::new(TestProvider {
                id: "first",
                capabilities,
                calls: Some(first_calls.clone()),
            }),
            Box::new(TestProvider {
                id: "second",
                capabilities,
                calls: Some(second_calls.clone()),
            }),
        ]);
        let scan = registry.scan_at(7);
        assert_eq!(scan.session_count, 2);
        assert_eq!(scan.active_count, 2);
        assert_eq!(scan.archived_count, 0);
        assert_eq!(scan.active_bytes, 84);
        assert_eq!(scan.archived_bytes, 0);
        assert_eq!(scan.sessions_bytes, 84);
        assert_eq!(scan.total_bytes, 200);
        assert_eq!(
            scan.providers
                .iter()
                .map(|provider| provider.total_bytes)
                .sum::<u64>(),
            200
        );
        assert_eq!(
            scan.other_dirs
                .iter()
                .map(|usage| usage.size_bytes)
                .sum::<u64>(),
            116
        );
        let targets: Vec<_> = scan
            .sessions
            .iter()
            .map(|session| OperationTarget {
                provider_id: session.provider_id.clone(),
                session_key: session.session_key.clone(),
            })
            .collect();
        let result = registry.archive(ArchiveRequest {
            targets: targets.clone(),
        });
        assert_eq!(result.archived_count, 2);
        assert!(result.failed.is_empty());
        for mode in [DeleteMode::Trash, DeleteMode::Permanent] {
            let result = registry.delete(DeleteRequest {
                targets: targets.clone(),
                mode,
            });
            assert_eq!(result.deleted_count, 2);
            assert_eq!(result.freed_bytes, 84);
            assert!(result.failed.is_empty());
        }
        let expected = vec![
            (None, vec!["opaque-key".to_string()]),
            (Some(DeleteMode::Trash), vec!["opaque-key".to_string()]),
            (Some(DeleteMode::Permanent), vec!["opaque-key".to_string()]),
        ];
        assert_eq!(*first_calls.lock().unwrap(), expected);
        assert_eq!(*second_calls.lock().unwrap(), expected);
    }

    #[test]
    fn registry_routes_codex_scan_archive_delete_and_preserves_session_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sessions/2026/08/14")).unwrap();
        std::fs::create_dir_all(root.join("archived_sessions")).unwrap();
        let active_key = "sessions/2026/08/14/rollout-active.jsonl";
        let archived_key = "archived_sessions/rollout-archived.jsonl";
        std::fs::write(root.join(active_key), "active").unwrap();
        std::fs::write(root.join(archived_key), "old").unwrap();
        let registry = ProviderRegistry::new(vec![Box::new(codex::CodexProvider::at(root))]);

        let scan = registry.scan_at(7);
        assert_eq!(scan.active_count, 1);
        assert_eq!(scan.archived_count, 1);
        assert_eq!(scan.active_bytes, 6);
        assert_eq!(scan.archived_bytes, 3);
        assert_eq!(scan.providers[0].id, "codex");
        assert!(scan.sessions[0].capabilities.archive);
        assert!(!scan.sessions[1].capabilities.archive);
        assert!(scan
            .other_dirs
            .iter()
            .all(|usage| usage.provider_id == "codex" && usage.provider_name == "Codex"));
        let archived = registry.archive(ArchiveRequest {
            targets: vec![OperationTarget {
                provider_id: scan.sessions[0].provider_id.clone(),
                session_key: scan.sessions[0].session_key.clone(),
            }],
        });
        assert_eq!(archived.archived_count, 1);
        assert!(archived.failed.is_empty());
        let scan = registry.scan_at(8);
        assert_eq!(scan.active_count, 0);
        assert_eq!(scan.archived_count, 2);
        assert!(scan
            .sessions
            .iter()
            .all(|session| !session.capabilities.archive));
        let targets = scan
            .sessions
            .iter()
            .map(|session| OperationTarget {
                provider_id: session.provider_id.clone(),
                session_key: session.session_key.clone(),
            })
            .collect();
        let rejected = registry.archive(ArchiveRequest { targets });
        assert_eq!(rejected.archived_count, 0);
        assert_eq!(rejected.failed.len(), 2);
        assert!(
            rejected
                .failed
                .iter()
                .all(|failure| failure.provider_id == "codex"
                    && failure.message == "Already archived")
        );
        let deleted = registry.delete(DeleteRequest {
            targets: vec![OperationTarget {
                provider_id: "codex".to_string(),
                session_key: "archived_sessions/rollout-active.jsonl".to_string(),
            }],
            mode: DeleteMode::Permanent,
        });
        assert_eq!(deleted.deleted_count, 1);
        assert_eq!(deleted.freed_bytes, 6);
        assert!(deleted.failed.is_empty());
        assert_eq!(registry.scan_at(9).session_count, 1);
    }

    #[test]
    fn built_in_registers_all_providers() {
        let registry = ProviderRegistry::built_in();
        let ids: Vec<_> = registry.providers.iter().map(|p| p.id()).collect();
        assert_eq!(ids, ["codex", "claude", "opencode", "kimi", "zcode"]);
    }

    #[test]
    fn registry_rejects_unsupported_capability() {
        let registry = ProviderRegistry::new(vec![Box::new(TestProvider {
            id: "test",
            calls: None,
            capabilities: ProviderCapabilities {
                archive: false,
                delete_trash: false,
                delete_permanent: true,
            },
        })]);

        let archived = registry.archive(ArchiveRequest {
            targets: vec![target("test")],
        });
        assert_eq!(archived.archived_count, 0);
        assert!(archived.failed[0].message.contains("does not support"));

        let trashed = registry.delete(DeleteRequest {
            targets: vec![target("test")],
            mode: DeleteMode::Trash,
        });
        assert_eq!(trashed.deleted_count, 0);
        assert!(trashed.failed[0].message.contains("does not support"));

        let deleted = registry.delete(DeleteRequest {
            targets: vec![target("test")],
            mode: DeleteMode::Permanent,
        });
        assert_eq!(deleted.deleted_count, 1);
        assert!(deleted.failed.is_empty());
    }
}

pub(crate) fn reclaim_opencode() -> Result<crate::models::StorageCleanupResult, String> {
    opencode::OpenCodeProvider::new().reclaim()
}
