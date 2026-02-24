//! Publisher for batching and sending events to Catsyphon
//!
//! The Publisher manages event batching and provides both sync and async
//! interfaces for sending events to a Catsyphon server.
//!
//! ## Architecture
//!
//! - `Publisher`: Core async publisher with batching and retry logic
//! - `SyncPublisher`: Blocking wrapper for use in synchronous code
//! - `StatefulSyncPublisher`: Full-featured publisher with database persistence
//!   for sequence tracking and crash recovery

use std::collections::HashMap;
use std::time::Instant;

use chrono::{Duration, Utc};

use crate::config::CollectorConfig;
use crate::db::{CollectorPublishState, Database};
use crate::error::{Error, Result};
use crate::types::Message;

use super::client::CollectorClient;
use super::events::{CollectorEvent, EventBatch};

const STALE_COMPLETION_MINUTES: i64 = 60;
const STALE_COMPLETION_OUTCOME: &str = "partial";
const STALE_COMPLETION_SUMMARY: &str = "Completed by aiobscura after inactivity";

/// Manages event publishing to Catsyphon
///
/// The Publisher batches events per session and sends them when:
/// - Batch size threshold is reached
/// - Flush is explicitly called
/// - Flush interval expires (when using watch mode)
pub struct Publisher {
    client: CollectorClient,
    /// Buffered events per session
    buffers: HashMap<String, SessionBuffer>,
    /// Stats for reporting
    stats: PublishStats,
}

/// Buffer for a single session's events
struct SessionBuffer {
    #[allow(dead_code)] // Used for debugging/logging
    session_id: String,
    events: Vec<CollectorEvent>,
    /// Sequence numbers for each event, used for tracking progress
    event_seqs: Vec<i32>,
    last_flush: Instant,
}

/// Publishing statistics
#[derive(Debug, Default, Clone)]
pub struct PublishStats {
    /// Total events sent successfully
    pub events_sent: usize,
    /// Total events rejected by server
    pub events_rejected: usize,
    /// Number of API calls made
    pub api_calls: usize,
    /// Number of failed API calls
    pub api_failures: usize,
}

impl Publisher {
    /// Create a new publisher from configuration
    ///
    /// Returns None if collector is not enabled or not properly configured.
    pub fn new(config: &CollectorConfig) -> Result<Option<Self>> {
        if !config.is_ready() {
            return Ok(None);
        }

        let client = CollectorClient::new(config.clone())?;
        Ok(Some(Self {
            client,
            buffers: HashMap::new(),
            stats: PublishStats::default(),
        }))
    }

    /// Queue messages for publishing
    ///
    /// Messages are buffered and sent when batch size is reached.
    /// Returns the number of events sent (may be 0 if still buffering).
    pub async fn queue(&mut self, messages: &[Message]) -> Result<usize> {
        if messages.is_empty() {
            return Ok(0);
        }

        let batch_size = self.client.batch_size();
        let mut total_sent = 0;

        // Group messages by session
        let mut by_session: HashMap<String, Vec<&Message>> = HashMap::new();
        for msg in messages {
            by_session
                .entry(msg.session_id.clone())
                .or_default()
                .push(msg);
        }

        // Add to buffers and flush if needed
        for (session_id, msgs) in by_session {
            let buffer = self
                .buffers
                .entry(session_id.clone())
                .or_insert_with(|| SessionBuffer {
                    session_id: session_id.clone(),
                    events: Vec::new(),
                    event_seqs: Vec::new(),
                    last_flush: Instant::now(),
                });

            // Convert messages to events, tracking sequence numbers
            for msg in msgs {
                buffer.events.push(CollectorEvent::from_message(msg));
                buffer.event_seqs.push(msg.seq);
            }

            // Flush if batch size reached
            if buffer.events.len() >= batch_size {
                total_sent += self.flush_session_count(&session_id).await?;
            }
        }

        Ok(total_sent)
    }

