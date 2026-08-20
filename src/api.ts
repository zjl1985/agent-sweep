import { invoke } from "@tauri-apps/api/core";
import type {
  ArchiveRequest,
  ArchiveResult,
  DeleteRequest,
  DeleteResult,
  ScanResult,
} from "./types";

function assertDesktopRuntime() {
  if (!("__TAURI_INTERNALS__" in window)) {
    throw new Error(
      "Local backend unavailable. Open this app through the CodexClear desktop app, not a browser.",
    );
  }
}

export async function scanSessions(): Promise<ScanResult> {
  assertDesktopRuntime();
  return invoke<ScanResult>("scan_sessions");
}

export async function deleteSessions(request: DeleteRequest): Promise<DeleteResult> {
  assertDesktopRuntime();
  return invoke<DeleteResult>("delete_sessions", { request });
}

export async function archiveSessions(request: ArchiveRequest): Promise<ArchiveResult> {
  assertDesktopRuntime();
  return invoke<ArchiveResult>("archive_sessions", { request });
}
