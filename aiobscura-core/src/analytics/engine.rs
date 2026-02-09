//! Analytics plugin framework
//!
//! Plugins consume Layer 1 data (sessions, messages) and produce
//! Layer 2 metrics stored in the `plugin_metrics` table.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     ANALYTICS ENGINE                            │
//! │                                                                 │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
//! │  │ Plugin A    │  │ Plugin B    │  │ Plugin C    │  ...        │
//! │  │ (edit_churn)│  │ (first_ord) │  │ (custom)    │             │
//! │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘             │
//! │         │                │                │                     │
//! │         ▼                ▼                ▼                     │
//! │  ┌─────────────────────────────────────────────────────────┐   │
//! │  │              AnalyticsEngine.run_plugin()               │   │
//! │  │  - Loads session + messages                             │   │
//! │  │  - Calls plugin.analyze_session()                       │   │
//! │  │  - Stores MetricOutputs in plugin_metrics               │   │
//! │  │  - Records PluginRunResult in plugin_runs               │   │
//! │  └─────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aiobscura_core::analytics::{AnalyticsEngine, create_default_engine};
//!
//! // Create engine with built-in plugins
//! let engine = create_default_engine();
//!
//! // Run all plugins on a session
//! let session = db.get_session("session-id")?.unwrap();
//! let messages = db.get_session_messages("session-id", 10000)?;
//! let results = engine.run_all(&session, &messages, &db);
//!
//! for result in results {
//!     println!("{}: {:?}", result.plugin_name, result.status);
//! }
//! ```

use crate::db::Database;
use crate::error::{Error, Result};
use crate::types::{Message, Session, Thread};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::time::Instant;

/// Current metric schema version.
///
/// Increment this when the metric format changes to trigger recomputation.
pub const METRIC_VERSION: i32 = 1;
const DEFAULT_PLUGIN_TIMEOUT_MS: u64 = 30_000;

// ============================================
// Trigger types (for future expansion)
// ============================================

/// Trigger conditions for when plugins should run.
///
/// For v1, only `OnDemand` is implemented. Future versions may support
/// automatic triggering based on event counts or inactivity.
#[derive(Debug, Clone)]
pub enum AnalyticsTrigger {
    /// Manual trigger via CLI or API
    OnDemand,
    /// After N new messages in a session (future)
    EventCount(usize),
    /// After session inactive for duration (future)
    Inactivity(std::time::Duration),
}

// ============================================
// Plugin context and outputs
// ============================================

/// Context provided to plugins during analysis.
///
/// Gives plugins read-only access to query additional data from the database
/// if needed (e.g., to look up related sessions or project info).
pub struct AnalyticsContext<'a> {
    /// Read-only database access for querying related data
    pub db: &'a Database,
}

/// Output from a plugin: a single metric value.
///
/// Plugins return a vector of these, which the engine stores in the
/// `plugin_metrics` table.
#[derive(Debug, Clone)]
pub struct MetricOutput {
    /// Type of entity: "session", "thread", "project", "global"
    pub entity_type: String,
    /// ID of the entity (session_id, thread_id, etc.), None for global metrics
    pub entity_id: Option<String>,
    /// Name of the metric (e.g., "edit_count", "churn_ratio")
    pub metric_name: String,
    /// Value (JSON for flexibility - can be number, string, object, array)
    pub metric_value: serde_json::Value,
}

impl MetricOutput {
    /// Create a session-level metric.
    pub fn session(session_id: &str, name: &str, value: serde_json::Value) -> Self {
        Self {
            entity_type: "session".to_string(),
            entity_id: Some(session_id.to_string()),
            metric_name: name.to_string(),
            metric_value: value,
        }
    }

    /// Create a thread-level metric.
    pub fn thread(thread_id: &str, name: &str, value: serde_json::Value) -> Self {
        Self {
            entity_type: "thread".to_string(),
            entity_id: Some(thread_id.to_string()),
            metric_name: name.to_string(),
            metric_value: value,
        }
    }

    /// Create a global metric (not tied to a specific entity).
    pub fn global(name: &str, value: serde_json::Value) -> Self {
        Self {
            entity_type: "global".to_string(),
            entity_id: None,
            metric_name: name.to_string(),
            metric_value: value,
        }
    }
}

// ============================================
// Plugin run results
// ============================================

