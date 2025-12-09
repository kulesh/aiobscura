# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2024-12-09

### Changed

- Consolidated distribution to single homebrew formula with two binaries:
  - `aiobscura` - Terminal UI
  - `aiobscura-sync` - Background watcher/sync daemon

### Removed

- `aiobscura-wrapped` from distribution (still available in codebase)
- Debug binaries from distribution

## [0.1.0] - 2024-12-09

### Added

- Initial public release
- **Core library** (`aiobscura-core`)
  - SQLite storage layer with sessions, events, and plans tables
  - Claude Code log parser with full JSONL support
  - Codex log parser
  - Incremental ingestion with checkpoint tracking
  - Session and event type definitions
- **Terminal UI** (`aiobscura-tui`)
  - Live view showing active sessions and recent events
  - History view for browsing past sessions
  - Detail view for individual session exploration
  - Dashboard with token usage and tool call statistics
  - Project-based organization with tab navigation
  - Assistant personality coloring
- **Wrapped analytics** (`aiobscura-wrapped`)
  - "Year in Review" style statistics generator
  - Time patterns analysis (peak hours, busiest days)
  - Streak tracking
  - Personality classification
- Documentation
  - Architecture document
  - Requirements specification
  - Log format documentation for supported agents

### Not Yet Implemented

- Analytics plugin framework (Phase 4)
- LLM-based assessment infrastructure (Phase 5)
- Aider parser
- Cursor parser

[Unreleased]: https://github.com/kulesh/aiobscura/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/kulesh/aiobscura/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/kulesh/aiobscura/releases/tag/v0.1.0