    /// Flush all pending events for a session
    ///
    /// Returns (events_sent, max_seq) where max_seq is the highest sequence
    /// number that was successfully published, or None if nothing was sent.
    async fn flush_session(&mut self, session_id: &str) -> Result<(usize, Option<i32>)> {
        let buffer = match self.buffers.get_mut(session_id) {
            Some(b) if !b.events.is_empty() => b,
            _ => return Ok((0, None)),
        };

        let events: Vec<CollectorEvent> = buffer.events.drain(..).collect();
        let seqs: Vec<i32> = buffer.event_seqs.drain(..).collect();
        let max_seq = seqs.iter().max().copied();
        buffer.last_flush = Instant::now();

        let batch = EventBatch {
            session_id: session_id.to_string(),
            events,
        };

        self.stats.api_calls += 1;

        match self.client.send_events_with_retry(&batch).await {
            Ok(response) => {
                self.stats.events_sent += response.accepted;
                self.stats.events_rejected += response.rejected;
                tracing::debug!(
                    session_id = %session_id,
                    accepted = response.accepted,
                    rejected = response.rejected,
                    max_seq = ?max_seq,
                    "Published events to Catsyphon"
                );
                Ok((response.accepted, max_seq))
            }
            Err(e) => {
                self.stats.api_failures += 1;
                tracing::warn!(
                    session_id = %session_id,
                    error = %e,
                    "Failed to publish events to Catsyphon"
                );
                // Don't return error - we don't want to block local operation
                // Events are lost but local DB has them
                Ok((0, None))
            }
        }
    }

    /// Flush all pending events for a session (simple interface)
    async fn flush_session_count(&mut self, session_id: &str) -> Result<usize> {
        let (count, _) = self.flush_session(session_id).await?;
        Ok(count)
    }

    /// Flush all pending events across all sessions
    pub async fn flush_all(&mut self) -> Result<usize> {
        let session_ids: Vec<String> = self.buffers.keys().cloned().collect();
        let mut total_sent = 0;

        for session_id in session_ids {
            total_sent += self.flush_session_count(&session_id).await?;
        }

        Ok(total_sent)
    }

    /// Flush all pending events with sequence tracking
    ///
    /// Returns a map of session_id -> max_seq for sessions that had events published.
    pub async fn flush_all_with_seqs(&mut self) -> Result<HashMap<String, i32>> {
        let session_ids: Vec<String> = self.buffers.keys().cloned().collect();
        let mut results = HashMap::new();

        for session_id in session_ids {
            let (_, max_seq) = self.flush_session(&session_id).await?;
            if let Some(seq) = max_seq {
                results.insert(session_id, seq);
            }
        }

        Ok(results)
    }

    /// Get current publishing statistics
    pub fn stats(&self) -> &PublishStats {
        &self.stats
    }

    /// Get number of pending events across all buffers
    pub fn pending_count(&self) -> usize {
        self.buffers.values().map(|b| b.events.len()).sum()
    }

    /// Check if there are any pending events
    pub fn has_pending(&self) -> bool {
        self.buffers.values().any(|b| !b.events.is_empty())
    }
}

/// Synchronous wrapper for Publisher
///
/// Provides blocking methods for use in synchronous code.
pub struct SyncPublisher {
    inner: Publisher,
    runtime: tokio::runtime::Runtime,
}

impl SyncPublisher {
    /// Create a new sync publisher from configuration
    ///
    /// Returns None if collector is not enabled or not properly configured.
    pub fn new(config: &CollectorConfig) -> Result<Option<Self>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| {
                crate::error::Error::Collector(format!("failed to create runtime: {}", e))
            })?;

        match Publisher::new(config)? {
            Some(publisher) => Ok(Some(Self {
                inner: publisher,
                runtime,
            })),
            None => Ok(None),
        }
    }

    /// Queue messages for publishing (blocking)
    pub fn queue(&mut self, messages: &[Message]) -> Result<usize> {
        self.runtime.block_on(self.inner.queue(messages))
    }

    /// Flush all pending events (blocking)
    pub fn flush_all(&mut self) -> Result<usize> {
        self.runtime.block_on(self.inner.flush_all())
    }

    /// Get current publishing statistics
    pub fn stats(&self) -> &PublishStats {
        self.inner.stats()
    }

    /// Get number of pending events
    pub fn pending_count(&self) -> usize {
        self.inner.pending_count()
    }

    /// Check if there are any pending events
    pub fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }
}