/// Result of running a plugin on a session.
///
/// Stored in the `plugin_runs` table for observability and debugging.
#[derive(Debug, Clone)]
pub struct PluginRunResult {
    /// Name of the plugin that was run
    pub plugin_name: String,
    /// Session ID that was analyzed (None for global analysis)
    pub session_id: Option<String>,
    /// When the plugin run started
    pub started_at: DateTime<Utc>,
    /// How long the plugin took to run (milliseconds)
    pub duration_ms: i64,
    /// Whether the run succeeded or failed
    pub status: PluginRunStatus,
    /// Error message if the run failed
    pub error_message: Option<String>,
    /// Number of metrics produced
    pub metrics_produced: usize,
    /// Number of messages that were analyzed
    pub input_message_count: usize,
    /// Total tokens in the analyzed messages (for cost tracking)
    pub input_token_count: i64,
}

/// Status of a plugin run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginRunStatus {
    /// Plugin completed successfully
    Success,
    /// Plugin encountered an error
    Error,
    /// Plugin exceeded configured timeout
    Timeout,
}

impl PluginRunStatus {
    /// Convert to string for database storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginRunStatus::Success => "success",
            PluginRunStatus::Error => "error",
            PluginRunStatus::Timeout => "timeout",
        }
    }

    /// Parse status string from storage.
    pub fn from_storage(value: &str) -> Self {
        match value {
            "success" => PluginRunStatus::Success,
            "timeout" => PluginRunStatus::Timeout,
            _ => PluginRunStatus::Error,
        }
    }
}

// ============================================
// Plugin trait
// ============================================

/// Trait that all analytics plugins must implement.
///
/// Plugins are stateless analyzers that consume session data and produce
/// metrics. They should be:
/// - **Deterministic**: Same input produces same output
/// - **Idempotent**: Can be run multiple times safely (metrics are upserted)
/// - **Fast**: Should complete in reasonable time for large sessions
///
/// ## Example
///
/// ```rust,ignore
/// use aiobscura_core::analytics::{AnalyticsPlugin, AnalyticsContext, AnalyticsTrigger, MetricOutput};
///
/// pub struct MyPlugin;
///
/// impl AnalyticsPlugin for MyPlugin {
///     fn name(&self) -> &str { "custom.my_plugin" }
///     
///     fn triggers(&self) -> Vec<AnalyticsTrigger> {
///         vec![AnalyticsTrigger::OnDemand]
///     }
///     
///     fn analyze_session(
///         &self,
///         session: &Session,
///         messages: &[Message],
///         ctx: &AnalyticsContext,
///     ) -> Result<Vec<MetricOutput>> {
///         // Compute metrics from messages...
///         Ok(vec![
///             MetricOutput::session(&session.id, "my_metric", json!(42)),
///         ])
///     }
/// }
/// ```
pub trait AnalyticsPlugin: Send + Sync {
    /// Unique name for this plugin.
    ///
    /// Convention: `namespace.plugin_name` (e.g., "core.edit_churn", "llm.assessment")
    fn name(&self) -> &str;

    /// When this plugin should be triggered.
    ///
    /// For v1, only `OnDemand` is supported. The engine ignores other triggers
    /// but they can be specified for future compatibility.
    fn triggers(&self) -> Vec<AnalyticsTrigger>;

    /// Analyze a session and produce metrics.
    ///
    /// This is the main entry point for the plugin. It receives:
    /// - `session`: The session being analyzed
    /// - `messages`: All messages in the session (across all threads)
    /// - `ctx`: Context for querying additional data if needed
    ///
    /// Returns a vector of metrics to be stored in the database.
    fn analyze_session(
        &self,
        session: &Session,
        messages: &[Message],
        ctx: &AnalyticsContext,
    ) -> Result<Vec<MetricOutput>>;

    /// Whether this plugin supports thread-level analysis.
    ///
    /// Plugins that return `true` must implement `analyze_thread()`.
    /// Default implementation returns `false`.
    fn supports_thread_analysis(&self) -> bool {
        false
    }

