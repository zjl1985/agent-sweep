# AgentSweep

A local-first desktop app for inspecting and safely cleaning AI coding-agent sessions. Agent-specific storage formats and operations live behind provider adapters; the first supported provider is Codex (`~/.codex`). Built with Tauri 2 (Rust backend + Preact frontend), with all filesystem access confined to Rust.

<p align="center">
  <img src="docs/screenshot-light.png#gh-light-mode-only" alt="AgentSweep — light theme" width="820">
  <img src="docs/screenshot-dark.png#gh-dark-mode-only" alt="AgentSweep — dark theme" width="820">
</p>

> The screenshots show synthetic demo data, not real sessions.

## Features

- **Provider adapters** — a registry discovers agent stores and normalizes sessions without leaking provider-specific paths into shared orchestration
- **Bilingual UI** — English and Chinese, remembered locally
- **Light & dark themes** — follows the system appearance until overridden
- **Storage stats** — aggregate session usage plus per-provider totals
- **Per-session sizes** — sortable by size or update time, with title, workspace, provider, and status
- **Filters and bulk selection** — search, active/archived, age, and select-all-filtered
- **Capability-aware operations** — Archive and Delete are only enabled when the owning provider supports them
- **Safe cleanup** — Trash by default or permanent deletion after explicit confirmation

## Supported providers

| Provider | Discover | Archive | Trash | Permanent delete |
| --- | --- | --- | --- | --- |
| Codex | Yes | Yes | Yes | Yes |

Codex covers the session storage shared by the ChatGPT app, Codex desktop app, and [Codex CLI](https://developers.openai.com/codex/). Additional agents can be added as isolated adapters without changing shared UI/API semantics.

## Safety design

- The frontend identifies targets by opaque `providerId + sessionKey` pairs; absolute paths never leave Rust
- Each adapter independently validates its allowed roots, file types, canonical paths, and symlink policy
- Unsupported capabilities are rejected by the backend even if a client submits the command directly
- The Codex scanner only reads a bounded number of leading JSONL lines (512 KB per line cap)
- Codex deletion only accepts regular `rollout-*.jsonl` files inside `sessions/` or `archived_sessions/`; index and history files are never modified
- Empty Codex date directories are pruned after cleanup, but storage roots are retained

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

- Only the Codex adapter is included today
- Codex index/history orphan entries are intentionally left untouched after rollout deletion
- Codex titles depend on `session_index.jsonl`; missing entries fall back to workspace and date
- Provider roots are detected from conventional locations and are not yet configurable

## License

[MIT](LICENSE)
