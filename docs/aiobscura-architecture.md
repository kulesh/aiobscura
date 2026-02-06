# aiobscura: Architecture Document

*Based on Requirements v1.3*

---

## Overview

aiobscura is structured as a **Rust workspace** with three crates:

1. **aiobscura-core** — Library containing all business logic, data access, and analytics
2. **aiobscura** — Terminal UI and operational CLI binaries (`aiobscura`, `aiobscura-sync`, `aiobscura-analyze`, `aiobscura-collector`)
3. **aiobscura-wrapped** — Wrapped summary CLI

This separation ensures future frontends (macOS GUI, web) can link against the same core library.

---

## Crate Structure

```
aiobscura/
├── Cargo.toml                 # Workspace manifest
│
├── aiobscura-core/            # Library crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs             # Public API exports
│       ├── config.rs          # XDG config/path helpers
│       ├── types.rs           # Domain types (Project, Session, Thread, Message, Plan, ...)
│       ├── format.rs          # Shared formatting helpers
│       ├── logging.rs         # Tracing/logging setup
│       ├── db/                # SQLite schema + repository
│       ├── ingest/            # Ingest coordinator + assistant parsers
│       ├── analytics/         # Plugin engine + built-in plugins + wrapped stats
│       └── collector/         # Catsyphon client/publisher integration
│
├── aiobscura/                 # TUI + operational CLI crate
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs            # aiobscura (TUI)
│       ├── app.rs             # TUI application state + data loading
│       ├── ui.rs              # TUI rendering
│       ├── thread_row.rs      # TUI row view models
│       ├── sync.rs            # aiobscura-sync
│       ├── analyze.rs         # aiobscura-analyze
│       ├── collector.rs       # aiobscura-collector
│       ├── debug_claude.rs    # parser debug CLI
│       ├── debug_codex.rs     # parser debug CLI
│       ├── debug_claude_watch.rs
│       └── debug_codex_watch.rs
│
├── aiobscura-wrapped/         # Wrapped summary CLI crate
│   ├── Cargo.toml
│   └── src/main.rs
│
└── README.md
```

---

## Module Responsibilities

### aiobscura-core

#### `config`
- Load/parse `~/.config/aiobscura/config.toml`
- Provide defaults for missing values
- Validate LLM configuration

#### `ingest`
- Discover source files using parser-specific glob patterns
- Parse assistant logs incrementally with checkpoints
- Normalize parsed data into canonical Layer 1 tables

#### `types`
Core domain types shared across modules:

```rust
// ============================================
// Sessions - multiple types
// ============================================

pub struct Session {
    pub id: String,
    pub agent: AgentType,
    pub session_type: SessionType,
    pub project_path: Option<PathBuf>,
    pub started_at: DateTime<Utc>,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub status: SessionStatus,
    
    // Lineage
    pub source_file: String,
    
    // Lossless capture
    pub raw_data: Option<serde_json::Value>,  // original session metadata
    pub metadata: serde_json::Value,           // parsed agent-specific fields
}

pub enum SessionType {
    AgentTask,       // Full agent coding session (human + AI + tools)
    Conversation,    // Pure human-AI conversation (no tool use)
    FileOperation,   // Batch file operations
    Unknown,
}

pub enum SessionStatus {
    Active,      // activity within last 5 min
    Inactive,    // 5-60 min since last activity
    Stale,       // >60 min since last activity
}

// ============================================
// Events within sessions
// ============================================

pub struct Event {
    pub id: i64,
    pub session_id: String,
    pub seq: i32,
    pub ts: DateTime<Utc>,
    pub event_type: EventType,
    pub tokens_in: Option<i32>,
    pub tokens_out: Option<i32>,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_result: Option<String>,
    pub duration_ms: Option<i32>,
    pub content: Option<String>,
    
    // Lineage
    pub source_file: String,
    pub source_offset: i64,
    pub source_line: Option<i32>,
    
    // Lossless capture
    pub raw_data: serde_json::Value,  // complete original record
    pub metadata: serde_json::Value,   // parsed agent-specific fields
}

pub enum EventType {
    Prompt,
    Response,
    ToolCall,
    ToolResult,
    Plan,
    Error,
    Context,
}

// ============================================
// Plans - standalone artifacts
// ============================================

pub struct Plan {
    pub id: String,
    pub agent: AgentType,
    pub project_path: Option<PathBuf>,
    pub title: Option<String>,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    
    // Lineage
    pub source_file: String,
    
    // Lossless capture
    pub raw_data: Option<serde_json::Value>,  // original if from structured source
    pub metadata: serde_json::Value,
}

// ============================================
// Metrics
// ============================================

pub struct SessionMetrics {
    pub session_id: String,
    pub metric_version: i32,
    pub computed_at: DateTime<Utc>,
    
    // First-order
    pub total_tokens_in: i32,
    pub total_tokens_out: i32,
    pub total_tool_calls: i32,
    pub tool_call_breakdown: HashMap<String, i32>,
    pub error_count: i32,
    pub duration_ms: i64,
    
    // Higher-order
    pub tokens_per_minute: f64,
    pub tool_success_rate: f64,
    pub edit_churn_ratio: f64,
}

// LLM assessments are stored separately in the assessments table
pub struct Assessment {
    pub id: i64,
    pub session_id: String,
    pub assessor: String,           // plugin name
    pub model: Option<String>,      // LLM model if applicable
    pub assessed_at: DateTime<Utc>,
    pub scores: serde_json::Value,  // {"sycophancy": 0.3, "clarity": 0.8, ...}
    pub raw_response: Option<String>,
    pub prompt_hash: Option<String>,
}

// ============================================
// Agent types
// ============================================

pub enum AgentType {
    ClaudeCode,
    Codex,
    Aider,
    Cursor,
}
```

