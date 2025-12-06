# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Note**: This project uses [bd (beads)](https://github.com/steveyegge/beads)
for issue tracking. Use `bd` commands instead of markdown TODOs.
See AGENTS.md for workflow details.

## Project Overview

**aiobscura** is an AI agent activity monitor - a unified tool to observe, query, and analyze logs from multiple AI coding agents (Claude Code, Codex, Aider, Cursor).

See `docs/aiobscura-requirements.md` for full requirements and `docs/aiobscura-architecture.md` for architecture details.

**Workspace structure:**
- `aiobscura-core/` - Core library (types, DB, ingestion, analytics, API)
- `aiobscura-tui/` - Terminal UI using ratatui

Tooling: mise (Rust toolchain), cargo-nextest (tests), cargo-watch (dev), clippy, rustfmt

## Key Commands

```bash
# Build
cargo build                    # Debug build
cargo build --release          # Optimized release build

# Run
cargo run                      # Run the TUI

# Test
cargo nextest run              # Run all tests (preferred)
cargo nextest run <test_name>  # Run specific test
cargo nextest run -p aiobscura-core  # Test core library only
cargo test --doc               # Run doctests

# Code quality
cargo fmt                      # Format code
cargo clippy                   # Lint
cargo check --all-targets      # Quick syntax check

# Development
cargo watch -x run             # Auto-rebuild and run on changes
cargo doc --workspace --open   # Build and view docs
```

## Architecture

```
aiobscura/
├── aiobscura-core/src/   # Core library
│   ├── types.rs          # Domain types (Session, Event, Plan)
│   ├── db/               # SQLite storage layer
│   ├── ingest/           # Parser framework
│   │   └── parsers/      # Agent-specific parsers
│   ├── analytics/        # Metrics and assessments
│   └── api/              # Public API for UIs
├── aiobscura-tui/src/    # Terminal UI
│   ├── views/            # Live, history, detail, analytics views
│   └── widgets/          # Reusable UI components
├── tests/                # Integration tests
├── examples/             # Usage examples
└── benches/              # Performance benchmarks
```

**Error handling pattern:**
- Core library: Custom `Error` enum with `thiserror`, exposes `Result<T>` alias
- TUI binary: Uses `anyhow::Result` with `?` operator for ergonomic error propagation
- When you come across @HeyClaude in plan files the subsequent comment is for you Claude Code. Multi-line comments will end with two empty lines.