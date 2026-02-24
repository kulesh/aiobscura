//! HTTP client for Catsyphon Collector Events API
//!
//! This client implements the Catsyphon collector protocol for pushing
//! events from aiobscura to a central Catsyphon server.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::config::CollectorConfig;
use crate::error::{Error, Result};

use super::events::{CollectorEvent, EventBatch};

/// Response from POST /collectors/events
#[derive(Debug, Deserialize)]
pub struct EventsResponse {
    /// Number of events accepted
    pub accepted: usize,
    /// Number of events rejected (duplicates, validation errors)
    #[serde(default)]
    pub rejected: usize,
    /// Session status after ingestion
    #[serde(default)]
    pub session_status: Option<String>,
}

/// Response from GET /collectors/sessions/{session_id}
#[derive(Debug, Deserialize)]
pub struct SessionStatus {
    /// Session ID
    pub session_id: String,
    /// Last received sequence number
    pub last_sequence: i32,
    /// Total events received
    pub event_count: i64,
    /// Session status (active, completed)
    pub status: String,
}

/// HTTP client for Catsyphon Collector API
pub struct CollectorClient {
    config: CollectorConfig,
    http_client: reqwest::Client,
    base_url: String,
}

impl CollectorClient {
    /// Create a new collector client from configuration
    ///
    /// Returns an error if the configuration is invalid or missing required fields.
    pub fn new(config: CollectorConfig) -> Result<Self> {
        config.validate()?;

        let base_url = config
            .server_url
            .clone()
            .ok_or_else(|| Error::Config("collector.server_url is required".to_string()))?
            .trim_end_matches('/')
            .to_string();

        // Build default headers
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // Add authorization header
        if let Some(api_key) = &config.api_key {
            let auth_value = format!("Bearer {}", api_key);
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&auth_value)
                    .map_err(|e| Error::Config(format!("invalid api_key: {}", e)))?,
            );
        }

        // Add collector ID header
        if let Some(collector_id) = &config.collector_id {
            headers.insert(
                "X-Collector-ID",
                HeaderValue::from_str(collector_id)
                    .map_err(|e| Error::Config(format!("invalid collector_id: {}", e)))?,
            );
        }

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .default_headers(headers)
            .build()
            .map_err(|e| Error::Config(format!("failed to create HTTP client: {}", e)))?;

        Ok(Self {
            config,
            http_client,
            base_url,
        })
    }

    /// Send a batch of events for a session
    ///
    /// Returns the number of events accepted and rejected.
    pub async fn send_events(&self, batch: &EventBatch) -> Result<EventsResponse> {
        let url = format!("{}/collectors/events", self.base_url);

        let request_body = SendEventsRequest {
            session_id: &batch.session_id,
            events: &batch.events,
        };

        let response = self
            .http_client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| Error::Collector(format!("HTTP request failed: {}", e)))?;

        let status = response.status();

        if status.is_success() {
            let result: EventsResponse = response
                .json()
                .await
                .map_err(|e| Error::Collector(format!("failed to parse response: {}", e)))?;
            Ok(result)
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            Err(Error::Collector(format!(
                "API error ({}): {}",
                status, error_text
            )))
        }
    }

    /// Ensure a remote session exists by sending a `session_start` event if needed.
    ///
    /// Returns true if a `session_start` event was sent, false if the session already existed.
    pub async fn ensure_session_started(
        &self,
        session_id: &str,
        session_start: CollectorEvent,
    ) -> Result<bool> {
        if self.get_session_status(session_id).await?.is_some() {
            return Ok(false);
        }

        let batch = EventBatch {
            session_id: session_id.to_string(),
            events: vec![session_start],
        };
        self.send_events_with_retry(&batch).await?;
        Ok(true)
    }

    /// Mark a session as completed on the server.
    ///
    /// Returns true if the completion endpoint succeeded, false if the session did not exist.
    pub async fn complete_session(
        &self,
        session_id: &str,
        outcome: &str,
        summary: Option<&str>,
        event_count: Option<i64>,
    ) -> Result<bool> {
        let url = format!(
            "{}/collectors/sessions/{}/complete",
            self.base_url,
            urlencoding::encode(session_id)
        );

        let request = SessionCompleteRequest {
            event_count,
            outcome,
            summary,
        };

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::Collector(format!("HTTP request failed: {}", e)))?;

        let status = response.status();

        if status.is_success() {
            let _: serde_json::Value = response
                .json()
                .await
                .map_err(|e| Error::Collector(format!("failed to parse response: {}", e)))?;
            Ok(true)
        } else if status == reqwest::StatusCode::NOT_FOUND {
            Ok(false)
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            Err(Error::Collector(format!(
                "API error ({}): {}",
                status, error_text
            )))
        }
    }

    /// Send events with retry logic
    ///
    /// Retries transient failures (5xx, timeouts) with exponential backoff.
    pub async fn send_events_with_retry(&self, batch: &EventBatch) -> Result<EventsResponse> {
        let mut last_error = None;
        let mut delay = Duration::from_millis(500);

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                tracing::debug!(
                    "Retrying send_events (attempt {}/{}), waiting {:?}",
                    attempt + 1,
                    self.config.max_retries + 1,
                    delay
                );
                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, Duration::from_secs(30));
            }

            match self.send_events(batch).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    // Check if error is retryable
                    if is_retryable_error(&e) {
                        tracing::warn!("Transient error sending events: {}", e);
                        last_error = Some(e);
                        continue;
                    } else {
                        // Non-retryable error, fail immediately
                        return Err(e);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| Error::Collector("max retries exceeded".to_string())))
    }

    /// Get session status (for resumption after failures)
    ///
    /// Returns None if the session doesn't exist on the server.
    pub async fn get_session_status(&self, session_id: &str) -> Result<Option<SessionStatus>> {
        let url = format!(
            "{}/collectors/sessions/{}",
            self.base_url,
            urlencoding::encode(session_id)
        );

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Collector(format!("HTTP request failed: {}", e)))?;

        let status = response.status();

        if status.is_success() {
            let result: SessionStatus = response
                .json()
                .await
                .map_err(|e| Error::Collector(format!("failed to parse response: {}", e)))?;
            Ok(Some(result))
        } else if status == reqwest::StatusCode::NOT_FOUND {
            Ok(None)
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            Err(Error::Collector(format!(
                "API error ({}): {}",
                status, error_text
            )))
        }
    }

    /// Check if the collector can connect to the server
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);

        match self.http_client.get(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    /// Get the configured batch size
    pub fn batch_size(&self) -> usize {
        self.config.batch_size
    }

    /// Get the configured flush interval
    pub fn flush_interval(&self) -> Duration {
        Duration::from_secs(self.config.flush_interval_secs)
    }
}

