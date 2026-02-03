# Design: CollectorClient for Catsyphon Integration

## Overview

Add optional capability for aiobscura to push events to a Catsyphon server via the Collector Events API. This enables aiobscura to serve as a lightweight, efficient Rust-based collector for enterprise Catsyphon deployments.

### Goals

1. **Optional integration** - Catsyphon publishing is opt-in via configuration
2. **Local-first** - Always store locally first, then publish (never lose data)
3. **Efficient** - Minimal overhead when disabled; batched/async when enabled
4. **Resilient** - Handle network failures gracefully with retry logic
5. **Schema-aligned** - Leverage existing type alignment between aiobscura and Catsyphon

### Non-Goals

- Replacing local SQLite storage (always kept)
- Real-time streaming (batch-based is sufficient)
- Bidirectional sync (push-only to Catsyphon)

---

## Architecture

### Current Flow (unchanged)

```
Log Files → Parser → Messages → SQLite DB
```

### New Flow (with CollectorClient)

```
Log Files → Parser → Messages → SQLite DB
                                    ↓
                            CollectorClient (optional)
                                    ↓
                            Catsyphon Server API
```

### Key Principle: Local-First

The CollectorClient operates **after** successful database insertion:
- If DB insert succeeds but API fails → data is safe locally, retry later
- If DB insert fails → no API call attempted
- Network issues never block local operation

---

## Components

### 1. Configuration (`config.rs`)

Add new `[collector]` section to `~/.config/aiobscura/config.toml`:

```toml
[collector]
# Enable/disable Catsyphon integration
enabled = false

# Catsyphon server URL
server_url = "https://catsyphon.example.com"

# Credentials (from registration)
collector_id = "uuid"
api_key = "cs_live_xxxxxxxxxxxx"

# Optional settings
batch_size = 20          # Events per API call (max 50)
flush_interval_secs = 5  # Max time before flush
timeout_secs = 30        # HTTP timeout
max_retries = 3          # Retry attempts for transient failures
```

```rust
// In config.rs
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CollectorConfig {
    #[serde(default)]
    pub enabled: bool,
    pub server_url: Option<String>,
    pub collector_id: Option<String>,
    pub api_key: Option<String>,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_flush_interval")]
    pub flush_interval_secs: u64,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: usize,
}

fn default_batch_size() -> usize { 20 }
fn default_flush_interval() -> u64 { 5 }
fn default_timeout() -> u64 { 30 }
fn default_max_retries() -> usize { 3 }
```

### 2. CollectorClient (`collector/client.rs`)

Core HTTP client for Catsyphon API communication.

```rust
pub struct CollectorClient {
    config: CollectorConfig,
    http_client: reqwest::Client,
}

impl CollectorClient {
    /// Create client from configuration
    pub fn new(config: CollectorConfig) -> Result<Self>;

    /// Send a batch of events for a session
    pub async fn send_events(
        &self,
        session_id: &str,
        events: &[CollectorEvent],
    ) -> Result<EventsResponse>;

    /// Get session status (for resumption after failures)
    pub async fn get_session_status(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionStatus>>;

    // NOTE: No complete_session() method - session lifecycle is managed
    // by Catsyphon server based on inactivity, not by collectors.
}
```

### 3. Event Transformation (`collector/events.rs`)

Convert aiobscura `Message` to Catsyphon `CollectorEvent`.

#### Timestamp Semantics

Events flow through a three-stage pipeline, with each stage recording its observation time:

```
Source (Claude Code logs) → Collector (aiobscura) → Catsyphon Server
        emitted_at                observed_at         server_received_at
```

| Timestamp | Set By | Definition |
|-----------|--------|------------|
| `emitted_at` | Source log file | When the event was originally produced by the AI assistant |
| `observed_at` | aiobscura parser | When aiobscura first parsed/ingested this event from the log file |
| `server_received_at` | Catsyphon server | When the API received the event (set automatically by server) |

This enables end-to-end latency measurement across the ingestion pipeline.

**aiobscura already captures both timestamps correctly** in `Message` (see `types.rs:704-708`),
so the mapping is direct: `msg.emitted_at` → `event.emitted_at`, `msg.observed_at` → `event.observed_at`.

```rust
/// Catsyphon event envelope
#[derive(Debug, Serialize)]
pub struct CollectorEvent {
    pub sequence: u32,
    #[serde(rename = "type")]
    pub event_type: String,
    /// When the event was originally produced (from source log)
    pub emitted_at: DateTime<Utc>,
    /// When aiobscura parsed this event
    pub observed_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_hash: Option<String>,
    pub data: serde_json::Value,
}

/// Convert aiobscura Message to CollectorEvent
impl From<&Message> for CollectorEvent {
    fn from(msg: &Message) -> Self {
        CollectorEvent {
            sequence: msg.seq as u32,
            event_type: map_message_type(&msg.message_type),
            emitted_at: msg.emitted_at,
            observed_at: msg.observed_at,
            event_hash: Some(compute_event_hash(msg)),
            data: build_event_data(msg),
        }
    }
}

/// Map aiobscura MessageType to Catsyphon event type
fn map_message_type(mt: &MessageType) -> String {
    match mt {
        MessageType::Prompt | MessageType::Response => "message",
        MessageType::ToolCall => "tool_call",
        MessageType::ToolResult => "tool_result",
        MessageType::Plan => "message",  // with message_type=plan in data
        MessageType::Summary => "message",
        MessageType::Context => "message",
        MessageType::Error => "error",
    }.to_string()
}
```

