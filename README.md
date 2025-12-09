# aiobscura

[![CI](https://github.com/kulesh/aiobscura/actions/workflows/ci.yml/badge.svg)](https://github.com/kulesh/aiobscura/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE-MIT)

AI Agent Activity Monitor - A tool for observing, querying, and analyzing activity from AI coding assistants.

## Overview

aiobscura ingests activity logs from AI coding assistants (Claude Code, Codex, Aider, Cursor) and provides:

- **Unified storage** - Normalized SQLite database with lossless raw data preservation
- **Session tracking** - Monitor active and historical coding sessions
- **Analytics** - Token usage, tool call patterns, edit churn metrics
- **LLM assessment** - Qualitative analysis via configurable LLM backend (in development)
- **Terminal UI** - Real-time monitoring and historical exploration

## Installation

### From source

Requires Rust 1.70 or later.

```bash
git clone https://github.com/kulesh/aiobscura.git
cd aiobscura
cargo build --release
```

The binary will be at `target/release/aiobscura`.

### Running

```bash
# Run the TUI
cargo run

# Or directly
./target/release/aiobscura
```

On first run, aiobscura will:
1. Scan for installed AI agents (Claude Code, Codex, etc.)
2. Create a SQLite database at `~/.local/share/aiobscura/data.db`
3. Ingest available session logs
4. Launch the terminal UI

## Supported Agents

| Agent       | Location        | Status      |
|-------------|-----------------|-------------|
| Claude Code | `~/.claude/`    | Supported   |
| Codex       | `~/.codex/`     | Supported   |
| Aider       | `.aider.*`      | Planned     |
| Cursor      | `~/.cursor/`    | Planned     |

## Project Structure

```
aiobscura/
├── aiobscura-core/     # Core library (parsing, storage, analytics)
├── aiobscura-tui/      # Terminal UI binary
├── aiobscura-wrapped/  # "Year in Review" analytics tool
├── docs/               # Architecture and requirements
└── tests/              # Integration tests
```

## Development

```bash
# Build
cargo build

# Run tests
cargo nextest run  # or cargo test

# Lint
cargo clippy

# Format
cargo fmt
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed guidelines.

## Status

This project is under active development. The analytics framework and LLM assessment features are in progress. See the [docs/](docs/) folder for architecture and requirements documentation.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to get started.