#### `db`
- SQLite via `rusqlite`
- Schema migrations on startup
- Repository pattern for queries and inserts

#### `ingest`
- **Coordinator:** orchestrates parser execution and sync bookkeeping
- **Checkpointing:** byte-offset based incremental parsing for append-only logs
- **Parsers:** `claude.rs` and `codex.rs` (Aider/Cursor planned)

#### `analytics`
- **Engine:** plugin runtime (`AnalyticsEngine`) with per-plugin run tracking
- **Built-ins:** `core.first_order` and `core.edit_churn`
- **Outputs:** writes plugin metrics to Layer 2 derived tables
- **Wrapped:** year/month summary generation used by TUI and wrapped CLI

#### `collector`
- Optional Catsyphon integration with batching/retry
- Local-first flow: local DB write first, publish after
- Supports resume/flush/status workflows through `aiobscura-collector`

### aiobscura

#### `main` + `app` + `ui`
- TUI event loop, state machine, rendering, and navigation
- Reads canonical/derived data via `aiobscura-core` repository APIs

#### `sync`
- `aiobscura-sync`: one-shot or polling sync mode
- Displays progress and summary counts per run

#### `analyze`
- `aiobscura-analyze`: plugin execution and metrics reporting
- Supports plugin listing and JSON/text output modes

#### `collector`
- `aiobscura-collector`: collector status/resume/flush/session diagnostics

---

## Data Flow Architecture

