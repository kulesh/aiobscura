# aiobscura: AI Agent Monitor

**A lightweight Unix utility for real-time and batch analysis of AI coding agent activity.**

*Status: Requirements v1.3*

---

## Problem

AI coding agents (Claude Code, Codex, Cursor, Aider, etc.) generate rich operational logs—prompts, tool calls, plans, token usage, errors—but this data is siloed in agent-specific directories with no unified way to observe, query, or analyze it.

---

## Goals

1. **Unified data model** — Canonical schema normalizing all agent activity into a queryable relational store
2. **Incremental ingestion** — Process new log data without re-parsing everything; track watermarks
3. **Real-time monitoring** — Live view showing active sessions and metrics as they happen
4. **Historical analysis** — Exploration of past sessions with filtering and aggregation
5. **Higher-order analytics** — Derived metrics including LLM-assessed qualitative measures
6. **Agent-agnostic** — Plugin architecture supporting multiple agents via parsers
7. **UI-agnostic core** — Clean separation between core engine and presentation; TUI is v1, native macOS GUI planned
8. **Lightweight** — Minimal dependencies, fast startup

## Non-Goals (v1)

- Modifying agent behavior or injecting prompts
- Cloud sync or team features
- Cost estimation (v2)
- Semantic/vector search (v2)
- Daemon mode (v2)
- Scripting plugins (v2) — Lua/WASM for non-Rust plugin authors
- Time-series metrics storage (v2)

---

## Data Sources

| Agent       | Location              | Format        | Priority |
|-------------|-----------------------|---------------|----------|
| Claude Code | `~/.claude/`          | JSONL         | P0       |
| Codex       | `~/.codex/`           | JSON          | P0       |
| Aider       | `.aider.chat.*`       | Markdown/JSON | P1       |
| Cursor      | `~/.cursor/`          | SQLite/JSON   | P1       |

### Agent Auto-Discovery

On startup, aiobscura scans known paths and reports detected agents:

```
$ aiobscura
Discovered agents:
  ✓ claude-code  ~/.claude          (3 projects, 47 sessions)
  ✓ codex        ~/.codex           (12 sessions)
  ✗ aider        (not found)
  ✗ cursor       (not found)

Starting live monitor...
```

Discovery logic:
- Check existence of known directories
- Validate expected structure
- Count available sessions for status display
- Can be overridden via config

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        SOURCE FILES                             │
│  ~/.claude/   ~/.codex/   .aider.*   ~/.cursor/                 │
└──────────────────────────┬──────────────────────────────────────┘
                           │
              ┌────────────▼────────────┐
              │     INGESTION LAYER     │
              │                         │
              │  • Agent Parsers        │
              │    (per-agent logic)    │
              │                         │
              │  • Checkpoint Manager   │
              │    (watermarks, hashes) │
              └────────────┬────────────┘
                           │
              ┌────────────▼────────────┐
              │      STORAGE LAYER      │
              │                         │
              │  SQLite                 │
              │  Layer 1 (Canonical):   │
              │  • sessions             │
              │  • events               │
              │  • plans                │
              │  • checkpoints          │
              │                         │
              │  Layer 2 (Derived):     │
              │  • session_metrics      │
              │  • assessments          │
              │  • plugin_metrics       │
              │  • plugin_runs          │
              └────────────┬────────────┘
                           │
              ┌────────────▼────────────┐
              │     ANALYTICS LAYER     │
              │                         │
              │  • First-order metrics  │
              │  • Higher-order metrics │
              │  • LLM-assessed metrics │
              └────────────┬────────────┘
                           │
                    ┌──────┴──────┐
                    │  CORE API   │  ◄── UI-agnostic interface
                    └──────┬──────┘
                           │
         ┌─────────────────┼─────────────────┐
         │                 │                 │
         ▼                 ▼                 ▼
   ┌───────────┐    ┌───────────┐    ┌───────────┐
   │ TUI (v1)  │    │ macOS GUI │    │  Web UI   │
   │           │    │ (future)  │    │ (future)  │
   └───────────┘    └───────────┘    └───────────┘
