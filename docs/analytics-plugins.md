# Analytics Plugin Guide

Create custom analytics plugins to extract insights from AI coding sessions.

## Quick Start

```rust
use aiobscura_core::analytics::{
    AnalyticsPlugin, AnalyticsContext, AnalyticsTrigger, MetricOutput
};
use aiobscura_core::error::Result;
use aiobscura_core::types::{Message, Session};

pub struct MyPlugin;

impl AnalyticsPlugin for MyPlugin {
    fn name(&self) -> &str {
        "custom.my_plugin"  // Convention: namespace.plugin_name
    }

    fn triggers(&self) -> Vec<AnalyticsTrigger> {
        vec![AnalyticsTrigger::OnDemand]
    }

    fn analyze_session(
        &self,
        session: &Session,
        messages: &[Message],
        _ctx: &AnalyticsContext,
    ) -> Result<Vec<MetricOutput>> {
        // Your analysis logic here
        let count = messages.len();
        
        Ok(vec![
            MetricOutput::session(&session.id, "message_count", serde_json::json!(count)),
        ])
    }
}
```

## Registering Your Plugin

```rust
use aiobscura_core::analytics::{AnalyticsEngine, create_default_engine};

// Option 1: Add to default engine
let mut engine = create_default_engine();
engine.register(Box::new(MyPlugin));

// Option 2: Fresh engine with only your plugins
let mut engine = AnalyticsEngine::new();
engine.register(Box::new(MyPlugin));
```

## Core Concepts

### MetricOutput

Metrics are JSON values tagged with an entity type and ID:

```rust
// Session-level metric
MetricOutput::session(&session.id, "churn_ratio", json!(0.42))

// Thread-level metric  
MetricOutput::thread(&thread.id, "edit_count", json!(15))

// Global metric (not tied to entity)
MetricOutput::global("total_sessions", json!(100))
```

Metrics are stored in the `plugin_metrics` table and automatically upserted (same plugin + entity + metric name = update).

### AnalyticsContext

Provides read-only database access if you need to query related data:

```rust
fn analyze_session(
    &self,
    session: &Session,
    messages: &[Message],
    ctx: &AnalyticsContext,
) -> Result<Vec<MetricOutput>> {
    // Query threads for this session
    let threads = ctx.db.get_session_threads(&session.id)?;
    
    // Query project info
    if let Some(project_id) = &session.project_id {
        let project = ctx.db.get_project(project_id)?;
    }
    
    // ... compute metrics
}
```

### Thread-Level Analysis

To support per-thread analytics, implement `supports_thread_analysis()` and `analyze_thread()`:

```rust
impl AnalyticsPlugin for MyPlugin {
    // ... name(), triggers(), analyze_session() ...

    fn supports_thread_analysis(&self) -> bool {
        true
    }

    fn analyze_thread(
        &self,
        thread: &Thread,
        messages: &[Message],
        _ctx: &AnalyticsContext,
    ) -> Result<Vec<MetricOutput>> {
        Ok(vec![
            MetricOutput::thread(&thread.id, "depth", json!(messages.len())),
        ])
    }
}
```

## Message Structure

Key fields available on each `Message`:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `i64` | Database ID |
| `session_id` | `String` | Parent session |
| `thread_id` | `String` | Parent thread |
| `ts` | `DateTime<Utc>` | Timestamp |
| `author_role` | `AuthorRole` | Human, Assistant, Agent, Tool, System |
| `message_type` | `MessageType` | Prompt, Response, ToolCall, ToolResult, etc. |
| `content` | `Option<String>` | Text content |
| `tool_name` | `Option<String>` | Tool name for ToolCall/ToolResult |
| `tool_input` | `Option<Value>` | JSON input for tool calls |
| `tool_result` | `Option<String>` | Result from tool execution |
| `tokens_in` | `Option<i32>` | Input tokens (if available) |
| `tokens_out` | `Option<i32>` | Output tokens (if available) |

### Filtering Messages

