//! Publisher for batching and sending events to Catsyphon
//!
//! The Publisher manages event batching and provides both sync and async
//! interfaces for sending events to a Catsyphon server.

use std::collections::HashMap;
use std::time::Instant;

use crate::config::CollectorConfig;
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
                    last_flush: Instant::now(),
                });

            // Convert messages to events
            for msg in msgs {
                buffer.events.push(CollectorEvent::from_message(msg));
            }

            // Flush if batch size reached
            if buffer.events.len() >= batch_size {
                total_sent += self.flush_session(&session_id).await?;
            }
        }

        Ok(total_sent)
    }

    /// Flush all pending events for a session
    async fn flush_session(&mut self, session_id: &str) -> Result<usize> {
        let buffer = match self.buffers.get_mut(session_id) {
            Some(b) if !b.events.is_empty() => b,
            _ => return Ok(0),
        };

        let events: Vec<CollectorEvent> = buffer.events.drain(..).collect();
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
                    "Published events to Catsyphon"
                );
                Ok(response.accepted)
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
                Ok(0)
            }
        }
    }

    /// Flush all pending events across all sessions
    pub async fn flush_all(&mut self) -> Result<usize> {
        let session_ids: Vec<String> = self.buffers.keys().cloned().collect();
        let mut total_sent = 0;

        for session_id in session_ids {
            total_sent += self.flush_session(&session_id).await?;
        }

        Ok(total_sent)
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
            .map_err(|e| crate::error::Error::Collector(format!("failed to create runtime: {}", e)))?;

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