    /// Analyze a single thread and produce metrics.
    ///
    /// This is called for each thread when thread-level analytics are requested.
    /// - `thread`: The thread being analyzed
    /// - `messages`: All messages in this thread
    /// - `ctx`: Context for querying additional data if needed
    ///
    /// Default implementation returns empty (not supported).
    fn analyze_thread(
        &self,
        _thread: &Thread,
        _messages: &[Message],
        _ctx: &AnalyticsContext,
    ) -> Result<Vec<MetricOutput>> {
        Ok(vec![])
    }
}

// ============================================
// Analytics engine
// ============================================

/// Engine that manages and runs analytics plugins.
///
/// The engine is responsible for:
/// - Registering plugins
/// - Running plugins on sessions
/// - Storing metrics in the database
/// - Recording plugin run results for observability
pub struct AnalyticsEngine {
    plugins: Vec<Box<dyn AnalyticsPlugin>>,
    default_timeout_ms: u64,
    plugin_timeouts_ms: HashMap<String, u64>,
}

impl AnalyticsEngine {
    /// Create a new empty engine.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            default_timeout_ms: DEFAULT_PLUGIN_TIMEOUT_MS,
            plugin_timeouts_ms: HashMap::new(),
        }
    }

    /// Register a plugin with the engine.
    pub fn register(&mut self, plugin: Box<dyn AnalyticsPlugin>) {
        tracing::info!(plugin = plugin.name(), "Registered analytics plugin");
        self.plugins.push(plugin);
    }

    /// Set default timeout (milliseconds) for plugin execution.
    pub fn set_default_timeout_ms(&mut self, timeout_ms: u64) {
        self.default_timeout_ms = timeout_ms.max(1);
    }

    /// Set per-plugin timeout overrides (milliseconds).
    pub fn set_plugin_timeouts_ms(&mut self, plugin_timeouts_ms: HashMap<String, u64>) {
        self.plugin_timeouts_ms = plugin_timeouts_ms
            .into_iter()
            .map(|(name, timeout)| (name, timeout.max(1)))
            .collect();
    }

    /// Get list of registered plugin names.
    pub fn plugin_names(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }

    /// Check if a plugin is registered.
    pub fn has_plugin(&self, name: &str) -> bool {
        self.plugins.iter().any(|p| p.name() == name)
    }

    fn timeout_for_plugin_ms(&self, plugin_name: &str) -> u64 {
        self.plugin_timeouts_ms
            .get(plugin_name)
            .copied()
            .unwrap_or(self.default_timeout_ms)
    }

    fn timeout_error(plugin_name: &str, duration_ms: i64, timeout_ms: u64) -> String {
        format!("plugin {plugin_name} exceeded timeout: {duration_ms}ms > {timeout_ms}ms")
    }

    fn record_plugin_run(db: &Database, result: &PluginRunResult) {
        if let Err(e) = db.insert_plugin_run(result) {
            tracing::warn!(error = %e, "Failed to record plugin run");
        }
    }

    /// Run a specific plugin on a session.
    ///
    /// This method:
    /// 1. Finds the plugin by name
    /// 2. Calls the plugin's `analyze_session` method
    /// 3. Stores the resulting metrics in the database
    /// 4. Records the plugin run for observability
    ///
    /// Returns the run result, which includes timing and status information.
    pub fn run_plugin(
        &self,
        plugin_name: &str,
        session: &Session,
        messages: &[Message],
        db: &Database,
    ) -> Result<PluginRunResult> {
        let plugin = self
            .plugins
            .iter()
            .find(|p| p.name() == plugin_name)
            .ok_or_else(|| Error::Config(format!("Plugin not found: {}", plugin_name)))?;

        let ctx = AnalyticsContext { db };
        let started_at = Utc::now();
        let start = Instant::now();
        let timeout_ms = self.timeout_for_plugin_ms(plugin.name());

        // Calculate input token count for observability
        let input_token_count: i64 = messages
            .iter()
            .map(|m| m.tokens_in.unwrap_or(0) as i64 + m.tokens_out.unwrap_or(0) as i64)
            .sum();

        tracing::debug!(
            plugin = plugin.name(),
            session_id = session.id,
            message_count = messages.len(),
            timeout_ms,
            "Running analytics plugin"
        );

        match plugin.analyze_session(session, messages, &ctx) {
            Ok(metrics) => {
                let duration_ms = start.elapsed().as_millis() as i64;

                if duration_ms as u64 > timeout_ms {
                    let error_msg = Self::timeout_error(plugin.name(), duration_ms, timeout_ms);
                    tracing::warn!(
                        plugin = plugin.name(),
                        session_id = session.id,
                        duration_ms,
                        timeout_ms,
                        "Plugin exceeded timeout; dropping computed metrics"
                    );

                    let result = PluginRunResult {
                        plugin_name: plugin.name().to_string(),
                        session_id: Some(session.id.clone()),
                        started_at,
                        duration_ms,
                        status: PluginRunStatus::Timeout,
                        error_message: Some(error_msg),
                        metrics_produced: 0,
                        input_message_count: messages.len(),
                        input_token_count,
                    };

                    Self::record_plugin_run(db, &result);
                    return Ok(result);
                }

                // Store metrics in database
                for metric in &metrics {
                    db.insert_plugin_metric(
                        plugin.name(),
                        &metric.entity_type,
                        metric.entity_id.as_deref(),
                        &metric.metric_name,
                        &metric.metric_value,
                        METRIC_VERSION,
                    )?;
                }

                let result = PluginRunResult {
                    plugin_name: plugin.name().to_string(),
                    session_id: Some(session.id.clone()),
                    started_at,
                    duration_ms,
                    status: PluginRunStatus::Success,
                    error_message: None,
                    metrics_produced: metrics.len(),
                    input_message_count: messages.len(),
                    input_token_count,
                };

                Self::record_plugin_run(db, &result);

                tracing::info!(
                    plugin = plugin.name(),
                    session_id = session.id,
                    metrics = metrics.len(),
                    duration_ms = duration_ms,
                    "Plugin completed successfully"
                );

                Ok(result)
            }
            Err(e) => {
                let duration_ms = start.elapsed().as_millis() as i64;
                let error_msg = e.to_string();

                tracing::error!(
                    plugin = plugin.name(),
                    session_id = session.id,
                    error = %e,
                    "Plugin failed"
                );

                let result = PluginRunResult {
                    plugin_name: plugin.name().to_string(),
                    session_id: Some(session.id.clone()),
                    started_at,
                    duration_ms,
                    status: PluginRunStatus::Error,
                    error_message: Some(error_msg),
                    metrics_produced: 0,
                    input_message_count: messages.len(),
                    input_token_count,
                };

                Self::record_plugin_run(db, &result);

                Ok(result)
            }
        }
    }

    /// Run all registered plugins on a session.
    ///
    /// Returns a vector of run results, one for each plugin.
    /// Failed plugins don't stop other plugins from running.
    pub fn run_all(
        &self,
        session: &Session,
        messages: &[Message],
        db: &Database,
    ) -> Vec<PluginRunResult> {
        self.plugins
            .iter()
            .filter_map(|p| self.run_plugin(p.name(), session, messages, db).ok())
            .collect()
    }

    /// Ensure session analytics are computed and up-to-date.
    ///
    /// This method:
    /// 1. Checks if analytics exist for the session
    /// 2. Checks if they're still fresh (computed after last message)
    /// 3. Recomputes if stale or missing
    /// 4. Returns the analytics
    ///
    /// Used by the TUI to show analytics inline with minimal latency.
    pub fn ensure_session_analytics(
        &self,
        session_id: &str,
        db: &Database,
    ) -> Result<crate::analytics::SessionAnalytics> {
        // Check if we have existing analytics
        if let Some(existing) = db.get_session_analytics(session_id)? {
            // Check freshness: is computed_at >= last message timestamp?
            if let Some(last_msg_ts) = db.get_session_last_message_ts(session_id)? {
                if existing.computed_at >= last_msg_ts {
                    // Analytics are fresh, return cached
                    tracing::debug!(
                        session_id,
                        computed_at = %existing.computed_at,
                        "Using cached session analytics"
                    );
                    return Ok(existing);
                }
                tracing::debug!(
                    session_id,
                    computed_at = %existing.computed_at,
                    last_msg_ts = %last_msg_ts,
                    "Session analytics are stale, recomputing"
                );
            } else {
                // No messages in session, return existing analytics
                return Ok(existing);
            }
        }

        // Need to compute analytics
        tracing::info!(session_id, "Computing session analytics");

        let session = db
            .get_session(session_id)?
            .ok_or_else(|| Error::Config(format!("Session not found: {}", session_id)))?;

        let messages = db.get_session_messages(session_id, 100_000)?;

        // Run the edit_churn plugin
        self.run_plugin("core.edit_churn", &session, &messages, db)?;

        // Fetch the newly computed analytics
        db.get_session_analytics(session_id)?
            .ok_or_else(|| Error::Config("Failed to compute session analytics".to_string()))
    }

    /// Ensure first-order session metrics are computed and up-to-date.
    ///
    /// This method mirrors `ensure_session_analytics`, but uses the
    /// `core.first_order` plugin and typed wrapper.
    pub fn ensure_first_order_metrics(
        &self,
        session_id: &str,
        db: &Database,
    ) -> Result<crate::analytics::FirstOrderSessionMetrics> {
        if let Some(existing) = db.get_session_first_order_metrics(session_id)? {
            if let Some(last_msg_ts) = db.get_session_last_message_ts(session_id)? {
                if existing.computed_at >= last_msg_ts {
                    tracing::debug!(
                        session_id,
                        computed_at = %existing.computed_at,
                        "Using cached first-order metrics"
                    );
                    return Ok(existing);
                }
                tracing::debug!(
                    session_id,
                    computed_at = %existing.computed_at,
                    last_msg_ts = %last_msg_ts,
                    "First-order metrics are stale, recomputing"
                );
            } else {
                return Ok(existing);
            }
        }

        tracing::info!(session_id, "Computing first-order metrics");

        let session = db
            .get_session(session_id)?
            .ok_or_else(|| Error::Config(format!("Session not found: {}", session_id)))?;

        let messages = db.get_session_messages(session_id, 100_000)?;

        self.run_plugin("core.first_order", &session, &messages, db)?;

        db.get_session_first_order_metrics(session_id)?
            .ok_or_else(|| Error::Config("Failed to compute first-order metrics".to_string()))
    }

    /// Run all registered plugins on all sessions in the database.
    ///
    /// This is useful for batch processing. Returns the total number of
    /// plugin runs and any errors encountered.
    pub fn run_all_sessions(&self, db: &Database) -> Result<(usize, Vec<String>)> {
        use crate::db::SessionFilter;

        let sessions = db.list_sessions(&SessionFilter::default())?;
        let mut total_runs = 0;
        let mut errors = Vec::new();

        for session in sessions {
            let messages = db.get_session_messages(&session.id, 100_000)?;
            let results = self.run_all(&session, &messages, db);

            for result in results {
                total_runs += 1;
                if let Some(error) = result.error_message {
                    errors.push(format!(
                        "{} on {}: {}",
                        result.plugin_name, session.id, error
                    ));
                }
            }
        }

        Ok((total_runs, errors))
    }

    /// Run a specific plugin on a thread.
    ///
    /// Similar to `run_plugin`, but calls `analyze_thread` instead.
    /// Only works for plugins that support thread-level analysis.
    pub fn run_thread_plugin(
        &self,
        plugin_name: &str,
        thread: &Thread,
        messages: &[Message],
        db: &Database,
    ) -> Result<PluginRunResult> {
        let plugin = self
            .plugins
            .iter()
            .find(|p| p.name() == plugin_name)
            .ok_or_else(|| Error::Config(format!("Plugin not found: {}", plugin_name)))?;

        if !plugin.supports_thread_analysis() {
            return Err(Error::Config(format!(
                "Plugin {} does not support thread analysis",
                plugin_name
            )));
        }

        let ctx = AnalyticsContext { db };
        let started_at = Utc::now();
        let start = Instant::now();
        let timeout_ms = self.timeout_for_plugin_ms(plugin.name());

        // Calculate input token count for observability
        let input_token_count: i64 = messages
            .iter()
            .map(|m| m.tokens_in.unwrap_or(0) as i64 + m.tokens_out.unwrap_or(0) as i64)
            .sum();

        tracing::debug!(
            plugin = plugin.name(),
            thread_id = thread.id,
            message_count = messages.len(),
            timeout_ms,
            "Running analytics plugin on thread"
        );

        match plugin.analyze_thread(thread, messages, &ctx) {
            Ok(metrics) => {
                let duration_ms = start.elapsed().as_millis() as i64;

                if duration_ms as u64 > timeout_ms {
                    let error_msg = Self::timeout_error(plugin.name(), duration_ms, timeout_ms);
                    tracing::warn!(
                        plugin = plugin.name(),
                        thread_id = thread.id,
                        duration_ms,
                        timeout_ms,
                        "Thread plugin exceeded timeout; dropping computed metrics"
                    );

                    let result = PluginRunResult {
                        plugin_name: plugin.name().to_string(),
                        session_id: Some(thread.session_id.clone()),
                        started_at,
                        duration_ms,
                        status: PluginRunStatus::Timeout,
                        error_message: Some(error_msg),
                        metrics_produced: 0,
                        input_message_count: messages.len(),
                        input_token_count,
                    };

                    Self::record_plugin_run(db, &result);
                    return Ok(result);
                }

                // Store metrics in database
                for metric in &metrics {
                    db.insert_plugin_metric(
                        plugin.name(),
                        &metric.entity_type,
                        metric.entity_id.as_deref(),
                        &metric.metric_name,
                        &metric.metric_value,
                        METRIC_VERSION,
                    )?;
                }

                let result = PluginRunResult {
                    plugin_name: plugin.name().to_string(),
                    session_id: Some(thread.session_id.clone()),
                    started_at,
                    duration_ms,
                    status: PluginRunStatus::Success,
                    error_message: None,
                    metrics_produced: metrics.len(),
                    input_message_count: messages.len(),
                    input_token_count,
                };

                Self::record_plugin_run(db, &result);

                tracing::info!(
                    plugin = plugin.name(),
                    thread_id = thread.id,
                    metrics = metrics.len(),
                    duration_ms = duration_ms,
                    "Plugin completed successfully on thread"
                );

                Ok(result)
            }
            Err(e) => {
                let duration_ms = start.elapsed().as_millis() as i64;
                let error_msg = e.to_string();

                tracing::error!(
                    plugin = plugin.name(),
                    thread_id = thread.id,
                    error = %e,
                    "Plugin failed on thread"
                );

                let result = PluginRunResult {
                    plugin_name: plugin.name().to_string(),
                    session_id: Some(thread.session_id.clone()),
                    started_at,
                    duration_ms,
                    status: PluginRunStatus::Error,
                    error_message: Some(error_msg),
                    metrics_produced: 0,
                    input_message_count: messages.len(),
                    input_token_count,
                };

                Self::record_plugin_run(db, &result);
                Ok(result)
            }
        }
    }

    /// Ensure thread analytics are computed and up-to-date.
    ///
    /// This method:
    /// 1. Checks if analytics exist for the thread
    /// 2. Checks if they're still fresh (computed after last message)
    /// 3. Recomputes if stale or missing
    /// 4. Returns the analytics
    ///
    /// Used by the TUI to show thread-level analytics.
    pub fn ensure_thread_analytics(
        &self,
        thread_id: &str,
        db: &Database,
    ) -> Result<crate::analytics::ThreadAnalytics> {
        // Check if we have existing analytics
        if let Some(existing) = db.get_thread_analytics(thread_id)? {
            // Check freshness: is computed_at >= last message timestamp?
            if let Some(last_msg_ts) = db.get_thread_last_activity(thread_id)? {
                if existing.computed_at >= last_msg_ts {
                    // Analytics are fresh, return cached
                    tracing::debug!(
                        thread_id,
                        computed_at = %existing.computed_at,
                        "Using cached thread analytics"
                    );
                    return Ok(existing);
                }
                tracing::debug!(
                    thread_id,
                    computed_at = %existing.computed_at,
                    last_msg_ts = %last_msg_ts,
                    "Thread analytics are stale, recomputing"
                );
            } else {
                // No messages in thread, return existing analytics
                return Ok(existing);
            }
        }

        // Need to compute analytics
        tracing::info!(thread_id, "Computing thread analytics");

        let thread = db
            .get_thread(thread_id)?
            .ok_or_else(|| Error::Config(format!("Thread not found: {}", thread_id)))?;

        let messages = db.get_thread_messages(thread_id, 100_000)?;

        // Run the edit_churn plugin on the thread
        self.run_thread_plugin("core.edit_churn", &thread, &messages, db)?;

        // Fetch the newly computed analytics
        db.get_thread_analytics(thread_id)?
            .ok_or_else(|| Error::Config("Failed to compute thread analytics".to_string()))
    }
}

