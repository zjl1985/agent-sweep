# AgentSweep

A local-first desktop app for inspecting and safely cleaning AI coding-agent sessions. Agent-specific storage formats and operations live behind provider adapters for Codex, Claude Code, OpenCode, Kimi Code, and ZCode. Built with Tauri 2 (Rust backend + Preact frontend), with all filesystem access confined to Rust.

<p align="center">
  <img src="docs/screenshot-light.png#gh-light-mode-only" alt="AgentSweep — light theme" width="820">
  <img src="docs/screenshot-dark.png#gh-dark-mode-only" alt="AgentSweep — dark theme" width="820">
</p>

> The screenshots show synthetic demo data, not real sessions.

## Features

- **Provider adapters** — a registry discovers agent stores and normalizes sessions without leaking provider-specific paths into shared orchestration
- **Bilingual UI** — English and Chinese, remembered locally
- **Light & dark themes** — follows the system appearance until overridden
- **Storage stats** — selected-agent statistics and a horizontal storage comparison across agents
- **Per-session sizes** — sortable by size or update time, with title, workspace, and status
- **Filters and bulk selection** — search, active/archived, age, and select-all-filtered
- **Capability-aware operations** — Archive and Delete are only enabled when the owning provider supports them
- **Safe cleanup** — Trash by default or permanent deletion after explicit confirmation

## Supported providers

| Provider | Root | Discover | Archive | Trash | Permanent delete |
| --- | --- | --- | --- | --- | --- |
| Codex | `~/.codex` | Yes | Yes | Yes | Yes |
| Claude Code | `~/.claude` | Yes | No | Yes | Yes |
| OpenCode | `~/.local/share/opencode` | Yes | Yes | No | Yes |
| Kimi Code | `~/.kimi-code` | Yes | Yes | Yes | Yes |
| ZCode | `~/.zcode` | Yes | Yes | No | No |

OpenCode stores sessions in SQLite and supports permanent deletion only; its Trash option is disabled.

Codex covers the session storage shared by the ChatGPT app, Codex desktop app, and [Codex CLI](https://developers.openai.com/codex/). Additional agents can be added as isolated adapters without changing shared UI/API semantics.

## Safety design

- The frontend identifies targets by opaque `providerId + sessionKey` pairs; absolute paths never leave Rust
- Each adapter independently validates its allowed roots, file types, canonical paths, and symlink policy
- Unsupported capabilities are rejected by the backend even if a client submits the command directly
- The Codex scanner only reads a bounded number of leading JSONL lines (512 KB per line cap)
- Codex deletion only accepts regular `rollout-*.jsonl` files inside `sessions/` or `archived_sessions/`; index and history files are never modified
- Empty Codex date directories are pruned after cleanup, but storage roots are retained
- Claude Code deletes UUID transcripts, sibling subagent/tool files, and matching file-history, session-env, tasks, and todo artifacts; never touches shared history or credentials
- OpenCode deletes selected sessions and their durable event streams transactionally, then checkpoints and compacts SQLite; reported reclaimed space is measured from database/WAL files
- OpenCode “Reclaim database space” removes known event streams for already-deleted sessions and compacts storage, preserving current sessions; close OpenCode first
- OpenCode session sizes count UTF-8 message, part, and event payload bytes; database overhead and deleted-session event residue are shown separately
- Kimi Code archives via `state.json`; deletes whole session directories under `sessions/` after symlink checks
- ZCode deletion is disabled: its tasks index and separate session database require coordinated cleanup; archive remains available.

File-based cleanup groups session-owned artifacts before disposal and rolls moves back on staging or Trash failure. Trash bundles include `restore.json` mapping numbered items to original paths; restore items using that manifest. Moving to Trash does not reclaim disk space until Trash is emptied. Permanent cleanup errors report partial progress and retain recovery paths. Shared agent indexes remain untouched, following the conservative ownership boundary in [Magpie](https://github.com/yetone/magpie/blob/main/internal/sessions/manage.go).

## Token usage

Session rows show model names, uncached input/output tokens and cache reads/writes, separately from disk size. The summary shows usage coverage and recorded token totals. No pricing or cost estimation is performed.

- Codex: deduplicate native response records, prefer them over overlapping legacy counters, and mark incomplete histories.
- Claude Code: merge message usage snapshots and include subagent logs; ignore zero-usage synthetic notices.
- OpenCode / ZCode: read assistant message usage from SQLite and normalize their different cache and reasoning conventions.
- Kimi Code: read agent wire usage records rather than context-window measurements.
- Scans stay local and cache parsed usage by file size and modification time. Initial scans read full usage history and may take longer.

## Architecture

```text
agent-sweep/
├── src/                         # Preact UI
│   ├── App.tsx                  # display, filtering, capability-aware actions
│   ├── api.ts                   # Tauri invoke wrappers
│   ├── types.ts                 # mirrors of shared Rust models
│   └── components/Icons.tsx
└── src-tauri/src/
    ├── lib.rs                   # Tauri commands routed through the registry
    ├── models.rs                # provider-neutral serde models
    └── providers/
        ├── mod.rs               # SessionProvider trait + registry/orchestration
        └── codex/               # Codex discovery, validation, archive, and delete
```

## Development

Prerequisites: [Rust](https://rustup.rs/), Node.js, and [pnpm](https://pnpm.io/).

```sh
pnpm install
pnpm tauri dev
```

Validation and release builds:

```sh
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml
pnpm tauri build --bundles app
```

On macOS, `tauri dev` uses an unbundled binary with a generic Dock icon. The release `.app` contains the configured icon.

## Adding a provider

Implement the Rust `SessionProvider` contract, declare capabilities, and register the adapter. Start new providers as discovery-only; enable destructive operations only after adding provider-specific path, file-type, and symlink tests.

## Known limitations

- Codex index/history orphan entries are intentionally left untouched after rollout deletion
- Codex titles depend on `session_index.jsonl`; missing entries fall back to workspace and date
- Claude Code has no first-class archive concept, so archive is disabled
- Claude project folder names encode paths lossily (`-` → `/`); workspace labels are heuristic
- OpenCode SQLite rows cannot be moved to the OS trash; only explicitly confirmed permanent deletion is supported
- OpenCode compaction needs free temporary disk space and may be blocked by active connections; incomplete reclamation is reported and can be retried
- ZCode never scans `~/Library/Application Support/ZCode` (Chromium profile)
- Provider roots are detected from conventional locations and are not yet configurable

## License

[MIT](LICENSE)