```

### Core API Surface

The core exposes a UI-agnostic interface:

| Category        | Operations                                                    |
|-----------------|---------------------------------------------------------------|
| **Queries**     | List sessions, get session detail, search events, get metrics |
| **Subscriptions** | Live event stream, session status changes, sync status      |
| **Commands**    | Trigger sync, refresh metrics, run assessment                 |

### Ingestion Modes

| Mode      | Trigger        | Behavior                                    |
|-----------|----------------|---------------------------------------------|
| **Batch** | `aiobscura sync`   | Scan all sources, process new/changed files |
| **Live**  | `aiobscura`        | Poll DB for updates; ingest only if it owns sync lock |

Process coordination rules:
- `aiobscura-sync` and `aiobscura` do not ingest concurrently for the same database.
- If `aiobscura-sync` is running, `aiobscura` runs read-only and only reads from SQLite.
- If `aiobscura` is running, `aiobscura-sync` exits with an error.

---

## Data Model

aiobscura uses a three-layer data architecture:

| Layer | Purpose | Mutability |
|-------|---------|------------|
| **Layer 0: Raw** | Source files on disk | Immutable (read-only) |
| **Layer 1: Canonical** | Normalized, lossless SQLite tables | Append-only |
| **Layer 2: Derived** | Computed metrics, assessments | Regenerable |

Key principle: **No information loss** from Layer 0 → Layer 1. Every record stores `raw_data` (complete original) plus `metadata` (parsed agent-specific fields).

### Checkpoint Tracking

Enables incremental ingestion without re-parsing:

```sql
CREATE TABLE checkpoints (
    source_path   TEXT PRIMARY KEY,
    agent         TEXT NOT NULL,
    entity_type   TEXT NOT NULL,    -- 'session', 'plan'
    file_hash     TEXT,             -- SHA256 (for change detection)
    byte_offset   INTEGER,          -- last processed position (append-only logs)
    last_rowid    INTEGER,          -- for SQLite sources
    last_event_ts DATETIME,
    updated_at    DATETIME
);
```

Strategy per file type:
- **Append-only (JSONL):** Track byte offset, resume from there
- **Rewritten (JSON):** Track file hash, re-parse if changed
- **SQLite sources:** Track max rowid or timestamp

### Sessions (Layer 1)

```sql
CREATE TABLE sessions (
    id               TEXT PRIMARY KEY,
    agent            TEXT NOT NULL,
    session_type     TEXT NOT NULL,
    project_path     TEXT,
    started_at       DATETIME NOT NULL,
    last_activity_at DATETIME,
    status           TEXT,
    
    -- Lineage
    source_file      TEXT NOT NULL,
    
    -- Lossless capture
    raw_data         JSON,           -- original session metadata
    metadata         JSON            -- parsed agent-specific fields
);
```

Session types:
- **agent_task** — Full agent coding session (human + AI + tools)
- **conversation** — Pure human-AI conversation (no tool use)
- **file_operation** — Batch file operations

Note: Session boundaries are fuzzy. Status is computed from `last_activity_at` relative to current time.

### Events (Layer 1)

```sql
CREATE TABLE events (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id       TEXT NOT NULL REFERENCES sessions(id),
    seq              INTEGER NOT NULL,
    ts               DATETIME NOT NULL,
    event_type       TEXT NOT NULL,

    -- Common parsed fields
    tokens_in        INTEGER,
    tokens_out       INTEGER,
    tool_name        TEXT,
    tool_input       JSON,
    tool_result      TEXT,
    duration_ms      INTEGER,
    content          TEXT,

    -- Lineage
    source_file      TEXT NOT NULL,
    source_offset    INTEGER NOT NULL,
    source_line      INTEGER,

    -- Lossless capture
    raw_data         JSON NOT NULL,  -- complete original record
    metadata         JSON            -- parsed agent-specific fields
);
```

### Event Types

| Type          | Description                     | Key Fields                |
|---------------|---------------------------------|---------------------------|
| `prompt`      | User message to agent           | content, tokens_in        |
| `response`    | Agent reply                     | content, tokens_out       |
| `tool_call`   | Agent invokes a tool            | tool_name, tool_input     |
| `tool_result` | Result of tool execution        | tool_name, tool_result    |
| `plan`        | Agent's stated plan/reasoning   | content                   |
| `error`       | Error or exception              | content, tool_name        |
| `context`     | File/context loaded             | content, metadata         |

### Plans (Layer 1)

Standalone planning artifacts (e.g., `~/.claude/plans/*.md`):

```sql
CREATE TABLE plans (
    id            TEXT PRIMARY KEY,
    agent         TEXT NOT NULL,
    project_path  TEXT,
    title         TEXT,
    content       TEXT NOT NULL,
    created_at    DATETIME NOT NULL,
    updated_at    DATETIME NOT NULL,
    
    -- Lineage
    source_file   TEXT NOT NULL,
    
    -- Lossless capture
    raw_data      JSON,
    metadata      JSON
);
```

Plans are separate from session events—they're persistent documents that may be referenced across multiple sessions.

### Session Metrics (Layer 2)

```sql
CREATE TABLE session_metrics (
    session_id          TEXT PRIMARY KEY REFERENCES sessions(id),
    metric_version      INTEGER NOT NULL,
    computed_at         DATETIME NOT NULL,

    -- First-order aggregations
    total_tokens_in     INTEGER,
    total_tokens_out    INTEGER,
    total_tool_calls    INTEGER,
    tool_call_breakdown JSON,
    error_count         INTEGER,
    duration_ms         INTEGER,

    -- Higher-order derived
    tokens_per_minute   REAL,
    tool_success_rate   REAL,
    edit_churn_ratio    REAL
);
```

### Assessments (Layer 2)

```sql
CREATE TABLE assessments (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id       TEXT NOT NULL REFERENCES sessions(id),
    assessor         TEXT NOT NULL,     -- plugin name
    model            TEXT,              -- LLM model if applicable
    assessed_at      DATETIME NOT NULL,
    scores           JSON NOT NULL,     -- {"sycophancy": 0.3, ...}
    raw_response     TEXT,              -- full LLM response
    prompt_hash      TEXT               -- for cache invalidation
);
```

### Plugin Metrics (Layer 2)

Generic table for custom analytics plugins:

```sql
CREATE TABLE plugin_metrics (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_name      TEXT NOT NULL,
    entity_type      TEXT NOT NULL,     -- 'session', 'event', 'plan', 'global'
    entity_id        TEXT,
    metric_name      TEXT NOT NULL,
    metric_value     JSON NOT NULL,
    computed_at      DATETIME NOT NULL,
    
    UNIQUE(plugin_name, entity_type, entity_id, metric_name)
);
```

### Plugin Runs (Layer 2)

Observability table for debugging plugin health:

```sql
CREATE TABLE plugin_runs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_name       TEXT NOT NULL,
    session_id        TEXT,
    started_at        DATETIME NOT NULL,
    duration_ms       INTEGER NOT NULL,
    status            TEXT NOT NULL,      -- 'success', 'error', 'panic', 'timeout'
    error_message     TEXT,
    metrics_produced  INTEGER,
    input_event_count INTEGER,
    input_token_count INTEGER
);
```

---

## Analytics

Analytics are implemented as **plugins** that consume Layer 1 (canonical) data and produce Layer 2 (derived) metrics.

### Plugin Architecture

```rust
pub trait AnalyticsPlugin {
    fn name(&self) -> &str;
    fn triggers(&self) -> Vec<AnalyticsTrigger>;
    fn analyze_session(&self, session: &Session, events: &[Event], ctx: &AnalyticsContext) 
        -> Result<Vec<MetricOutput>>;
}

pub enum AnalyticsTrigger {
    EventCount(usize),           // After N new events
    Inactivity(Duration),        // After session inactive
    OnDemand,                    // Manual trigger
    Scheduled(Schedule),         // Periodic (daily rollups)
}
```

### Built-in Plugins

**core.first_order** — Basic aggregations (always enabled)

| Metric             | Derivation                          |
|--------------------|-------------------------------------|
| Total tokens       | SUM(tokens_in + tokens_out)         |
| Session duration   | MAX(ts) - MIN(ts)                   |
| Tool call count    | COUNT WHERE event_type = 'tool_call'|
| Error rate         | errors / total_tool_calls           |
| Tokens per tool    | total_tokens / tool_calls           |

**core.edit_churn** — Detects re-edits to same file regions

| Metric             | Logic                                    |
|--------------------|------------------------------------------|
| Edit churn ratio   | re-edits / total edits                   |
| Files touched      | Unique files edited                      |

**core.recovery** — Tracks error recovery patterns

| Metric             | Logic                                    |
|--------------------|------------------------------------------|
| Recovery rate      | Errors followed by successful retry      |
| Iteration velocity | Successful operations per hour           |

**llm.assessment** — Qualitative analysis via LLM (requires LLM config)

| Metric               | Assessment Approach                       |
|----------------------|-------------------------------------------|
| Sycophancy score     | Does agent push back appropriately?       |
| Goal clarity         | How well-defined was the task?            |
| Autonomy level       | Agent initiative vs waiting for direction |
| Code quality signals | Style, patterns, potential issues         |
| Frustration indicators | Signs of user confusion or repetition   |

### Plugin Data Access

Plugins can access:
- **Explicitly parsed fields** (tokens, tool_name, content, etc.)
- **raw_data JSON** for agent-specific fields not in common schema
- **Other canonical tables** via AnalyticsContext

This ensures no analytics is blocked by schema limitations—anything in the raw logs is accessible.

### Custom Plugins

Users can implement custom analytics by:
1. Implementing the `AnalyticsPlugin` trait in Rust
2. Registering in config

Example use cases:
- Track usage of specific tools
- Measure time spent on certain file types
- Custom quality metrics for your codebase

**Escape hatch:** For users who don't want to write Rust, the SQLite database is directly queryable. Run your own SQL or scripts against `~/.local/share/aiobscura/data.db`.

### Plugin Observability

All plugin runs are logged for debugging:

```bash
# Show plugin health summary
$ aiobscura plugins status
PLUGIN              SUCCESS   ERROR   PANIC   AVG_MS
core.first_order    1,247     0       0       12
core.edit_churn     1,245     2       0       45
llm.assessment      89        3       0       2,340

# Show recent errors
$ aiobscura plugins errors llm.assessment

# Show slow runs
$ aiobscura plugins slow --threshold=5000ms
```

Plugins have configurable timeouts and are isolated—a crashing plugin won't take down the core.

### Assessment Triggers

Since session boundaries are fuzzy, assessments trigger on:

1. **Time-based:** Session inactive for N minutes (default: 15)
2. **Event-based:** After N tool calls (default: 20)
3. **On-demand:** User requests via UI

```
┌─────────────────────────────────────────┐
│         Assessment Triggers             │
│                                         │
│  • Inactivity timeout (15 min)          │
│  • Event threshold (20 tool calls)      │
│  • Manual trigger from UI               │
└──────────────────┬──────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────┐
│         Assessment Engine               │
│                                         │
│  • Pull session transcript from DB      │
│  • Build assessment prompt              │
│  • Call configured LLM                  │
│  • Parse structured response            │
│  • Write scores to assessments table    │
└─────────────────────────────────────────┘
```

---

## Configuration

```toml
# ~/.config/aiobscura/config.toml

[llm]
provider = "ollama"                      # or "claude", "openai"
model = "llama3.2"
endpoint = "http://localhost:11434"

[analytics]
# Built-in plugins (enabled by default)
# Disable with: disabled_plugins = ["core.edit_churn"]

[analytics.triggers]
inactivity_minutes = 15
tool_call_threshold = 20

# Custom plugins (optional)
# [[analytics.plugins]]
# name = "custom.my_plugin"
# path = "~/.config/aiobscura/plugins/my_plugin.so"

# If no [llm] section, LLM-based assessments are skipped
```

---

## TUI Design (v1)

### View Hierarchy

```
aiobscura
├── [1] Live View (default)    — Active sessions, real-time updates
├── [2] History View           — Past sessions, filterable
├── [3] Session Detail View    — Drill into one session
├── [4] Analytics View         — Aggregated metrics, trends
└── [?] Help
```

### Navigation

| Key     | Action                    |
|---------|---------------------------|
| `1-4`   | Switch views              |
| `Tab`   | Cycle focus within view   |
| `Enter` | Drill into selected item  |
| `Esc`   | Back / close modal        |
| `f`     | Open filter dialog        |
| `s`     | Cycle sort order          |
| `/`     | Search                    |
| `r`     | Refresh / re-sync         |
| `q`     | Quit                      |

### Live View

```
┌─ aiobscura ─────────────────────────────────────────────── 12:34:05 ─┐
│ [1]Live  [2]History  [3]Detail  [4]Analytics            ◉ synced │
├──────────────────────────────────────────────────────────────────┤
│ ACTIVE SESSIONS                                                  │
│ ┌──────────────────────────────────────────────────────────────┐ │
│ │ AGENT       SESSION   PROJECT        TOKENS   TOOLS  DUR     │ │
│ │ ─────────────────────────────────────────────────────────────│ │
│ │►claude-code a3f2c1    ~/myapp        12.4k    47     23m     │ │
│ │ codex       b7e9d0    ~/api-server    3.2k    12      8m     │ │
│ └──────────────────────────────────────────────────────────────┘ │
├──────────────────────────────────────────────────────────────────┤
│ RECENT EVENTS (a3f2c1)                                           │
│ ┌──────────────────────────────────────────────────────────────┐ │
│ │ 12:34:02  tool_call   edit_file   src/main.rs                │ │
│ │ 12:33:58  response    "I'll update the error handling..."    │ │
│ │ 12:33:45  tool_call   bash        cargo build                │ │
│ │ 12:33:30  prompt      "fix the compilation errors"           │ │
│ └──────────────────────────────────────────────────────────────┘ │
├──────────────────────────────────────────────────────────────────┤
│ Total: 2 active │ 15.6k tokens │ 59 tools │ 0 errors            │
│ [q]uit [1-4]views [f]ilter [s]ort [Enter]detail [r]efresh [?]   │
└──────────────────────────────────────────────────────────────────┘
```

### History View

```
┌─ aiobscura ─────────────────────────────────────────────── 12:34:05 ─┐
│ [1]Live  [2]History  [3]Detail  [4]Analytics            ◉ synced │
├──────────────────────────────────────────────────────────────────┤
│ FILTERS: agent=all  since=7d  project=*           [f] to edit   │
├──────────────────────────────────────────────────────────────────┤
│ │ DATE        AGENT       PROJECT        TOKENS   DUR    SCORE │ │
│ │ ────────────────────────────────────────────────────────────  │ │
│ │ Dec 5 11:00 claude-code ~/myapp        45.2k    34m     0.82 │ │
│ │ Dec 5 09:15 claude-code ~/myapp        12.1k    12m     0.75 │ │
│ │ Dec 4 16:30 codex       ~/api-server   28.3k    45m     0.91 │ │
│ │ Dec 4 14:00 aider       ~/scripts       8.7k     8m     ──   │ │
│ │ Dec 4 10:00 claude-code ~/myapp        67.4k    1h      0.68 │ │
│ │ ...                                                          │ │
├──────────────────────────────────────────────────────────────────┤
│ Showing 47 sessions │ 892k tokens total                         │
│ [q]uit [1-4]views [f]ilter [s]ort [Enter]detail [/]search [?]   │
└──────────────────────────────────────────────────────────────────┘
```

---

## Decisions Log

| Decision                   | Choice                                                          |
|----------------------------|-----------------------------------------------------------------|
| Content storage            | Full prompt/response text in DB                                 |
| Lossless capture           | Store complete `raw_data` JSON for every record                 |
| Data architecture          | Three layers: Raw → Canonical → Derived                         |
| Analytics architecture     | Plugin system; built-in + custom plugins                        |
| Plugin isolation           | Catch panics, timeouts, full observability via `plugin_runs`    |
| Custom analytics escape    | SQLite DB queryable externally (no scripting layer in v1)       |
| Semantic search            | Deferred to v2                                                  |
| LLM assessment trigger     | Automatic when LLM configured                                   |
| Privacy mode               | None—personal tool                                              |
| UI architecture            | Core API separated from UI                                      |
| Session boundary detection | Fuzzy; time-based + event-based triggers                        |
| Agent discovery            | Auto-scan known paths on startup                                |
| Cost estimation            | Deferred to v2                                                  |
| Time-series metrics        | Deferred to v2                                                  |
| Core deployment model      | Embedded library for v1; daemon wrapper in v2                   |

---

## Open Questions

### Ingestion

1. **File watching strategy:** OS watchers (inotify/kqueue) vs polling?
2. **Consistency checks:** In live mode, how often to run full sync vs incremental only?

### Analytics

3. **Assessment prompts:** What questions yield reliable qualitative scores? Needs prototyping.
4. **Minimum threshold:** Skip assessment for trivial sessions (< N events)?

### TUI

5. **Refresh rate:** How often to update live view?
6. **Large sessions:** Paginate or summarize sessions with 1000+ events?

---

## References

- Claude Code logs: `~/.claude/projects/*/conversations/`
- Codex logs: `~/.codex/`
- ratatui (TUI framework): github.com/ratatui/ratatui
- htop (TUI patterns): github.com/htop-dev/htop