/// Request body for POST /collectors/events
#[derive(Serialize)]
struct SendEventsRequest<'a> {
    session_id: &'a str,
    events: &'a [CollectorEvent],
}

/// Request body for POST /collectors/sessions/{session_id}/complete
#[derive(Serialize)]
struct SessionCompleteRequest<'a> {
    event_count: Option<i64>,
    outcome: &'a str,
    summary: Option<&'a str>,
}

/// Check if an error is retryable (transient)
fn is_retryable_error(error: &Error) -> bool {
    match error {
        Error::Collector(msg) => {
            // Retry on 5xx errors
            msg.contains("50") && (msg.contains("API error") || msg.contains("HTTP"))
                // Retry on network/timeout errors
                || msg.contains("timeout")
                || msg.contains("connection")
                || msg.contains("request failed")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_requires_valid_config() {
        let config = CollectorConfig::default();
        assert!(CollectorClient::new(config).is_err());
    }

    #[test]
    fn test_client_with_valid_config() {
        let config = CollectorConfig {
            enabled: true,
            server_url: Some("https://catsyphon.example.com".to_string()),
            collector_id: Some("test-id".to_string()),
            api_key: Some("cs_live_test".to_string()),
            ..Default::default()
        };
        assert!(CollectorClient::new(config).is_ok());
    }

    #[test]
    fn test_is_retryable_error() {
        assert!(is_retryable_error(&Error::Collector(
            "API error (500): internal error".to_string()
        )));
        assert!(is_retryable_error(&Error::Collector(
            "HTTP request failed: timeout".to_string()
        )));
        assert!(!is_retryable_error(&Error::Collector(
            "API error (400): bad request".to_string()
        )));
        assert!(!is_retryable_error(&Error::Collector(
            "API error (401): unauthorized".to_string()
        )));
    }
}