The data pipeline has three distinct layers, each with a specific purpose:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              LAYER 0: RAW                                   │
│                                                                             │
│   Source files on disk (never modified, treated as immutable)               │
│   ~/.claude/projects/*/conversations/*.jsonl                                │
│   ~/.claude/plans/*.md                                                      │
│   ~/.codex/sessions/*.json                                                  │
│                                                                             │
│   Purpose: Ground truth, audit trail, reprocessing capability               │
└─────────────────────────────────┬───────────────────────────────────────────┘
                                  │
                                  │ Parser (per-agent)
                                  │ - Extracts all fields
                                  │ - Preserves unknown fields in `raw_data`
                                  │ - Tracks source lineage
                                  ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           LAYER 1: CANONICAL                                │
│                                                                             │
│   Normalized, queryable, lossless representation in SQLite                  │
│                                                                             │
│   ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │
│   │  sessions   │  │   events    │  │    plans    │  │ checkpoints │       │
│   └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘       │
│                                                                             │
│   Key properties:                                                           │
│   - No information loss from raw (unknown fields stored in raw_data JSON)   │
│   - Source lineage preserved (can trace any record back to raw)             │
│   - Schema is agent-agnostic (same tables for all agents)                   │
│   - Append-friendly (events are immutable once written)                     │
│                                                                             │
│   Purpose: Single source of truth for all queries and analytics             │
└─────────────────────────────────┬───────────────────────────────────────────┘
                                  │
                                  │ Analytics Plugins
                                  │ - Read from canonical tables
                                  │ - Compute metrics
                                  │ - Write to derived tables
                                  ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           LAYER 2: DERIVED                                  │
│                                                                             │
│   Computed/aggregated data, regenerable from Layer 1                        │
│                                                                             │
│   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐            │
│   │ session_metrics │  │ custom_metrics  │  │  assessments    │            │
│   │ (first-order)   │  │ (plugin output) │  │ (LLM-generated) │            │
│   └─────────────────┘  └─────────────────┘  └─────────────────┘            │
│                                                                             │
│   Key properties:                                                           │
│   - Fully regenerable from Layer 1 (can drop and recompute)                 │
│   - Schema defined by analytics plugins                                     │
│   - May be stale (updated async, on triggers)                               │
│                                                                             │
│   Purpose: Pre-computed views optimized for specific analytics/UI needs     │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Ensuring No Information Loss (Layer 0 → Layer 1)

The canonical schema must capture everything from raw logs. Strategy:

```rust
pub struct Event {
    // === Explicitly parsed fields ===
    pub id: i64,
    pub session_id: String,
    pub seq: i32,
    pub ts: DateTime<Utc>,
    pub event_type: EventType,
    pub tokens_in: Option<i32>,
    pub tokens_out: Option<i32>,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_result: Option<String>,
    pub duration_ms: Option<i32>,
    pub content: Option<String>,

    // === Lineage (trace back to raw) ===
    pub source_file: String,         // path to raw file
    pub source_offset: i64,          // byte offset in file
    pub source_line: Option<i32>,    // line number if applicable

    // === Lossless capture ===
    pub raw_data: serde_json::Value, // ENTIRE original record, unparsed

    // === Parsed but agent-specific ===
    pub metadata: serde_json::Value, // agent-specific fields we recognized
}
```

**The `raw_data` field is key:** We store the complete original JSON/record. This means:
- If we later discover we need a field we didn't parse, we can extract it from `raw_data`
- We can reprocess historical data without re-reading source files
- Analytics plugins can access fields we didn't anticipate

### Schema Diagram

```sql
-- LAYER 1: Canonical (lossless)

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
    raw_data         JSON,           -- original session metadata if any
    metadata         JSON            -- parsed agent-specific fields
);

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

CREATE TABLE plans (
    id               TEXT PRIMARY KEY,
    agent            TEXT NOT NULL,
    project_path     TEXT,
    title            TEXT,
    content          TEXT NOT NULL,
    created_at       DATETIME NOT NULL,
    updated_at       DATETIME NOT NULL,
    
    -- Lineage
    source_file      TEXT NOT NULL,
    
    -- Lossless capture  
    raw_data         JSON,           -- original if from structured source
    metadata         JSON
);

-- LAYER 2: Derived (regenerable)

CREATE TABLE session_metrics (
    session_id       TEXT PRIMARY KEY REFERENCES sessions(id),
    metric_version   INTEGER NOT NULL,  -- schema version for recomputation
    computed_at      DATETIME NOT NULL,
    
    -- First-order aggregations
    total_tokens_in  INTEGER,
    total_tokens_out INTEGER,
    total_tool_calls INTEGER,
    tool_call_breakdown JSON,
    error_count      INTEGER,
    duration_ms      INTEGER,
    
    -- Higher-order derived
    tokens_per_minute REAL,
    tool_success_rate REAL,
    edit_churn_ratio  REAL
);

CREATE TABLE assessments (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id       TEXT NOT NULL REFERENCES sessions(id),
    assessor         TEXT NOT NULL,     -- plugin name that generated this
    model            TEXT,              -- LLM model if applicable
    assessed_at      DATETIME NOT NULL,
    
    -- Structured scores
    scores           JSON NOT NULL,     -- {"sycophancy": 0.3, "clarity": 0.8, ...}
    
    -- Raw assessment
    raw_response     TEXT,              -- full LLM response for debugging
    prompt_hash      TEXT               -- hash of prompt for cache invalidation
);

-- Generic plugin output table
CREATE TABLE plugin_metrics (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_name      TEXT NOT NULL,
    entity_type      TEXT NOT NULL,     -- 'session', 'event', 'plan', 'global'
    entity_id        TEXT,              -- session_id, event_id, etc.
    metric_name      TEXT NOT NULL,
    metric_value     JSON NOT NULL,     -- flexible: number, string, object
    computed_at      DATETIME NOT NULL,
    
    UNIQUE(plugin_name, entity_type, entity_id, metric_name)
);
```

---

## Analytics Plugin Architecture

Analytics plugins consume canonical data and produce derived metrics.

### Plugin Trait

```rust
/// An analytics plugin that computes metrics from canonical data
pub trait AnalyticsPlugin: Send + Sync {
    /// Unique identifier for this plugin
    fn name(&self) -> &str;

    /// What entity types this plugin analyzes
    fn entity_types(&self) -> Vec<EntityType>;

    /// When should this plugin run?
    fn triggers(&self) -> Vec<AnalyticsTrigger>;

    /// Compute metrics for a session
    fn analyze_session(
        &self,
        session: &Session,
        events: &[Event],
        ctx: &AnalyticsContext,
    ) -> Result<Vec<MetricOutput>>;

    /// Compute global/aggregate metrics (optional)
    fn analyze_global(
        &self,
        ctx: &AnalyticsContext,
    ) -> Result<Vec<MetricOutput>> {
        Ok(vec![])
    }
}

pub enum AnalyticsTrigger {
    /// Run after N new events in a session
    EventCount(usize),
    
    /// Run after session inactive for duration
    Inactivity(Duration),
    
    /// Run on manual request
    OnDemand,
    
    /// Run on schedule (e.g., daily rollups)
    Scheduled(Schedule),
}

pub struct AnalyticsContext<'a> {
    pub db: &'a Database,
    pub config: &'a Config,
    pub llm: Option<&'a LlmClient>,  // available if LLM configured
}

pub struct MetricOutput {
    pub entity_type: EntityType,
    pub entity_id: Option<String>,
    pub metric_name: String,
    pub value: MetricValue,
}

pub enum MetricValue {
    Integer(i64),
    Float(f64),
    Boolean(bool),
    String(String),
    Json(serde_json::Value),
}
```

### Built-in Plugins

```rust
// First-order metrics (always enabled)
pub struct FirstOrderMetrics;

impl AnalyticsPlugin for FirstOrderMetrics {
    fn name(&self) -> &str { "core.first_order" }
    
    fn triggers(&self) -> Vec<AnalyticsTrigger> {
        vec![
            AnalyticsTrigger::EventCount(10),  // update every 10 events
            AnalyticsTrigger::Inactivity(Duration::from_secs(60)),
        ]
    }
    
    fn analyze_session(&self, session: &Session, events: &[Event], _ctx: &AnalyticsContext) 
        -> Result<Vec<MetricOutput>> 
    {
        let total_tokens_in: i32 = events.iter().filter_map(|e| e.tokens_in).sum();
        let total_tokens_out: i32 = events.iter().filter_map(|e| e.tokens_out).sum();
        let tool_calls = events.iter().filter(|e| e.event_type == EventType::ToolCall).count();
        
        Ok(vec![
            MetricOutput::int(&session.id, "total_tokens_in", total_tokens_in),
            MetricOutput::int(&session.id, "total_tokens_out", total_tokens_out),
            MetricOutput::int(&session.id, "total_tool_calls", tool_calls as i64),
            // ... etc
        ])
    }
}

// Higher-order metrics
pub struct EditChurnAnalyzer;

impl AnalyticsPlugin for EditChurnAnalyzer {
    fn name(&self) -> &str { "core.edit_churn" }
    
    fn analyze_session(&self, session: &Session, events: &[Event], _ctx: &AnalyticsContext) 
        -> Result<Vec<MetricOutput>> 
    {
        // Analyze edit_file tool calls to detect re-edits to same regions
        let edit_events: Vec<_> = events.iter()
            .filter(|e| e.tool_name.as_deref() == Some("edit_file"))
            .collect();
        
        let churn_ratio = compute_edit_churn(&edit_events);
        
        Ok(vec![
            MetricOutput::float(&session.id, "edit_churn_ratio", churn_ratio),
        ])
    }
}

// LLM-assessed metrics
pub struct SycophancyAssessor;

impl AnalyticsPlugin for SycophancyAssessor {
    fn name(&self) -> &str { "llm.sycophancy" }
    
    fn triggers(&self) -> Vec<AnalyticsTrigger> {
        vec![
            AnalyticsTrigger::Inactivity(Duration::from_secs(900)), // 15 min
            AnalyticsTrigger::OnDemand,
        ]
    }
    
    fn analyze_session(&self, session: &Session, events: &[Event], ctx: &AnalyticsContext) 
        -> Result<Vec<MetricOutput>> 
    {
        let Some(llm) = ctx.llm else {
            return Ok(vec![]); // LLM not configured, skip
        };
        
        let transcript = build_transcript(events);
        let prompt = SYCOPHANCY_PROMPT.replace("{transcript}", &transcript);
        
        let response = llm.complete(&prompt)?;
        let scores = parse_assessment_response(&response)?;
        
        Ok(vec![
            MetricOutput::float(&session.id, "sycophancy_score", scores.sycophancy),
            MetricOutput::float(&session.id, "goal_clarity", scores.goal_clarity),
            MetricOutput::float(&session.id, "autonomy_level", scores.autonomy),
        ])
    }
}
```

### Custom Plugin Example

Users can add custom analytics:

```rust
// Example: Track usage of specific tools
pub struct ToolUsageTracker {
    tools_of_interest: Vec<String>,
}

impl AnalyticsPlugin for ToolUsageTracker {
    fn name(&self) -> &str { "custom.tool_usage" }
    
    fn analyze_session(&self, session: &Session, events: &[Event], _ctx: &AnalyticsContext) 
        -> Result<Vec<MetricOutput>> 
    {
        let mut outputs = vec![];
        
        for tool in &self.tools_of_interest {
            let count = events.iter()
                .filter(|e| e.tool_name.as_deref() == Some(tool))
                .count();
            
            outputs.push(MetricOutput::int(
                &session.id, 
                &format!("{}_count", tool), 
                count as i64
            ));
        }
        
        Ok(outputs)
    }
}
```

### Plugin Registration

```rust
pub struct AnalyticsEngine {
    plugins: Vec<Box<dyn AnalyticsPlugin>>,
    db: Database,
    config: Config,
}

impl AnalyticsEngine {
    pub fn new(db: Database, config: Config) -> Self {
        let mut engine = Self { 
            plugins: vec![], 
            db, 
            config 
        };
        
        // Register built-in plugins
        engine.register(Box::new(FirstOrderMetrics));
        engine.register(Box::new(EditChurnAnalyzer));
        engine.register(Box::new(SycophancyAssessor));
        
        // Load custom plugins from config
        for plugin_config in &config.analytics.plugins {
            if let Some(plugin) = load_plugin(plugin_config) {
                engine.register(plugin);
            }
        }
        
        engine
    }
    
    pub fn register(&mut self, plugin: Box<dyn AnalyticsPlugin>) {
        self.plugins.push(plugin);
    }
    
    pub fn run_for_session(&self, session_id: &str) -> Result<()> {
        let session = self.db.get_session(session_id)?;
        let events = self.db.get_session_events(session_id, usize::MAX)?;
        
        let ctx = AnalyticsContext {
            db: &self.db,
            config: &self.config,
            llm: self.config.llm.as_ref().map(|c| &c.client),
        };
        
        for plugin in &self.plugins {
            match plugin.analyze_session(&session, &events, &ctx) {
                Ok(outputs) => {
                    for output in outputs {
                        self.db.write_plugin_metric(plugin.name(), &output)?;
                    }
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "Plugin failed");
                }
            }
        }
        
        Ok(())
    }
}
```

### Plugin Isolation & Error Handling

Plugins run in isolation—a misbehaving plugin must not crash the core or block other plugins.

```rust
impl AnalyticsEngine {
    pub fn run_plugin_safely(
        &self,
        plugin: &dyn AnalyticsPlugin,
        session: &Session,
        events: &[Event],
        ctx: &AnalyticsContext,
    ) -> PluginResult {
        let plugin_name = plugin.name().to_string();
        let start = Instant::now();

        // Catch panics
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            plugin.analyze_session(session, events, ctx)
        }));

        let duration = start.elapsed();

        match result {
            Ok(Ok(outputs)) => {
                self.record_plugin_success(&plugin_name, duration, outputs.len());
                PluginResult::Success(outputs)
            }
            Ok(Err(e)) => {
                self.record_plugin_error(&plugin_name, duration, &e);
                tracing::warn!(
                    plugin = %plugin_name,
                    error = %e,
                    duration_ms = %duration.as_millis(),
                    "Plugin returned error"
                );
                PluginResult::Error(e)
            }
            Err(panic_info) => {
                let msg = panic_message(&panic_info);
                self.record_plugin_panic(&plugin_name, duration, &msg);
                tracing::error!(
                    plugin = %plugin_name,
                    panic = %msg,
                    duration_ms = %duration.as_millis(),
                    "Plugin panicked"
                );
                PluginResult::Panic(msg)
            }
        }
    }
}

pub enum PluginResult {
    Success(Vec<MetricOutput>),
    Error(AiobscuraError),
    Panic(String),
}
```

### Plugin Observability

Track plugin health for debugging:

```sql
CREATE TABLE plugin_runs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_name   TEXT NOT NULL,
    session_id    TEXT,
    started_at    DATETIME NOT NULL,
    duration_ms   INTEGER NOT NULL,
    status        TEXT NOT NULL,      -- 'success', 'error', 'panic', 'timeout'
    error_message TEXT,
    metrics_produced INTEGER,
    
    -- For debugging slow/failing plugins
    input_event_count INTEGER,
    input_token_count INTEGER
);

CREATE INDEX idx_plugin_runs_plugin ON plugin_runs(plugin_name, started_at);
CREATE INDEX idx_plugin_runs_status ON plugin_runs(status) WHERE status != 'success';
```

**Observability features:**

1. **Run history:** Every plugin invocation logged with timing and status
2. **Error aggregation:** Query recent failures by plugin
3. **Performance tracking:** Identify slow plugins
4. **Debug context:** Input size captured for reproducing issues

**CLI commands for debugging:**

```bash
# Show plugin health summary
$ aiobscura plugins status
PLUGIN              SUCCESS   ERROR   PANIC   AVG_MS
core.first_order    1,247     0       0       12
core.edit_churn     1,245     2       0       45
llm.assessment      89        3       0       2,340

# Show recent errors for a plugin
$ aiobscura plugins errors llm.assessment
2024-12-06 10:23:45  session=a3f2c1  error="LLM timeout after 30s"
2024-12-06 09:15:12  session=b7e9d0  error="Failed to parse response"
...

# Show slow runs
$ aiobscura plugins slow --threshold=5000ms
PLUGIN           SESSION   DURATION   EVENTS   TOKENS
llm.assessment   c4d5e6    8,234ms    342      45.2k
llm.assessment   a3f2c1    6,891ms    289      38.1k
```

### Plugin Timeouts

Plugins have configurable timeouts:

```toml
[analytics]
# Default timeout for all plugins
timeout_ms = 30000

# Per-plugin overrides
[analytics.plugin_timeouts]
"llm.assessment" = 60000    # LLM calls can be slow
"core.first_order" = 5000   # Should be fast
```

```rust
impl AnalyticsEngine {
    fn get_timeout(&self, plugin_name: &str) -> Duration {
        self.config.analytics.plugin_timeouts
            .get(plugin_name)
            .copied()
            .unwrap_or(self.config.analytics.timeout_ms)
            .into()
    }

    pub async fn run_plugin_with_timeout(
        &self,
        plugin: &dyn AnalyticsPlugin,
        session: &Session,
        events: &[Event],
        ctx: &AnalyticsContext,
    ) -> PluginResult {
        let timeout = self.get_timeout(plugin.name());
        
        match tokio::time::timeout(
            timeout,
            self.run_plugin_safely(plugin, session, events, ctx)
        ).await {
            Ok(result) => result,
            Err(_) => {
                self.record_plugin_timeout(plugin.name(), timeout);
                tracing::warn!(
                    plugin = %plugin.name(),
                    timeout_ms = %timeout.as_millis(),
                    "Plugin timed out"
                );
                PluginResult::Timeout
            }
        }
    }
}
```

### Plugin Data Access

Plugins can access canonical data and `raw_data` for fields we didn't explicitly parse:

```rust
fn analyze_session(&self, session: &Session, events: &[Event], ctx: &AnalyticsContext) 
    -> Result<Vec<MetricOutput>> 
{
    // Access explicitly parsed fields
    let tool_calls = events.iter()
        .filter(|e| e.event_type == EventType::ToolCall)
        .count();
    
    // Access agent-specific fields from raw_data
    // (e.g., Claude Code might have fields Codex doesn't)
    for event in events {
        if let Some(model) = event.raw_data.get("model").and_then(|v| v.as_str()) {
            // Track which model was used (not in common schema)
        }
    }
    
    // Query other canonical tables if needed
    let related_plans = ctx.db.query_plans_for_project(&session.project_path)?;
    
    Ok(vec![...])
}
```

---

## Updated Crate Structure

```
aiobscura/
├── Cargo.toml
├── aiobscura-core/
│   └── src/
│       ├── ingest/
│       │   ├── mod.rs            # IngestCoordinator
│       │   └── parsers/          # Layer 0 -> Layer 1 parsers
│       │
│       ├── db/
│       │   ├── schema.rs         # migrations and table definitions
│       │   └── repo.rs           # query/insert operations
│       │
│       └── analytics/
│           ├── mod.rs            # public exports + wrapped helpers
│           ├── engine.rs         # AnalyticsEngine + AnalyticsPlugin trait
│           ├── dashboard.rs      # dashboard aggregates
│           ├── project.rs        # project-level analytics
│           ├── wrapped.rs        # year/month wrapped stats
│           ├── personality.rs    # wrapped personality model
│           ├── metrics_registry.rs
│           │
│           └── plugins/          # Built-in plugins
│               ├── first_order/
│               └── edit_churn/
├── aiobscura/
│   └── src/
│       ├── main.rs
│       ├── app.rs
│       ├── ui.rs
│       ├── sync.rs
│       ├── analyze.rs
│       └── collector.rs
└── aiobscura-wrapped/
    └── src/main.rs
```

### Startup Sequence

```
┌─────────────────────────────────────────────────────────────────┐
│                          main()                                 │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  1. Load config from ~/.config/aiobscura/config.toml            │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  2. Resolve DB path and acquire process lock(s) for that DB      │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  3. Open/create SQLite DB and run migrations                     │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  4. Decide runtime mode from locks:                              │
│     - ingest owner (can parse+insert)                            │
│     - read-only (sync lock held by aiobscura-sync)              │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  5. Initialize TUI and enter polling event loop                  │
│     (refresh DB views; ingest only when owner)                  │
└─────────────────────────────────────────────────────────────────┘
```

### Live Ingestion Flow

```
┌─────────────────────────────┐
│ Is sync lock currently held │
│ by another process?         │
└──────────────┬──────────────┘
               │
         yes   │   no
               │
               ▼
┌─────────────────────────────┐      ┌─────────────────────────────┐
│ aiobscura read-only mode    │      │ aiobscura ingest-owner mode │
│ - no parser execution        │      │ - periodic sync_all()       │
│ - DB polling refresh only    │      │ - parser + DB writes        │
└──────────────┬──────────────┘      └──────────────┬──────────────┘
               │                                     │
               └─────────────────┬───────────────────┘
                                 ▼
                       ┌──────────────────┐
                       │ SQLite canonical │
                       │ + derived tables │
                       └────────┬─────────┘
                                ▼
                        ┌──────────────┐
                        │ TUI refresh  │
                        │ live/project │
                        └──────────────┘
```

### Assessment Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                    Trigger Condition Met                        │
│     (inactivity timeout OR tool_call threshold OR manual)       │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  1. Load session events from DB                                 │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  2. Build assessment prompt with transcript                     │
│     (truncate if exceeds context window)                        │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  3. Call LLM (ollama / claude / openai)                         │
│     Request structured JSON response                            │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  4. Parse response, extract scores                              │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  5. Write to session_metrics table                              │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  6. Emit CoreEvent::AssessmentComplete                          │
└─────────────────────────────────────────────────────────────────┘
```

---

## Parser Plugin Architecture

Each agent parser implements a common trait that returns **all entity types** the agent produces:

```rust
pub trait AgentParser: Send + Sync {
    /// Unique identifier for this agent
    fn agent_type(&self) -> AgentType;

    /// Root directory to watch (e.g., ~/.claude)
    fn root_path(&self) -> PathBuf;

    /// Check if this agent is installed
    fn is_installed(&self) -> bool;

    /// Describe what directories/files this parser handles
    fn source_patterns(&self) -> Vec<SourcePattern>;

    /// Parse all entities from a source file
    /// Returns sessions, events, plans, and any other artifacts
    fn parse(&self, source: &SourceFile) -> Result<ParseResult>;
}

/// Describes a type of source this parser handles
pub struct SourcePattern {
    pub entity_type: EntityType,
    pub path_pattern: String,      // glob pattern relative to root
    pub file_format: FileFormat,
}

pub enum EntityType {
    Session,
    Plan,
    // Future: Config, Template, etc.
}

pub enum FileFormat {
    Jsonl,           // Append-only, track byte offset
    Json,            // Rewritten, track hash
    Markdown,        // May need full reparse
    Sqlite,          // Track rowid/timestamp
}

/// A discovered source file to parse
pub struct SourceFile {
    pub path: PathBuf,
    pub entity_type: EntityType,
    pub format: FileFormat,
    pub checkpoint: Option<Checkpoint>,  // previous parse state
}

/// Result of parsing a source
pub struct ParseResult {
    pub sessions: Vec<SessionUpdate>,
    pub events: Vec<Event>,
    pub plans: Vec<Plan>,
    pub new_checkpoint: Checkpoint,
}

pub struct SessionUpdate {
    pub id: String,
    pub session_type: SessionType,
    pub project_path: Option<PathBuf>,
    pub started_at: Option<DateTime<Utc>>,
    pub last_activity_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

pub struct Checkpoint {
    pub byte_offset: Option<u64>,
    pub file_hash: Option<String>,
    pub last_rowid: Option<i64>,
    pub last_timestamp: Option<DateTime<Utc>>,
}
```

### Example: Claude Code Parser

```rust
impl AgentParser for ClaudeCodeParser {
    fn agent_type(&self) -> AgentType {
        AgentType::ClaudeCode
    }

    fn root_path(&self) -> PathBuf {
        dirs::home_dir().unwrap().join(".claude")
    }

    fn source_patterns(&self) -> Vec<SourcePattern> {
        vec![
            // Conversation logs
            SourcePattern {
                entity_type: EntityType::Session,
                path_pattern: "projects/*/conversations/*.jsonl".into(),
                file_format: FileFormat::Jsonl,
            },
            // Plan files
            SourcePattern {
                entity_type: EntityType::Plan,
                path_pattern: "plans/*.md".into(),
                file_format: FileFormat::Markdown,
            },
        ]
    }

    fn parse(&self, source: &SourceFile) -> Result<ParseResult> {
        match source.entity_type {
            EntityType::Session => self.parse_conversation(source),
            EntityType::Plan => self.parse_plan(source),
        }
    }
}
```

### Adding a New Agent

1. Implement `AgentParser` trait
2. Define `source_patterns()` for all entity types the agent produces
3. Implement parsing logic for each entity type
4. Register in `ingest/parsers/mod.rs`
5. Add to `AgentType` enum

---

## Concurrency Model

Current v1 uses **process-level coordination** scoped to a database path.

```
┌──────────────────────────────┐
│ aiobscura (TUI)              │
│ acquires ui lock             │
└──────────────┬───────────────┘
               │
               ▼
┌─────────────────────────────────────────────┐
│ Try sync lock                               │
│ - success: TUI can ingest                   │
│ - busy: TUI runs read-only                  │
└─────────────────────────────────────────────┘

┌──────────────────────────────┐
│ aiobscura-sync               │
│ probes ui lock, then sync    │
│ exits if ui lock is held     │
└──────────────────────────────┘
```

### Key Concurrency Points

1. **Mutual exclusion at process level:** lock files prevent concurrent ingest writers for one DB.
2. **Role split:** `aiobscura-sync` is dedicated ingest owner; `aiobscura` can ingest only when sync lock is free.
3. **Read-only fallback:** TUI remains usable when sync is active, but parsing/inserts are disabled.
4. **SQLite concurrency model:** WAL allows concurrent read + write with one active writer process.

---

## File Watching Strategy

Current implementation uses polling-driven ingestion:
- `aiobscura-sync --watch`: periodic `sync_all()` loop.
- `aiobscura` TUI: periodic `sync_all()` only when it owns sync lock; otherwise DB refresh only.

No OS file watcher is required for correctness in v1.

---

## Database Schema Management

Use embedded migrations:

```rust
const MIGRATIONS: &[&str] = &[
    // v1
    r#"
    -- LAYER 1: Canonical (lossless)
    
    CREATE TABLE sessions (
        id               TEXT PRIMARY KEY,
        agent            TEXT NOT NULL,
        session_type     TEXT NOT NULL,
        project_path     TEXT,
        started_at       DATETIME NOT NULL,
        last_activity_at DATETIME,
        status           TEXT,
        source_file      TEXT NOT NULL,
        raw_data         JSON,            -- original session metadata
        metadata         JSON             -- parsed agent-specific fields
    );

    CREATE TABLE events (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id       TEXT NOT NULL REFERENCES sessions(id),
        seq              INTEGER NOT NULL,
        ts               DATETIME NOT NULL,
        event_type       TEXT NOT NULL,
        tokens_in        INTEGER,
        tokens_out       INTEGER,
        tool_name        TEXT,
        tool_input       JSON,
        tool_result      TEXT,
        duration_ms      INTEGER,
        content          TEXT,
        source_file      TEXT NOT NULL,
        source_offset    INTEGER NOT NULL,
        source_line      INTEGER,
        raw_data         JSON NOT NULL,   -- complete original record
        metadata         JSON             -- parsed agent-specific fields
    );

    CREATE TABLE plans (
        id               TEXT PRIMARY KEY,
        agent            TEXT NOT NULL,
        project_path     TEXT,
        title            TEXT,
        content          TEXT NOT NULL,
        created_at       DATETIME NOT NULL,
        updated_at       DATETIME NOT NULL,
        source_file      TEXT NOT NULL,
        raw_data         JSON,
        metadata         JSON
    );

    CREATE TABLE checkpoints (
        source_path      TEXT PRIMARY KEY,
        agent            TEXT NOT NULL,
        entity_type      TEXT NOT NULL,
        file_hash        TEXT,
        byte_offset      INTEGER,
        last_rowid       INTEGER,
        last_event_ts    DATETIME,
        updated_at       DATETIME
    );

    -- LAYER 2: Derived (regenerable)
    
    CREATE TABLE session_metrics (
        session_id          TEXT PRIMARY KEY REFERENCES sessions(id),
        metric_version      INTEGER NOT NULL,
        computed_at         DATETIME NOT NULL,
        total_tokens_in     INTEGER,
        total_tokens_out    INTEGER,
        total_tool_calls    INTEGER,
        tool_call_breakdown JSON,
        error_count         INTEGER,
        duration_ms         INTEGER,
        tokens_per_minute   REAL,
        tool_success_rate   REAL,
        edit_churn_ratio    REAL
    );

    CREATE TABLE assessments (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id       TEXT NOT NULL REFERENCES sessions(id),
        assessor         TEXT NOT NULL,
        model            TEXT,
        assessed_at      DATETIME NOT NULL,
        scores           JSON NOT NULL,
        raw_response     TEXT,
        prompt_hash      TEXT
    );

    CREATE TABLE plugin_metrics (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        plugin_name      TEXT NOT NULL,
        entity_type      TEXT NOT NULL,
        entity_id        TEXT,
        metric_name      TEXT NOT NULL,
        metric_value     JSON NOT NULL,
        computed_at      DATETIME NOT NULL,
        UNIQUE(plugin_name, entity_type, entity_id, metric_name)
    );

    CREATE TABLE plugin_runs (
        id                INTEGER PRIMARY KEY AUTOINCREMENT,
        plugin_name       TEXT NOT NULL,
        session_id        TEXT,
        started_at        DATETIME NOT NULL,
        duration_ms       INTEGER NOT NULL,
        status            TEXT NOT NULL,
        error_message     TEXT,
        metrics_produced  INTEGER,
        input_event_count INTEGER,
        input_token_count INTEGER
    );

    -- Indexes
    CREATE INDEX idx_events_session ON events(session_id);
    CREATE INDEX idx_events_ts ON events(ts);
    CREATE INDEX idx_sessions_agent ON sessions(agent);
    CREATE INDEX idx_plans_agent ON plans(agent);
    CREATE INDEX idx_assessments_session ON assessments(session_id);
    CREATE INDEX idx_plugin_runs_plugin ON plugin_runs(plugin_name, started_at);
    CREATE INDEX idx_plugin_runs_status ON plugin_runs(status) WHERE status != 'success';
    "#,
];

fn run_migrations(conn: &Connection) -> Result<()> {
    let current_version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);

    for (i, migration) in MIGRATIONS.iter().enumerate() {
        if i as i32 >= current_version {
            conn.execute_batch(migration)?;
            conn.execute(&format!("PRAGMA user_version = {}", i + 1), [])?;
        }
    }
    Ok(())
}
```

---

## Error Handling Strategy

```rust
#[derive(Debug, thiserror::Error)]
pub enum AiobscuraError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error in {agent} log: {message}")]
    Parse { agent: String, message: String },

    #[error("LLM error: {0}")]
    Llm(String),

    #[error("Config error: {0}")]
    Config(String),
}
```

**Philosophy:**
- Parsing errors for individual events → log warning, skip event, continue
- File-level errors → log error, skip file, continue with others
- DB errors → propagate up, may be fatal
- LLM errors → log warning, skip assessment, continue

---

## Configuration Schema

```rust
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub llm: Option<LlmConfig>,

    #[serde(default)]
    pub assessment: AssessmentConfig,

    #[serde(default)]
    pub agents: AgentOverrides,
}

#[derive(Debug, Deserialize)]
pub struct LlmConfig {
    pub provider: LlmProvider,  // "ollama", "claude", "openai"
    pub model: String,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,  // for claude/openai; can also use env var
}

#[derive(Debug, Deserialize, Default)]
pub struct AssessmentConfig {
    #[serde(default = "default_inactivity")]
    pub inactivity_minutes: u32,  // default: 15

    #[serde(default = "default_threshold")]
    pub tool_call_threshold: u32,  // default: 20
}

#[derive(Debug, Deserialize, Default)]
pub struct AgentOverrides {
    pub claude_code_path: Option<PathBuf>,
    pub codex_path: Option<PathBuf>,
    // ... etc
}
```

---

## TUI State Management

```rust
pub struct App {
    pub core: AiobscuraCore,
    pub current_view: ViewType,
    pub views: Views,
    pub should_quit: bool,
}

pub struct Views {
    pub live: LiveView,
    pub history: HistoryView,
    pub detail: DetailView,
    pub analytics: AnalyticsView,
}

pub enum ViewType {
    Live,
    History,
    Detail,
    Analytics,
}

impl App {
    pub async fn run(&mut self, terminal: &mut Terminal<...>) -> Result<()> {
        let mut core_events = self.core.subscribe();

        loop {
            // Render
            terminal.draw(|f| self.render(f))?;

            // Handle input with timeout
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
            }

            // Process core events
            while let Ok(event) = core_events.try_recv() {
                self.handle_core_event(event);
            }

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }
}
```

---

## Dependencies

### aiobscura-core

```toml
[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "sync", "time"] }
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
notify = "6"
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1"
tracing = "0.1"
reqwest = { version = "0.11", features = ["json"] }  # for LLM calls
sha2 = "0.10"  # for file hashing
glob = "0.3"   # for source pattern matching
dirs = "5"     # for home directory
```

### aiobscura

```toml
[dependencies]
aiobscura-core = { path = "../aiobscura-core" }
ratatui = "0.29"
crossterm = "0.28"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
indicatif = "0.17"
notify = "6"
notify-debouncer-mini = "0.4"
```

### aiobscura-wrapped

```toml
[dependencies]
aiobscura-core = { path = "../aiobscura-core" }
anyhow = "1"
chrono = "0.4"
clap = { version = "4", features = ["derive"] }
serde_json = "1"
```

---

## Testing Strategy

### Unit Tests
- Parser logic with fixture files (sample logs from each agent)
- Checkpoint tracking logic
- Metric calculations
- Trigger condition evaluation

### Integration Tests
- End-to-end: write log file → verify events in DB
- Assessment flow with mock LLM

### Test Fixtures
```
tests/fixtures/
├── claude-code/
│   └── sample-conversation.jsonl
├── codex/
│   └── sample-session.json
└── expected/
    └── parsed-events.json
```

---

## Future Considerations (v2)

1. **Daemon mode:** Wrap `AiobscuraCore` in a daemon with Unix socket API
2. **Semantic search:** Add sqlite-vec or separate vector store
3. **Cost estimation:** Add pricing config, track model per event
4. **macOS GUI:** Swift/SwiftUI app linking to `aiobscura-core` via C FFI or separate process with IPC
5. **Time-series metrics:** For high-frequency plugin metrics, add proper time-series storage (possibly separate from SQLite)
6. **Scripting plugins:** Lua or WASM-based plugins for users who don't want to compile Rust (evaluate if there's demand)

---

## Open Design Questions

1. **Sync vs Async DB:** Using sync rusqlite for simplicity. If DB becomes bottleneck, consider sqlx async. Decision: start sync, measure.

2. **Watcher granularity:** Watch entire agent directories recursively vs individual session files. Decision: recursive for simplicity, debounce heavily.

3. **Assessment batching:** Run one LLM call per session or batch multiple? Decision: one per session for simpler prompt engineering, but respect rate limits.

4. **Plugin run storage retention:** How long to keep `plugin_runs` history? Decision: configurable, default 30 days, auto-prune on startup.
