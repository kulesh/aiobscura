# Abstraction Layer Analysis: aiobscura

*Analysis Date: 2025-12-17*

## 1. Current Abstraction Layers

The architecture implements a **5-layer data flow** from raw logs to TUI display:

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ LAYER 0: RAW FILES                                                           │
│ Source: ~/.claude/projects/*/*.jsonl, ~/.codex/sessions/*.json               │
│ Ownership: External (AI assistants write these)                              │
│ Key Abstraction: Immutable read-only source of truth                         │
└───────────────────────────────────┬──────────────────────────────────────────┘
                                    │
                                    │ AssistantParser trait (claude.rs, codex.rs)
                                    │ ParseResult → (Session, Thread, Message, Plan)
                                    │
┌───────────────────────────────────▼──────────────────────────────────────────┐
│ LAYER 1: CANONICAL (types.rs)                                                │
│ Domain Types: Project, Session, Thread, Message, Plan, BackingModel          │
│ Key Invariants:                                                              │
│   - Lossless: raw_data field preserves complete original record              │
│   - Lineage: source_file_path, source_offset trace back to Layer 0           │
│   - Agent-agnostic: Same types for all assistants                            │
└───────────────────────────────────┬──────────────────────────────────────────┘
                                    │
                                    │ Database (repo.rs) upsert_*/insert_* methods
                                    │ row_to_* conversion functions
                                    │
┌───────────────────────────────────▼──────────────────────────────────────────┐
│ LAYER 1.5: DATABASE (schema.rs, repo.rs)                                     │
│ SQLite Tables: projects, sessions, threads, messages, plans, source_files    │
│ Key Role: Persistence + Query (single source for all views)                  │
└───────────────────────────────────┬──────────────────────────────────────────┘
                                    │
              ┌─────────────────────┴─────────────────────┐
              │                                           │
              ▼                                           ▼
┌─────────────────────────────┐         ┌─────────────────────────────────────┐
│ LAYER 2: DERIVED METRICS    │         │ LAYER 2: VIEW MODELS                │
│ Tables: session_metrics,    │         │ Types: ProjectRow, ProjectStats,    │
│ plugin_metrics, assessments │         │ SessionSummary, DashboardStats,     │
│ Engine: analytics/engine.rs │         │ ThreadRow, SessionRow               │
│ Key Role: Computed metrics  │         │ Key Role: Pre-aggregated for UI     │
└─────────────────────────────┘         └─────────────────────────────────────┘
                                                          │
                                    ┌─────────────────────┘
                                    │
┌───────────────────────────────────▼──────────────────────────────────────────┐
│ LAYER 3: TUI APPLICATION (app.rs, ui.rs)                                     │
│ State: App struct with view_mode, data vectors, table selection states       │
│ Key Role: View rendering + User interaction                                  │
│ Consumes: Domain types (Message, Plan) + View Models (ProjectRow, ThreadRow) │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Identified Abstraction Leaks

### ~~Leak 1: TUI Creates Its Own View Models (ThreadRow, SessionRow)~~ — NOT A LEAK

**Location:** `aiobscura/src/thread_row.rs`

**Original concern:** ThreadRow/SessionRow are defined in TUI, not core.

**Why this is actually correct:**

ThreadRow contains UI-specific fields that other UIs wouldn't use:
- `indent_level: usize` — for TUI tree rendering
- `is_last_child: bool` — for tree connector characters (├── vs └──)

A web UI would compute tree structure client-side with JavaScript. A mobile app might not show threads as a tree at all.

**The litmus test:** "Would a different UI use this exact shape?"
- `ProjectRow` (in core) → Yes, any UI showing projects wants these fields
- `ThreadRow` with `indent_level` → No, that's TUI tree-specific

**Conclusion:** ThreadRow/SessionRow correctly live in the TUI layer. The real issue is the N+1 query pattern used to populate them (see Leak 2).

---

### Leak 2: App.load_threads() Does Complex Query Logic (THE REAL ISSUE)

