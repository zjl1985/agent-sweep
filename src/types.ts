export interface ProviderCapabilities {
  archive: boolean;
  deleteTrash: boolean;
  deletePermanent: boolean;
}

export interface ProviderSummary {
  id: string;
  name: string;
  rootLabel: string;
  totalBytes: number;
  sessionCount: number;
  capabilities: ProviderCapabilities;
  issues: string[];
}

export interface SessionEntry {
  providerId: string;
  providerName: string;
  /** Opaque identifier interpreted only by the owning provider. */
  sessionKey: string;
  capabilities: ProviderCapabilities;
  sessionId: string;
  title: string;
  workspace: string | null;
  createdAtMs: number | null;
  updatedAtMs: number;
  sizeBytes: number;
  archived: boolean;
  fileName: string;
}

export interface DirUsage {
  providerId: string;
  providerName: string;
  name: string;
  sizeBytes: number;
  note: string;
}

export interface ScanResult {
  sessions: SessionEntry[];
  providers: ProviderSummary[];
  activeCount: number;
  archivedCount: number;
  activeBytes: number;
  archivedBytes: number;
  sessionsBytes: number;
  totalBytes: number;
  otherDirs: DirUsage[];
  issues: string[];
  scannedAtMs: number;
}

export interface SessionTarget {
  providerId: string;
  sessionKey: string;
}

export type DeleteMode = "trash" | "permanent";

export interface DeleteRequest {
  targets: SessionTarget[];
  mode: DeleteMode;
}

export interface ArchiveRequest {
  targets: SessionTarget[];
}

export interface OperationFailure extends SessionTarget {
  message: string;
}

export interface DeleteResult {
  deletedCount: number;
  freedBytes: number;
  failed: OperationFailure[];
}

export interface ArchiveResult {
  archivedCount: number;
  failed: OperationFailure[];
}

export interface StorageCleanupResult {
  removedEventGroups: number;
  freedBytes: number;
  warning: string | null;
}
