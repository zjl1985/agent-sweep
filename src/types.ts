export interface SessionEntry {
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
  sessionIds: string[];
  mode: DeleteMode;
}

export interface DeleteFailure {
  sessionId: string;
  message: string;
}

export interface DeleteResult {
  deletedCount: number;
  freedBytes: number;
  failed: DeleteFailure[];
}