/// Stateful publisher with database persistence for sequence tracking
///
/// This is the full-featured publisher that:
/// - Tracks publish progress per session in the database
/// - Supports crash recovery by resuming from last published sequence
/// - Provides sequence-based queries for efficient publishing
pub struct StatefulSyncPublisher {
    inner: Publisher,
    runtime: tokio::runtime::Runtime,
    db: Database,
}

impl StatefulSyncPublisher {
    /// Create a new stateful publisher from configuration and database
    ///
    /// Returns None if collector is not enabled or not properly configured.
    pub fn new(config: &CollectorConfig, db: Database) -> Result<Option<Self>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| {
                crate::error::Error::Collector(format!("failed to create runtime: {}", e))
            })?;

        match Publisher::new(config)? {
            Some(publisher) => Ok(Some(Self {
                inner: publisher,
                runtime,
                db,
            })),
            None => Ok(None),
        }
    }

    fn ensure_publish_state(&self, session_id: &str) -> Result<CollectorPublishState> {
        if let Some(state) = self.db.get_collector_publish_state(session_id)? {
            return Ok(state);
        }

        let now = Utc::now();
        let state = CollectorPublishState {
            session_id: session_id.to_string(),
            last_published_seq: 0,
            last_published_at: None,
            status: "active".to_string(),
            error_message: None,
            created_at: now,
            updated_at: now,
        };
        self.db.upsert_collector_publish_state(&state)?;
        Ok(state)
    }

    fn build_session_start_event(&self, session_id: &str) -> Result<CollectorEvent> {
        let session = self
            .db
            .get_session(session_id)?
            .ok_or_else(|| Error::SessionNotFound(session_id.to_string()))?;
        let project = match session.project_id.as_deref() {
            Some(project_id) => self.db.get_project(project_id)?,
            None => None,
        };

        let mut payload = serde_json::Map::new();
        payload.insert(
            "agent_type".to_string(),
            serde_json::Value::String(session.assistant.as_str().to_string()),
        );
        payload.insert(
            "agent_version".to_string(),
            serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string()),
        );

        let working_directory = session
            .metadata
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .or_else(|| {
                project
                    .as_ref()
                    .map(|p| p.path.to_string_lossy().to_string())
            });
        if let Some(cwd) = working_directory {
            payload.insert(
                "working_directory".to_string(),
                serde_json::Value::String(cwd),
            );
        }

        if let Some(git_branch) = session
            .metadata
            .get("git_branch")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
        {
            payload.insert(
                "git_branch".to_string(),
                serde_json::Value::String(git_branch),
            );
        }

        for key in [
            "parent_session_id",
            "context_semantics",
            "slug",
            "summaries",
            "compaction_events",
        ] {
            if let Some(value) = session.metadata.get(key) {
                payload.insert(key.to_string(), value.clone());
            }
        }

        Ok(CollectorEvent {
            event_type: "session_start".to_string(),
            emitted_at: session.started_at,
            observed_at: Utc::now(),
            event_hash: None,
            data: serde_json::Value::Object(payload),
        })
    }

    fn ensure_remote_session_started(
        &mut self,
        state: &CollectorPublishState,
    ) -> Result<CollectorPublishState> {
        if state.status == "completed"
            || state.last_published_seq > 0
            || state.last_published_at.is_some()
        {
            return Ok(state.clone());
        }

        let session_start = self.build_session_start_event(&state.session_id)?;
        self.runtime.block_on(
            self.inner
                .client
                .ensure_session_started(&state.session_id, session_start),
        )?;

        let now = Utc::now();
        let updated = CollectorPublishState {
            session_id: state.session_id.clone(),
            last_published_seq: state.last_published_seq,
            last_published_at: Some(now),
            status: "active".to_string(),
            error_message: None,
            created_at: state.created_at,
            updated_at: now,
        };
        self.db.upsert_collector_publish_state(&updated)?;
        Ok(updated)
    }

    fn maybe_complete_stale_session(&mut self, state: &CollectorPublishState) -> Result<bool> {
        if state.status == "completed" {
            return Ok(false);
        }

        if !self
            .db
            .get_unpublished_messages(&state.session_id, state.last_published_seq, 1)?
            .is_empty()
        {
            return Ok(false);
        }

        let Some(session) = self.db.get_session(&state.session_id)? else {
            return Ok(false);
        };
        let last_activity = session.last_activity_at.unwrap_or(session.started_at);
        if Utc::now().signed_duration_since(last_activity)
            < Duration::minutes(STALE_COMPLETION_MINUTES)
        {
            return Ok(false);
        }

        let event_count = self.db.count_session_messages(&state.session_id)?;
        let completed = self.runtime.block_on(self.inner.client.complete_session(
            &state.session_id,
            STALE_COMPLETION_OUTCOME,
            Some(STALE_COMPLETION_SUMMARY),
            Some(event_count.max(0)),
        ))?;
        if !completed {
            return Ok(false);
        }

        let now = Utc::now();
        let completed_state = CollectorPublishState {
            session_id: state.session_id.clone(),
            last_published_seq: state.last_published_seq,
            last_published_at: Some(now),
            status: "completed".to_string(),
            error_message: None,
            created_at: state.created_at,
            updated_at: now,
        };
        self.db.upsert_collector_publish_state(&completed_state)?;
        Ok(true)
    }

    /// Publish new messages for a session using sequence-based tracking
    ///
    /// This method:
    /// 1. Gets the last published sequence for the session from the database
    /// 2. Queries for messages with seq > last_published_seq
    /// 3. Publishes them and updates the database with the new last_published_seq
    ///
    /// Returns the number of events published.
    pub fn publish_session(&mut self, session_id: &str, batch_size: usize) -> Result<usize> {
        let state = self.ensure_publish_state(session_id)?;
        let state = self.ensure_remote_session_started(&state)?;

        let messages =
            self.db
                .get_unpublished_messages(session_id, state.last_published_seq, batch_size)?;
        if messages.is_empty() {
            let _ = self.maybe_complete_stale_session(&state)?;
            return Ok(0);
        }

        // Queue and flush
        let sent_with_seqs = self.runtime.block_on(async {
            self.inner.queue(&messages).await?;
            self.inner.flush_all_with_seqs().await
        })?;

        if let Some(max_seq) = sent_with_seqs.get(session_id) {
            let now = Utc::now();
            let new_state = CollectorPublishState {
                session_id: session_id.to_string(),
                last_published_seq: i64::from(*max_seq),
                last_published_at: Some(now),
                status: "active".to_string(),
                error_message: None,
                created_at: state.created_at,
                updated_at: now,
            };
            self.db.upsert_collector_publish_state(&new_state)?;

            let _ = self.maybe_complete_stale_session(&new_state)?;
            return Ok(messages.len());
        }

        let now = Utc::now();
        let failed_state = CollectorPublishState {
            session_id: session_id.to_string(),
            last_published_seq: state.last_published_seq,
            last_published_at: state.last_published_at,
            status: "active".to_string(),
            error_message: Some("publish failed; will retry".to_string()),
            created_at: state.created_at,
            updated_at: now,
        };
        self.db.upsert_collector_publish_state(&failed_state)?;
        Ok(0)
    }

    /// Resume publishing for all incomplete sessions
    ///
    /// Finds all sessions with unpublished messages and publishes them.
    /// This is used for crash recovery on startup.
    ///
    /// Returns the total number of events published.
    pub fn resume_incomplete(&mut self, batch_size: usize) -> Result<usize> {
        let incomplete = self.db.get_incomplete_publish_states()?;
        let mut total_sent = 0;

        for state in incomplete {
            tracing::info!(
                session_id = %state.session_id,
                last_published_seq = state.last_published_seq,
                "Resuming incomplete publish"
            );

            match self.publish_session(&state.session_id, batch_size) {
                Ok(sent) => {
                    total_sent += sent;
                    if sent > 0 {
                        tracing::info!(
                            session_id = %state.session_id,
                            events_sent = sent,
                            "Resumed session publish"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        session_id = %state.session_id,
                        error = %e,
                        "Failed to resume session publish"
                    );
                    let now = Utc::now();
                    let failed_state = CollectorPublishState {
                        session_id: state.session_id.clone(),
                        last_published_seq: state.last_published_seq,
                        last_published_at: state.last_published_at,
                        status: "active".to_string(),
                        error_message: Some(e.to_string()),
                        created_at: state.created_at,
                        updated_at: now,
                    };
                    let _ = self.db.upsert_collector_publish_state(&failed_state);
                }
            }
        }

        Ok(total_sent)
    }

    /// Complete sessions that have no unpublished messages and are stale.
    ///
    /// Returns the number of sessions marked completed on the server.
    pub fn complete_stale_sessions(&mut self) -> Result<usize> {
        let active_states = self.db.get_active_publish_states()?;
        let mut completed = 0;

        for state in active_states {
            if self.maybe_complete_stale_session(&state)? {
                completed += 1;
            }
        }

        Ok(completed)
    }

    /// Queue messages for publishing with sequence tracking
    ///
    /// This method updates publish state after successful sends.
    pub fn queue_with_tracking(&mut self, messages: &[Message]) -> Result<usize> {
        if messages.is_empty() {
            return Ok(0);
        }

        // Queue and flush
        let sent = self.runtime.block_on(async {
            self.inner.queue(messages).await?;
            self.inner.flush_all_with_seqs().await
        })?;

        // Update publish state for each session that had events published
        let now = Utc::now();
        for (session_id, max_seq) in sent {
            let state = self.db.get_collector_publish_state(&session_id)?;
            let new_state = CollectorPublishState {
                session_id: session_id.clone(),
                last_published_seq: max_seq as i64,
                last_published_at: Some(now),
                status: "active".to_string(),
                error_message: None,
                created_at: state.map(|s| s.created_at).unwrap_or(now),
                updated_at: now,
            };
            self.db.upsert_collector_publish_state(&new_state)?;
        }

        Ok(self.inner.stats().events_sent)
    }

    /// Flush all pending events with state persistence
    pub fn flush_all(&mut self) -> Result<usize> {
        let seqs = self.runtime.block_on(self.inner.flush_all_with_seqs())?;

        // Update publish state for each session
        let now = Utc::now();
        for (session_id, max_seq) in seqs {
            let state = self.db.get_collector_publish_state(&session_id)?;
            let new_state = CollectorPublishState {
                session_id: session_id.clone(),
                last_published_seq: max_seq as i64,
                last_published_at: Some(now),
                status: "active".to_string(),
                error_message: None,
                created_at: state.map(|s| s.created_at).unwrap_or(now),
                updated_at: now,
            };
            self.db.upsert_collector_publish_state(&new_state)?;
        }

        Ok(self.inner.stats().events_sent)
    }

    /// Get current publishing statistics
    pub fn stats(&self) -> &PublishStats {
        self.inner.stats()
    }

    /// Get number of pending events
    pub fn pending_count(&self) -> usize {
        self.inner.pending_count()
    }

    /// Check if there are any pending events
    pub fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }

    /// Get the database reference
    pub fn database(&self) -> &Database {
        &self.db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_publisher_disabled_config() {
        let config = CollectorConfig::default();
        let publisher = Publisher::new(&config).unwrap();
        assert!(publisher.is_none());
    }

    #[test]
    fn test_sync_publisher_disabled_config() {
        let config = CollectorConfig::default();
        let publisher = SyncPublisher::new(&config).unwrap();
        assert!(publisher.is_none());
    }

    #[test]
    fn test_publish_stats_default() {
        let stats = PublishStats::default();
        assert_eq!(stats.events_sent, 0);
        assert_eq!(stats.api_calls, 0);
    }
}