**Location:** `aiobscura/src/app.rs:386-581`

**Problem:** The `load_threads()` function performs:
1. Query all sessions
2. For each session, get project name (another query)
3. Get all threads for session
4. Build parent-child hierarchy
5. Sort by project, then by last_activity
6. Count messages per thread (N more queries)

This is **data access logic** that leaks into the presentation layer.

**Better Abstraction:** Core should provide efficient query methods that return **domain types with aggregated data**, not view models:

```rust
// In repo.rs - returns domain types, not UI types
pub fn list_threads_with_counts(&self, session_id: &str)
    -> Result<Vec<(Thread, i64)>>  // Thread + message_count

pub fn get_project_name(&self, project_id: &str) -> Result<Option<String>>
```

The TUI then assembles `ThreadRow` from these efficient queries, adding only UI-specific fields (`indent_level`, `is_last_child`).

---

### Leak 3: Project Sub-Tab Data Loading in TUI

**Location:** `aiobscura/src/app.rs:1823-1902`

**Problem:** Methods like `load_project_sessions()`, `load_project_plans()`, `load_project_files()` contain query logic that belongs in the data layer.

**Evidence:**
```rust
fn load_project_plans(&mut self, project_id: &str) -> Result<()> {
    let sessions = self.db.list_sessions(&SessionFilter::default())?;
    for session in sessions {
        if session.project_id.as_ref() != Some(&project_id.to_string()) {
            continue; // Filtering in Rust instead of SQL!
        }
        // ...
    }
}
```

This should be `Database::list_project_plans(project_id)`.

---

### Leak 4: ThreadMetadata Construction in TUI

**Location:** `aiobscura/src/app.rs:691-755`

**Problem:** `load_thread_metadata()` makes **9 separate database calls** to assemble metadata:
- `get_session_source_path()`
- `get_session_model_name()`
- `get_session_metadata()`
- `get_session_timestamps()`
- `count_thread_messages()`
- `count_session_agents()`
- `get_thread_tool_stats()`
- `count_session_plans()`
- `get_thread_file_stats()`

**Better Abstraction:** A single `Database::get_thread_detail(thread_id)` returning a `ThreadDetail` struct.

---

### Leak 5: EnvironmentHealth Constructed in TUI

**Location:** `aiobscura/src/app.rs:23-45, 302-329`

**Problem:** `EnvironmentHealth` and `AssistantHealth` are defined in `app.rs` (TUI layer) but represent data that should come from the core library.

**Evidence:** `load_environment_health()` calls:
- `db.get_database_size()`
- `db.get_assistant_source_stats()`
- `db.get_total_counts()`

These three pieces should be aggregated by a `Database::get_environment_health()` method.

---

### Leak 6: Session/Thread Analytics Loading Pattern

**Location:** `aiobscura/src/app.rs:758-791`

**Problem:** Analytics are loaded via:
```rust
let engine = create_default_engine();
engine.ensure_session_analytics(session_id, &self.db)
```

This couples the TUI to the analytics engine internals. The core API (`lib.rs`) should expose a higher-level method.

---

### Leak 7: Relative Time Formatting Duplicated

**Location:** `aiobscura/src/thread_row.rs:43-65` and `96-118`

**Problem:** `ThreadRow::relative_time()` and `SessionRow::relative_time()` are identical methods. This should be a utility function in `aiobscura-core`.

---

## 3. Types.rs Reconciliation with Requirements

### Terminology Alignment ✅

| Requirement Term | types.rs | Status |
|------------------|----------|--------|
| Project | `Project` | ✅ Aligned |
| Assistant | `Assistant` enum (was `AgentType`) | ✅ Aligned (with deprecation alias) |
| BackingModel | `BackingModel` | ✅ Aligned |
| Session | `Session` | ✅ Aligned |
| Thread | `Thread` | ✅ Aligned |
| Message | `Message` (was `Event`) | ✅ Aligned (with deprecation alias) |
| Plan | `Plan` | ✅ Aligned |

### Key Types vs Requirements

