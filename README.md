# aiobscura

AI Agent Activity Monitor - A tool for observing, querying, and analyzing activity from AI coding assistants.

## Overview

aiobscura ingests activity logs from AI coding assistants (Claude Code, Codex, Aider, Cursor) and provides:

- **Unified storage** - Normalized SQLite database with lossless raw data preservation
- **Session tracking** - Monitor active and historical coding sessions
- **Analytics** - Token usage, tool call patterns, edit churn metrics
- **LLM assessment** - Qualitative analysis via configurable LLM backend
- **Terminal UI** - Real-time monitoring and historical exploration

## Status

This project is under active development. See `docs/` for the architecture and requirements documentation.

## Project Structure

```
aiobscura/
├── aiobscura-core/   # Core library (parsing, storage, analytics)
├── aiobscura-tui/    # Terminal UI binary
├── docs/             # Architecture and requirements
├── examples/         # Usage examples
├── benches/          # Performance benchmarks
└── tests/            # Integration tests
```

## License

MIT OR Apache-2.0