impl Default for AnalyticsEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    struct TestPlugin {
        name: String,
        should_fail: bool,
    }

    impl TestPlugin {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                should_fail: false,
            }
        }

        #[allow(dead_code)]
        fn failing(name: &str) -> Self {
            Self {
                name: name.to_string(),
                should_fail: true,
            }
        }
    }

    impl AnalyticsPlugin for TestPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> Vec<AnalyticsTrigger> {
            vec![AnalyticsTrigger::OnDemand]
        }

        fn analyze_session(
            &self,
            session: &Session,
            _messages: &[Message],
            _ctx: &AnalyticsContext,
        ) -> Result<Vec<MetricOutput>> {
            if self.should_fail {
                return Err(Error::Config("Test failure".to_string()));
            }

            Ok(vec![MetricOutput::session(
                &session.id,
                "test_metric",
                serde_json::json!(42),
            )])
        }
    }

    struct SlowPlugin {
        name: String,
        sleep_ms: u64,
    }

    impl SlowPlugin {
        fn new(name: &str, sleep_ms: u64) -> Self {
            Self {
                name: name.to_string(),
                sleep_ms,
            }
        }
    }

    impl AnalyticsPlugin for SlowPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn triggers(&self) -> Vec<AnalyticsTrigger> {
            vec![AnalyticsTrigger::OnDemand]
        }

        fn analyze_session(
            &self,
            session: &Session,
            _messages: &[Message],
            _ctx: &AnalyticsContext,
        ) -> Result<Vec<MetricOutput>> {
            thread::sleep(Duration::from_millis(self.sleep_ms));
            Ok(vec![MetricOutput::session(
                &session.id,
                "slow_metric",
                serde_json::json!(1),
            )])
        }
    }

    fn test_session() -> Session {
        Session {
            id: "session-timeout".to_string(),
            assistant: crate::types::Assistant::Codex,
            backing_model_id: None,
            project_id: None,
            started_at: Utc::now(),
            last_activity_at: Some(Utc::now()),
            status: crate::types::SessionStatus::Active,
            source_file_path: "/tmp/session-timeout.jsonl".to_string(),
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn test_engine_registration() {
        let mut engine = AnalyticsEngine::new();
        assert!(engine.plugin_names().is_empty());

        engine.register(Box::new(TestPlugin::new("test.plugin1")));
        engine.register(Box::new(TestPlugin::new("test.plugin2")));

        assert_eq!(engine.plugin_names().len(), 2);
        assert!(engine.has_plugin("test.plugin1"));
        assert!(engine.has_plugin("test.plugin2"));
        assert!(!engine.has_plugin("test.nonexistent"));
    }

    #[test]
    fn test_metric_output_helpers() {
        let session = MetricOutput::session("sess-1", "count", serde_json::json!(10));
        assert_eq!(session.entity_type, "session");
        assert_eq!(session.entity_id, Some("sess-1".to_string()));
        assert_eq!(session.metric_name, "count");

        let thread = MetricOutput::thread("thread-1", "depth", serde_json::json!(5));
        assert_eq!(thread.entity_type, "thread");
        assert_eq!(thread.entity_id, Some("thread-1".to_string()));

        let global = MetricOutput::global("total", serde_json::json!(100));
        assert_eq!(global.entity_type, "global");
        assert_eq!(global.entity_id, None);
    }

    #[test]
    fn test_run_plugin_marks_timeout_and_drops_metrics() {
        let db = crate::db::Database::open_in_memory().expect("open in-memory db");
        db.migrate().expect("migrate schema");

        let mut engine = AnalyticsEngine::new();
        engine.set_default_timeout_ms(5);
        engine.register(Box::new(SlowPlugin::new("test.slow", 25)));

        let session = test_session();
        let result = engine
            .run_plugin("test.slow", &session, &[], &db)
            .expect("plugin run should return a timeout result");

        assert_eq!(result.status, PluginRunStatus::Timeout);
        assert_eq!(result.metrics_produced, 0);
        assert!(result
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("exceeded timeout"));

        let metrics = db
            .get_session_plugin_metrics(&session.id)
            .expect("read session metrics");
        assert!(
            metrics.is_empty(),
            "timed out run should not persist metrics"
        );

        let runs = db
            .get_plugin_runs("test.slow", 10)
            .expect("read plugin runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, PluginRunStatus::Timeout);
    }
}