#### Session (types.rs:332-362)

| Field | Requirement | Implementation | Status |
|-------|-------------|----------------|--------|
| id | ✅ | `String` | ✅ |
| agent/assistant | ✅ | `Assistant` | ✅ |
| session_type | ✅ (AgentTask, Conversation, FileOperation) | **Missing!** | ❌ **LEAK** |
| project_path | ✅ | `project_id: Option<String>` (FK) | ✅ Better design |
| started_at | ✅ | `DateTime<Utc>` | ✅ |
| last_activity_at | ✅ | `Option<DateTime<Utc>>` | ✅ |
| status | ✅ | `SessionStatus` | ✅ |
| source_file | ✅ | `source_file_path: String` | ✅ |
| raw_data | ✅ | **Removed** (derived from messages) | ✅ Correct decision |
| metadata | ✅ | `serde_json::Value` | ✅ |
| backing_model_id | In architecture doc | `Option<String>` | ✅ |

**Finding:** `SessionType` enum is defined in requirements but **not implemented** in types.rs.

#### Message (types.rs:687-746)

| Field | Requirement | Implementation | Status |
|-------|-------------|----------------|--------|
| id | ✅ | `i64` | ✅ |
| session_id | ✅ | `String` | ✅ |
| thread_id | New (architecture) | `String` | ✅ |
| seq | ✅ | `i32` | ✅ |
| ts/emitted_at | ✅ | `emitted_at: DateTime<Utc>` | ✅ |
| observed_at | New | `DateTime<Utc>` | ✅ Enhanced |
| event_type/message_type | ✅ | `MessageType` | ✅ |
| author_role/name | New | `AuthorRole`, `author_name` | ✅ Enhanced |
| tokens_in/out | ✅ | `Option<i32>` | ✅ |
| tool_name | ✅ | `Option<String>` | ✅ |
| tool_input | ✅ | `Option<serde_json::Value>` | ✅ |
| tool_result | ✅ | `Option<String>` | ✅ |
| duration_ms | ✅ | `Option<i32>` | ✅ |
| content | ✅ | `Option<String>` | ✅ |
| content_type | New | `Option<ContentType>` | ✅ Enhanced |
| source_file | ✅ | `source_file_path: String` | ✅ |
| source_offset | ✅ | `i64` | ✅ |
| source_line | ✅ | `Option<i32>` | ✅ |
| raw_data | ✅ | `serde_json::Value` | ✅ |
| metadata | ✅ | `serde_json::Value` | ✅ |

**Status:** Message is well-aligned and enhanced beyond requirements.

#### Plan (types.rs:884-912)

| Field | Requirement | Implementation | Status |
|-------|-------------|----------------|--------|
| id | ✅ | `String` | ✅ |
| agent | ✅ | **Missing** (uses session_id FK) | ⚠️ Different design |
| project_path | ✅ | **Missing** (via session FK) | ⚠️ Different design |
| session_id | New | `String` | ✅ |
| title | ✅ | `Option<String>` | ✅ |
| content | ✅ | `Option<String>` | ✅ |
| created_at | ✅ | `DateTime<Utc>` | ✅ |
| updated_at | ✅ | `modified_at: DateTime<Utc>` | ✅ (renamed) |
| status | New | `PlanStatus` | ✅ Enhanced |
| path | New | `PathBuf` | ✅ |
| source_file | ✅ | `source_file_path: String` | ✅ |
| raw_data | ✅ | `serde_json::Value` | ✅ |
| metadata | ✅ | `serde_json::Value` | ✅ |

**Finding:** Plan uses `session_id` FK instead of direct `agent`/`project_path`. This is a **better** design because it:
1. Avoids data duplication
2. Maintains referential integrity
3. Allows plan-session-project traversal

---

## 4. Summary of Issues

### Critical (Architectural)
1. **SessionType enum missing** - Requirements define AgentTask/Conversation/FileOperation but types.rs lacks this