### 4. Event Buffer (`collector/buffer.rs`)

Manages batching and flush timing.

```rust
pub struct EventBuffer {
    session_id: String,
    events: Vec<CollectorEvent>,
    next_sequence: u32,
    batch_size: usize,
    last_flush: Instant,
    flush_interval: Duration,
}

impl EventBuffer {
    pub fn new(session_id: String, config: &CollectorConfig) -> Self;

    /// Add event, returns true if buffer should be flushed
    pub fn add(&mut self, event: CollectorEvent) -> bool;

    /// Check if flush is needed (batch full or interval elapsed)
    pub fn should_flush(&self) -> bool;

    /// Take events for sending, reset buffer
    pub fn take(&mut self) -> Vec<CollectorEvent>;

    /// Number of pending events
    pub fn len(&self) -> usize;
}
```

### 5. Session Tracker (`collector/tracker.rs`)

Tracks publishing state per session for resumption.

```rust
/// Persisted in SQLite for crash recovery
pub struct SessionPublishState {
    pub session_id: String,
    pub last_published_seq: i32,
    pub last_published_at: Option<DateTime<Utc>>,
    pub status: PublishStatus,
}

pub enum PublishStatus {
    Active,      // Publishing in progress
    Completed,   // Session marked complete on server
    Failed,      // Permanent failure (auth error, etc.)
}
```

### 6. Publisher Service (`collector/publisher.rs`)

Orchestrates the publishing pipeline.

```rust
pub struct Publisher {
    client: CollectorClient,
    buffers: HashMap<String, EventBuffer>,
    tracker: SessionTracker,
}

impl Publisher {
    /// Queue messages for publishing (called after DB insert)
    pub async fn queue(&mut self, messages: &[Message]) -> Result<()>;

    /// Flush all pending buffers
    pub async fn flush_all(&mut self) -> Result<()>;

    /// Resume incomplete sessions after restart
    pub async fn resume_incomplete(&mut self, db: &Database) -> Result<()>;

    // NOTE: No complete_session() - Catsyphon manages session lifecycle
}
```

---

## Integration Points

### 1. IngestCoordinator (`ingest/mod.rs`)

Add optional Publisher to coordinator:

```rust
pub struct IngestCoordinator {
    db: Database,
    parsers: Vec<Box<dyn AssistantParser>>,
    publisher: Option<Publisher>,  // NEW
}

impl IngestCoordinator {
    pub async fn sync_file(&mut self, path: &Path) -> Result<SyncResult> {
        // ... existing parsing logic ...

        // Insert to local DB (existing)
        self.db.insert_messages(&messages)?;

        // NEW: Publish to Catsyphon if enabled
        if let Some(publisher) = &mut self.publisher {
            // Fire-and-forget with error logging (don't block)
            if let Err(e) = publisher.queue(&messages).await {
                tracing::warn!("Failed to queue for Catsyphon: {}", e);
            }
        }

        Ok(result)
    }
}
```

### 2. Sync Daemon (`sync.rs`)

Initialize publisher from config:

```rust
async fn run_sync(config: Config) -> Result<()> {
    let db = Database::open(&db_path)?;

    // NEW: Initialize publisher if configured
    let publisher = if config.collector.enabled {
        let client = CollectorClient::new(config.collector.clone())?;
        let publisher = Publisher::new(client);

        // Resume any incomplete sessions from previous run
        publisher.resume_incomplete(&db).await?;

        Some(publisher)
    } else {
        None
    };

    let coordinator = IngestCoordinator::new(db, publisher);

    // ... rest of sync loop ...

    // On shutdown: flush remaining events
    if let Some(mut pub) = publisher {
        pub.flush_all().await?;
    }
}
```

---

## Database Schema Additions

New table for tracking publish state:

```sql
CREATE TABLE IF NOT EXISTS collector_publish_state (
    session_id TEXT PRIMARY KEY,
    last_published_seq INTEGER NOT NULL DEFAULT 0,
    last_published_at TEXT,
    status TEXT NOT NULL DEFAULT 'active',  -- active, completed, failed
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for finding incomplete sessions on startup
CREATE INDEX IF NOT EXISTS idx_publish_state_status
    ON collector_publish_state(status) WHERE status = 'active';
```

---

## Error Handling

### Retry Strategy

```rust
pub struct RetryConfig {
    pub max_retries: usize,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub exponential_base: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            exponential_base: 2.0,
        }
    }
}
```

