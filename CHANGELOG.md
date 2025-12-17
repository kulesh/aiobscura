# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4] - 2024-12-17

### Added

- **Analytics Plugin Framework**
  - New `aiobscura-analyze` CLI for running analytics plugins
  - Edit churn analyzer with metrics: edit count, unique files, churn ratio, high churn detection
  - Line change metrics (lines added/removed)
  - Edits by file extension breakdown
  - First-try rate efficiency metric
  - Smart high churn detection with burst analysis
  - Thread-level analytics with session/thread toggle in UI

- **UI Improvements**
  - Redesigned Live View as development dashboard with multi-window stats
  - Environment health panel showing assistant status
  - Adaptive activity heatmap
  - Session Detail view with merged timeline
  - Timestamps in Thread Detail view
  - Caller labels for CLI-invoked prompts vs human input

### Changed

- Replaced Threads tab with Sessions tab in Project view
- Introduced `AuthorRole::Caller` to distinguish CLI invocations from human input

### Fixed

- **Timestamps**: Implemented dual timestamp model (`emitted_at`/`observed_at`) so "Last Updated" shows actual event time, not ingestion time
- Added `last_activity_at` column to threads table for accurate activity tracking
- Codex parser: deduplicate messages, improve semantic accuracy
- Codex parser: label first user prompt as [caller] (CLI invocation)
- Codex parser: use [caller] for system-injected context, improve [snapshot] display
- Tool call/result display in Session Detail view
- UTF-8 safe string truncation for multi-byte characters

## [0.1.3] - 2024-12-10

### Added

- Pre-push git hook for running CI checks locally

### Fixed

- CI now uses stable Rust only (removed MSRV complexity)

## [0.1.2] - 2024-12-09

### Changed

- Renamed crate from `aiobscura-tui` to `aiobscura` for cleaner homebrew formula name
- Homebrew formula is now `brew install kulesh/tap/aiobscura`

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

[Unreleased]: https://github.com/kulesh/aiobscura/compare/v0.1.4...HEAD
[0.1.4]: https://github.com/kulesh/aiobscura/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/kulesh/aiobscura/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/kulesh/aiobscura/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/kulesh/aiobscura/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/kulesh/aiobscura/releases/tag/v0.1.0