### Major (Abstraction Leaks)
2. **N+1 query patterns** - TUI makes many DB calls instead of single efficient queries
3. **Data access logic in TUI** - load_threads(), load_project_plans() filter/aggregate in app.rs instead of using efficient core queries
4. **EnvironmentHealth/AssistantHealth in wrong layer** - Should be in core (no UI-specific fields)

### Minor (Code Quality)
5. **Duplicated relative_time()** - Same logic in ThreadRow and SessionRow
6. **Analytics coupling** - TUI directly uses create_default_engine() instead of core API

### Correctly Placed (Not Issues)
- **ThreadRow/SessionRow in TUI** - These have UI-specific fields (`indent_level`, `is_last_child`) that other UIs wouldn't use. Different UIs will shape data differently for their rendering needs.

---

## 5. Proposed Fixes

### Fix 1: Add SessionType Enum

**Change:** Add to `types.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    /// Full agent coding session (human + AI + tools)
    AgentTask,
    /// Pure human-AI conversation (no tool use)
    Conversation,
    /// Batch file operations
    FileOperation,
    /// Unknown or undetermined
    Unknown,
}
```

And add `session_type: SessionType` to `Session` struct.

**Trade-off:** This requires parsers to infer session type from content (presence/absence of tool calls). May need to be computed lazily.

---

### ~~Fix 2: Move View Models to Core~~ — REJECTED

**Original proposal:** Move `ThreadRow`, `SessionRow` to core.

**Why rejected:** These types have UI-specific fields (`indent_level`, `is_last_child`) that other UIs wouldn't use. View models should stay in their respective UI layers.

**What to do instead:** Core provides efficient data access; each UI shapes data for its needs.

---

### Fix 2: Add Efficient Query Methods to Database (Revised)

**Add to `repo.rs`:**
```rust
/// List threads with message counts in a single query (avoids N+1).
/// Returns domain types, not view models.
pub fn list_threads_with_counts(&self, session_id: &str)
    -> Result<Vec<(Thread, i64)>> {
    // Single JOIN query: threads LEFT JOIN (SELECT COUNT(*) FROM messages)
}

/// Get all plans for a project directly via session FK.
pub fn list_project_plans(&self, project_id: &str) -> Result<Vec<Plan>> {
    // Direct SQL with WHERE sessions.project_id = ?
}

/// Get thread metadata in one query (source_path, model, cwd, etc).
pub fn get_thread_context(&self, thread_id: &str) -> Result<Option<ThreadContext>> {
    // Single query returning aggregated context
}

/// Get environment health stats in one call.
pub fn get_environment_health(&self) -> Result<EnvironmentHealth> {
    // Combines db size, assistant stats, total counts
}
```

**Key principle:** These methods return **domain types** or **simple aggregation structs**, not UI-shaped view models. The TUI then assembles its `ThreadRow` from these efficient building blocks.

**Trade-off:**
- **Pro:** Eliminates N+1 queries, cleaner separation
- **Con:** More methods in repo.rs
- **Mitigation:** Group these as "aggregate" or "summary" queries, distinct from basic CRUD

---

### Fix 3: Move EnvironmentHealth to Core

**Change:** Move `EnvironmentHealth` and `AssistantHealth` from `app.rs` to `types.rs`.

**Trade-off:** Minimal - these are clearly data types, not UI types.

---

### Fix 4: Add Utility Module for Common Formatting

**Add `aiobscura-core/src/format.rs`:**
```rust
/// Format a duration as relative time (e.g., "2m ago", "1h ago").
pub fn relative_time(ts: Option<DateTime<Utc>>) -> String { ... }

/// Format duration in seconds as human-readable (e.g., "5m", "1h 30m").
pub fn format_duration(secs: i64) -> String { ... }
```

**Trade-off:** Adds a new module, but eliminates code duplication.

---

### Fix 5: Create a Higher-Level API Module