### Error Categories

| Error Type | Retry? | Action |
|------------|--------|--------|
| Network timeout | Yes | Exponential backoff |
| 5xx server error | Yes | Exponential backoff |
| 429 rate limited | Yes | Use Retry-After header |
| 401 unauthorized | No | Log error, disable publishing |
| 400 bad request | No | Log error, skip batch |
| 409 sequence gap | Special | Query status, resend from gap |

### Sequence Gap Recovery

```rust
async fn handle_sequence_gap(
    &mut self,
    session_id: &str,
    expected: u32,
) -> Result<()> {
    // Query server for last received sequence
    let status = self.client.get_session_status(session_id).await?;

    if let Some(status) = status {
        // Fetch messages from local DB after last_sequence
        let messages = self.db.get_messages_after_seq(
            session_id,
            status.last_sequence
        )?;

        // Re-queue for publishing
        self.queue(&messages).await?;
    }

    Ok(())
}
```

---

## Dependencies

Add to `aiobscura-core/Cargo.toml`:

```toml
[dependencies]
# HTTP client (async)
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }

# Already have these:
# tokio (workspace)
# serde, serde_json
# chrono
# sha2 (for event hashing)
# tracing
```

### Feature Flag (Optional)

Make Catsyphon integration a compile-time feature:

```toml
[features]
default = []
collector = ["reqwest"]

[dependencies]
reqwest = { version = "0.12", features = ["json"], optional = true }
```

Usage: `cargo build --features collector`

---

## File Structure

```
aiobscura-core/src/
├── collector/
│   ├── mod.rs          # Module exports
│   ├── client.rs       # HTTP client for Catsyphon API
│   ├── events.rs       # Event transformation (Message → CollectorEvent)
│   ├── buffer.rs       # Event batching
│   ├── tracker.rs      # Session publish state tracking
│   ├── publisher.rs    # Orchestration
│   └── error.rs        # Error types
├── config.rs           # Add CollectorConfig
├── db/
│   └── repo.rs         # Add publish_state table operations
└── ingest/
    └── mod.rs          # Integrate Publisher
```

---

## CLI Commands (Future)

Optional CLI commands for collector management:

```bash
# Register with Catsyphon server (stores credentials)
aiobscura collector register --server https://catsyphon.example.com

# Check connection status
aiobscura collector status

# Force flush pending events
aiobscura collector flush

# Show publish statistics
aiobscura collector stats
```

---

## Testing Strategy

### Unit Tests

1. Event transformation (Message → CollectorEvent)
2. Buffer batching logic
3. Retry delay calculation
4. Sequence gap detection

### Integration Tests

1. Mock Catsyphon server (using `wiremock`)
2. Full publish flow with simulated failures
3. Sequence gap recovery
4. Crash recovery (resume incomplete sessions)

### Manual Testing

1. Run against local Catsyphon instance
2. Test network interruption scenarios
3. Verify event schema compatibility

---

## Implementation Phases

### Phase 1: Core Client (MVP)
- [ ] `CollectorConfig` in config.rs
- [ ] `CollectorClient` HTTP operations
- [ ] `CollectorEvent` transformation
- [ ] Basic integration in IngestCoordinator
- [ ] Manual flush on shutdown

### Phase 2: Robustness
- [ ] Event batching with `EventBuffer`
- [ ] Retry logic with exponential backoff
- [ ] Sequence gap recovery
- [ ] `collector_publish_state` table
- [ ] Resume on startup

### Phase 3: CLI & Polish
- [ ] `aiobscura collector` subcommands
- [ ] Metrics/stats tracking
- [ ] Feature flag for optional compilation
- [ ] Documentation

---

## Open Questions

1. **Sync vs Async**: Should we use blocking `ureq` (simpler) or async `reqwest` (more efficient for batching)?
   - Recommendation: Start with async `reqwest` since sync daemon can use `tokio::runtime::Runtime::block_on()`

2. **Credential Storage**: Store in config file or separate credentials file?
   - Recommendation: Config file for simplicity; separate file later if security concerns arise

3. **Backpressure**: What if Catsyphon is slow and buffers grow large?
   - Recommendation: Cap buffer size, drop oldest events with warning log

## Resolved Decisions

1. **Session Completion**: Keep the collector "dumb" - Catsyphon server manages session
   lifecycle based on inactivity. The collector only pushes events; it never marks
   sessions as complete. This simplifies the client and centralizes lifecycle logic.

2. **Timestamp Semantics**: Direct 1:1 mapping from aiobscura's existing timestamps:
   - `Message.emitted_at` → `CollectorEvent.emitted_at` (when event occurred in source)
   - `Message.observed_at` → `CollectorEvent.observed_at` (when aiobscura parsed it)

---

## Success Criteria

1. Events successfully delivered to Catsyphon test server
2. No data loss when network is unavailable (local DB always has data)
3. Graceful recovery after crashes/restarts
4. <5% CPU overhead when collector is enabled
5. Schema validation passes on Catsyphon side
