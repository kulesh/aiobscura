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

use chrono::Utc;

use crate::config::CollectorConfig;
use crate::db::{CollectorPublishState, Database};
use crate::error::Result;
use crate::types::Message;

use super::client::CollectorClient;
use super::events::{CollectorEvent, EventBatch};

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
                Ok((
                    response.accepted,
                    if response.accepted > 0 { max_seq } else { None },
                ))
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

    /// Publish new messages for a session using sequence-based tracking
    ///
    /// This method:
    /// 1. Gets the last published sequence for the session from the database
    /// 2. Queries for messages with seq > last_published_seq
    /// 3. Publishes them and updates the database with the new last_published_seq
    ///
    /// Returns the number of events published.
    pub fn publish_session(&mut self, session_id: &str, batch_size: usize) -> Result<usize> {
        // Get current publish state
        let state = self.db.get_collector_publish_state(session_id)?;
        let last_seq = state.as_ref().map(|s| s.last_published_seq).unwrap_or(0);

        // Get unpublished messages
        let messages = self
            .db
            .get_unpublished_messages(session_id, last_seq, batch_size)?;
        if messages.is_empty() {
            return Ok(0);
        }

        let max_seq = messages.iter().map(|m| m.seq).max().unwrap_or(0);

        // Queue and flush
        let sent = self.runtime.block_on(async {
            self.inner.queue(&messages).await?;
            self.inner.flush_all().await
        })?;

        // Update publish state if we sent anything
        if sent > 0 {
            let now = Utc::now();
            let new_state = CollectorPublishState {
                session_id: session_id.to_string(),
                last_published_seq: max_seq as i64,
                last_published_at: Some(now),
                status: "active".to_string(),
                error_message: None,
                created_at: state.map(|s| s.created_at).unwrap_or(now),
                updated_at: now,
            };
            self.db.upsert_collector_publish_state(&new_state)?;
        }

        Ok(sent)
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
                    // Mark as failed but continue with other sessions
                    let _ = self
                        .db
                        .mark_publish_failed(&state.session_id, &e.to_string());
                }
            }
        }

        Ok(total_sent)
    }

    /// Queue messages for publishing with sequence tracking
    ///
    /// This method updates publish state after successful sends.
    pub fn queue_with_tracking(&mut self, messages: &[Message]) -> Result<usize> {
        if messages.is_empty() {
            return Ok(0);
        }

        // Track max seq per session
        let mut session_max_seqs: HashMap<String, i32> = HashMap::new();
        for msg in messages {
            let entry = session_max_seqs.entry(msg.session_id.clone()).or_insert(0);
            *entry = (*entry).max(msg.seq);
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
