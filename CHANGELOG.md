# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