```rust
// Find all tool calls
let tool_calls: Vec<_> = messages
    .iter()
    .filter(|m| m.message_type == MessageType::ToolCall)
    .collect();

// Find Edit tool calls specifically
let edits: Vec<_> = messages
    .iter()
    .filter(|m| m.tool_name.as_deref() == Some("Edit"))
    .collect();

// Find human prompts
let prompts: Vec<_> = messages
    .iter()
    .filter(|m| m.author_role == AuthorRole::Human)
    .collect();
```

## Best Practices

### Plugin Design

1. **Deterministic** - Same input produces same output
2. **Idempotent** - Safe to run multiple times (metrics are upserted)
3. **Fast** - Avoid expensive operations; sessions can have 10k+ messages
4. **Focused** - One plugin per concern (don't combine unrelated metrics)

### Naming Conventions

- Plugin names: `namespace.plugin_name` (e.g., `core.edit_churn`, `llm.assessment`)
- Metric names: `snake_case` (e.g., `edit_count`, `churn_ratio`, `high_churn_files`)

### Error Handling

Return `Err(...)` only for unrecoverable errors. For missing/invalid data, return empty metrics or sensible defaults:

```rust
fn analyze_session(...) -> Result<Vec<MetricOutput>> {
    // Handle empty session gracefully
    if messages.is_empty() {
        return Ok(vec![
            MetricOutput::session(&session.id, "edit_count", json!(0)),
        ]);
    }
    
    // ... normal analysis
}
```

### Metric Values

Use JSON for flexibility:

```rust
// Scalar
json!(42)
json!(0.75)
json!("success")

// Array (e.g., list of files)
json!(["src/main.rs", "src/lib.rs"])

// Object (e.g., breakdown by category)
json!({
    "rs": 15,
    "ts": 8,
    "md": 3
})
```

## Example: Token Usage Plugin

```rust
pub struct TokenUsagePlugin;

impl AnalyticsPlugin for TokenUsagePlugin {
    fn name(&self) -> &str { "custom.token_usage" }
    
    fn triggers(&self) -> Vec<AnalyticsTrigger> {
        vec![AnalyticsTrigger::OnDemand]
    }

    fn analyze_session(
        &self,
        session: &Session,
        messages: &[Message],
        _ctx: &AnalyticsContext,
    ) -> Result<Vec<MetricOutput>> {
        let total_in: i64 = messages
            .iter()
            .filter_map(|m| m.tokens_in.map(|t| t as i64))
            .sum();
            
        let total_out: i64 = messages
            .iter()
            .filter_map(|m| m.tokens_out.map(|t| t as i64))
            .sum();

        Ok(vec![
            MetricOutput::session(&session.id, "tokens_in", json!(total_in)),
            MetricOutput::session(&session.id, "tokens_out", json!(total_out)),
            MetricOutput::session(&session.id, "tokens_total", json!(total_in + total_out)),
        ])
    }
}
```

## Running Plugins

```rust
// Run on a single session
let result = engine.run_plugin("custom.my_plugin", &session, &messages, &db)?;
println!("Produced {} metrics in {}ms", result.metrics_produced, result.duration_ms);

// Run all plugins on a session
let results = engine.run_all(&session, &messages, &db);

// Run all plugins on all sessions
let (total_runs, errors) = engine.run_all_sessions(&db)?;
```

## Storage Schema

Metrics are stored in `plugin_metrics`:

| Column | Type | Description |
|--------|------|-------------|
| `plugin_name` | TEXT | Plugin that produced this metric |
| `entity_type` | TEXT | "session", "thread", "project", "global" |
| `entity_id` | TEXT | ID of the entity (nullable for global) |
| `metric_name` | TEXT | Name of the metric |
| `metric_value` | JSON | The computed value |
| `version` | INT | Schema version (for cache invalidation) |
| `computed_at` | TIMESTAMP | When this was computed |

Plugin runs are logged to `plugin_runs` for observability.

## See Also

- `aiobscura-core/src/analytics/engine.rs` - Full engine implementation
- `aiobscura-core/src/analytics/plugins/edit_churn/` - Reference implementation
- `docs/edit-churn-algorithm.md` - Detailed algorithm documentation