**Add `aiobscura-core/src/api.rs`:**
```rust
/// High-level API for UI consumption
pub struct AiobscuraApi {
    db: Database,
    analytics: AnalyticsEngine,
}

impl AiobscuraApi {
    pub fn get_session_analytics(&self, session_id: &str) -> Result<SessionAnalytics>;
    pub fn get_thread_analytics(&self, thread_id: &str) -> Result<ThreadAnalytics>;
    pub fn list_threads(&self, filter: &ThreadFilter) -> Result<Vec<ThreadRow>>;
    // ... etc
}
```

**Trade-off:**
- **Pro:** Clean separation, TUI only talks to API
- **Con:** Additional layer of indirection
- **Mitigation:** Keep it thin; mainly orchestration

---

## 6. Priority Matrix

| Fix | Impact | Effort | Priority |
|-----|--------|--------|----------|
| Fix 2: Efficient Query Methods | High (perf) | Medium | **P0** |
| Fix 3: Move EnvironmentHealth | Medium | Low | **P1** |
| Fix 4: Utility Module | Low | Low | **P1** |
| Fix 1: SessionType Enum | Medium | Medium | **P2** |
| Fix 5: API Module | High (future) | High | **P2** |

Note: Original "Fix 2: Move View Models to Core" was **rejected** after analysis — ThreadRow/SessionRow correctly belong in the TUI layer due to UI-specific fields.

---

## 7. Refined Architecture: Three-Layer Model

Based on this analysis, the recommended architecture separates concerns into three distinct layers:

```
┌─────────────────────────────────────────────────────────────────────────┐
│ LAYER 1: CORE (Domain Types)                                            │
│                                                                         │
│ Location: aiobscura-core/src/types.rs                                   │
│ Contents: Session, Thread, Message, Plan, Project, BackingModel         │
│ Principle: Pure DDD entities, normalized, no computed fields            │
│ Used by: Everything                                                     │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ LAYER 2: AGGREGATES (Query Layer)                                       │
│                                                                         │
│ Location: aiobscura-core/src/db/repo.rs (aggregate methods)             │
│ Contents: Efficient queries returning domain types + computed values    │
│ Examples:                                                               │
│   - list_threads_with_counts() → Vec<(Thread, i64)>                     │
│   - get_project_stats() → ProjectStats                                  │
│   - get_environment_health() → EnvironmentHealth                        │
│ Principle: First/second-order aggregates over domain types              │
│ Used by: Any UI that needs efficient data access                        │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ LAYER 3: VIEW (UI-Specific Shaping)                                     │
│                                                                         │
│ Location: aiobscura/src/thread_row.rs, app.rs                           │
│ Contents: ThreadRow, SessionRow (with indent_level, is_last_child)      │
│ Principle: UI-specific decoration and layout concerns                   │
│ Note: Different UIs will have different view types                      │
└─────────────────────────────────────────────────────────────────────────┘
```

### Key Boundaries

| Concern | Layer | Example |
|---------|-------|---------|
| Entity identity & relationships | Core | `Thread.parent_thread_id` |
| Counting, summing, joining | Aggregates | `message_count: i64` |
| Tree indentation, relative time | View | `indent_level: usize` |

### The Rule

**Ask: "Is this intrinsic to the domain, or is it computed for display?"**

- `Thread.thread_type` → Domain (intrinsic to what a thread IS)
- `message_count` → Aggregate (computed from messages)
- `indent_level` → View (for TUI tree rendering)

---

## 8. Innocuous "Leaks" (Acceptable Trade-offs)

Some patterns that **look** like leaks but are actually acceptable:

1. **TUI importing core types directly** - This is fine; the core library is designed to be consumed this way.

2. **Plan using session_id instead of agent/project_path** - Actually a **better** design than requirements; maintains normalization.

3. **Message having both emitted_at and observed_at** - Enhancement over requirements; useful for debugging ingestion lag.

4. **Deprecation aliases (AgentType, Event, EventType)** - Proper migration strategy; will be removed after full migration.

5. **ThreadRow/SessionRow in TUI** - Correctly placed; they contain UI-specific fields (`indent_level`, `is_last_child`) that are presentation concerns, not domain or aggregate concerns.
