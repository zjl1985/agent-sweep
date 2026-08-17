# CodexClear

A local-first desktop app for inspecting and cleaning the local session storage shared by the **ChatGPT app**, the **Codex desktop app**, and the **[Codex CLI](https://developers.openai.com/codex/)** — all three keep their chat/session rollout files under `~/.codex`. Built with Tauri 2 (Rust backend + Preact frontend), following the architecture conventions of the baton handoff app: all file system work lives in Rust, the web layer is presentation only.

<p align="center">
  <img src="docs/screenshot-light.png#gh-light-mode-only" alt="CodexClear — light theme" width="820">
  <img src="docs/screenshot-dark.png#gh-dark-mode-only" alt="CodexClear — dark theme" width="820">
</p>

> The screenshots show synthetic demo data, not real sessions.

## Features

- **Bilingual UI** — English by default with an in-app toggle for Chinese; the choice is remembered locally
- **Light & dark themes** — follows the system appearance until you pick one with the header toggle; the choice is remembered locally
- **Storage stats** — total size of all Codex sessions, split by active vs. archived, plus the overall `~/.codex` footprint
- **Per-session sizes, sorted** — every `rollout-*.jsonl` under `sessions/YYYY/MM/DD/` and `archived_sessions/`, sorted by size (or updated time), with a relative size bar, title, working directory, and timestamps
- **Filters** — keyword search (title / directory / session id), active/archived, older-than-7/30/90-days, and "select all filtered" for bulk operations
- **Safe cleanup** — delete selected sessions to the system Trash (default) or permanently, after an explicit confirmation
- **Read-only overview** — other large `~/.codex` entries (log database, plugins, generated images, …) are shown for context but never touched

## Safety design

- The scanner only stats files and reads a bounded number of leading lines (512 KB per line cap) to extract `session_meta`; file bodies are never loaded
- The frontend only exchanges paths relative to `~/.codex` (e.g. `sessions/2026/08/15/rollout-….jsonl`) — a session id is not unique per file, so selection and deletion key off the relative path; absolute paths never leave the Rust side
- Every deletion is validated: the file name must match `rollout-*.jsonl`, the target must be a regular file (symbolic links are rejected), and the canonicalized path must stay inside `sessions/` or `archived_sessions/`
- After deletion, empty `YYYY/MM/DD` directories are pruned bottom-up; the roots themselves are never removed
- Deleting only removes rollout files; Codex index files (`session_index.jsonl`, `history.jsonl`) are left untouched

## Architecture

```
codex_clear/
├── src/                     # Preact UI (display + filtering only)
│   ├── App.tsx              # single-page UI
│   ├── api.ts               # Tauri invoke wrappers
│   ├── types.ts             # mirrors of the Rust models
│   └── components/Icons.tsx
└── src-tauri/src/
    ├── lib.rs               # 2 tauri commands (scan_sessions, delete_sessions)
    ├── models.rs            # serde models shared with the frontend
    ├── discovery.rs         # session scanning + size stats + usage overview
    └── cleanup.rs           # id resolution, validation, trash/permanent delete
```

## Development

Prerequisites: [Rust](https://rustup.rs/), Node.js, and [pnpm](https://pnpm.io/).

```sh
pnpm install
pnpm tauri dev      # run the desktop app in dev mode
```

Note: `tauri dev` runs an unbundled binary on macOS, so the Dock shows a generic icon during development. The app icon appears in the bundled `.app` produced by a release build.

```sh
pnpm check                    # TypeScript type check
cargo test --manifest-path src-tauri/Cargo.toml   # Rust unit tests
pnpm tauri build --bundles app   # build the release .app
```

A manual diagnostic test runs the scanner against your real `~/.codex`:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib scan_real -- --ignored --nocapture
```

## Known limitations

- Orphaned entries in `session_index.jsonl` / `history.jsonl` are not cleaned up after deletion (they are tiny, and writing Codex-owned files is deliberately out of scope)
- Session titles come from `session_index.jsonl`; sessions without an index entry fall back to "directory · date"
- The `~/.codex` overview list is a fixed set of well-known directories

## License

[MIT](LICENSE)
