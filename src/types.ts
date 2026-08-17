export interface SessionEntry {
  sessionId: string;
  title: string;
  workspace: string | null;
  createdAtMs: number | null;
  updatedAtMs: number;
  sizeBytes: number;
  archived: boolean;
  fileName: string;
  /** Path relative to the Codex home; unique per file (session ids are not). */
  relPath: string;
}

export interface DirUsage {
  name: string;
  sizeBytes: number;
  note: string;
}

export interface ScanResult {
  sessions: SessionEntry[];
  activeCount: number;
  archivedCount: number;
  activeBytes: number;
  archivedBytes: number;
  sessionsBytes: number;
  codexTotalBytes: number;
  otherDirs: DirUsage[];
  issues: string[];
  scannedAtMs: number;
}

export type DeleteMode = "trash" | "permanent";

export interface DeleteRequest {
  paths: string[];
  mode: DeleteMode;
}

export interface DeleteFailure {
  path: string;
  message: string;
}

export interface DeleteResult {
  deletedCount: number;
  freedBytes: number;
  failed: DeleteFailure[];
}
