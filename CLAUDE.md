# CLAUDE.md

This file provides guidance to Claude Code when working in this repository.

Note: this project uses [bd (beads)](https://github.com/steveyegge/beads) for issue tracking.
Use `bd` commands instead of markdown TODO lists.

## Primary Guidance

`AGENTS.md` is the source of truth for workflow and standards.

## Project Overview

`aiobscura` is an AI agent activity monitor that ingests logs from coding assistants and provides storage, analytics, and a TUI.

Core references:

- `docs/aiobscura-requirements.md`
- `docs/aiobscura-architecture.md`

## Crate Layout

- `aiobscura-core/`: domain types, config, ingest, db, analytics, collector
- `aiobscura/`: TUI and CLI binaries
- `aiobscura-wrapped/`: wrapped summary CLI

## Build, Test, and Quality

Prefer CI-equivalent commands:

```bash
cargo build --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo doc --no-deps --all-features
```

## Useful Run Commands

```bash
# TUI
cargo run -p aiobscura --bin aiobscura

# Sync
cargo run -p aiobscura --bin aiobscura-sync -- --help

# Analytics
cargo run -p aiobscura --bin aiobscura-analyze -- --help

# Collector
cargo run -p aiobscura --bin aiobscura-collector -- --help

# Wrapped
cargo run -p aiobscura-wrapped -- --help
```

Runtime coordination (same DB path):
- `aiobscura-sync` exits if `aiobscura` is already running.
- `aiobscura` starts in read-only mode if `aiobscura-sync` already holds the sync lock.

## Error Handling Pattern

- `aiobscura-core`: custom `Error` + `Result<T>` alias
- binaries: `anyhow::Result` for top-level orchestration
