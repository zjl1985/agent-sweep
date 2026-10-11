# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- OpenCode deletion now removes matching durable event streams in the same transaction and compacts the database; reclaimed bytes reflect measured disk changes.
- Added explicit recovery of space occupied by event streams from previously deleted OpenCode sessions.
- Count UTF-8 bytes and event payloads in OpenCode session sizes; exclude them from database overhead to avoid double-counting.
- Show operation results visibly and report database contention or partial cleanup.

### Added

- Horizontal agent storage comparison with clickable segments.

- Claude Code, OpenCode, Kimi Code, and ZCode provider adapters (scan + delete; archive where supported).
- `rusqlite` (bundled) for OpenCode and ZCode SQLite session stores.

### Changed

- Default to a remembered single agent; scope statistics and sessions to the selection, and collapse other storage details.
- Disable OpenCode Trash mode because SQLite session deletion is irreversible.

- Renamed the application from CodexClear to AgentSweep and the package/binary to
  `agent-sweep`.
- Moved Codex discovery and cleanup behind a `SessionProvider` adapter and registry.
- Added provider-scoped session keys, capability checks, per-provider usage summaries,
  and provider-aware filtering and bulk operations.
- Kept the existing application identifier and local setting keys so upgrades retain
  the current app identity and user preferences.

## [0.1.1] - 2026-08-21

### Added

- Batch archive: selected active sessions can be moved into `~/.codex/archived_sessions/`
  (same layout the Codex CLI uses), via a new "Archive" button in the selection bar.
  Archived sessions stay visible under the "Archived" filter.

## [0.1.0] - 2026-08-17

### Added

- First public release: inspect and clean local ChatGPT / Codex session storage (`~/.codex`)
- Per-session sizes with active/archived split; filter by keyword, status, and age
- Bulk select and delete, safe by default (Trash first, every target validated)
- English and Chinese UI, light and dark themes
