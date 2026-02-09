# GitHub Copilot Instructions for aiobscura

## Issue Tracking

This project uses **bd (beads)** for issue tracking.
Run `bd prime` for workflow context, or install hooks (`bd hooks install`) for auto-injection.

Quick reference:

- `bd ready --json` - Find unblocked work
- `bd create "Title" --type task --priority 2 --json` - Create issue
- `bd update <id> --status in_progress --json` - Claim issue
- `bd close <id> --reason "Done" --json` - Complete issue
- `bd sync` - Sync with git (run at session end)

## Repository Layout

```text
aiobscura/
├── aiobscura-core/        # Core library: types, config, ingest, db, analytics, collector
├── aiobscura/             # TUI + CLI binaries (aiobscura, sync, analyze, collector, debuggers)
├── aiobscura-wrapped/     # Wrapped summary CLI
├── docs/                  # Product, architecture, format specs, design docs
├── aiobscura/tests/       # CLI acceptance/integration tests
├── tests/                 # Currently empty (reserved for top-level integration tests)
└── .beads/issues.jsonl    # Git-synced issue storage
```

## Build and Verify

Use commands aligned with CI:

```bash
mise install
cargo build --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo doc --no-deps --all-features
```

## Source of Truth

- `AGENTS.md` is the primary collaborator guidance.
- Keep this file aligned with `AGENTS.md` whenever workflow or structure changes.

## Runtime Coordination

- `aiobscura-sync` exits if `aiobscura` is already running for the same DB path.
- `aiobscura` starts read-only if `aiobscura-sync` already holds the sync lock.
