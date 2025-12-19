//! Analytics module for aiobscura
//!
//! Provides aggregate statistics and insights including:
//! - Plugin-based analytics framework
//! - Wrapped (year/month in review)
//! - Personality classification
//! - Usage trends
//! - Project-level analytics
//! - Dashboard statistics
//!
//! ## Plugin Framework
//!
//! The analytics system uses a plugin architecture where each plugin:
//! - Consumes Layer 1 data (sessions, messages)
//! - Produces Layer 2 metrics stored in `plugin_metrics`
//! - Can be triggered on-demand (future: event-based, scheduled)
//!
//! See [`engine`] module for the core framework and [`plugins`] for built-in plugins.

pub mod dashboard;
pub mod engine;
pub mod personality;
pub mod plugins;
pub mod project;
pub mod wrapped;

// Engine exports
pub use engine::{
    AnalyticsContext, AnalyticsEngine, AnalyticsPlugin, AnalyticsTrigger, MetricOutput,
    PluginRunResult, PluginRunStatus, METRIC_VERSION,
};
pub use plugins::create_default_engine;
use crate::db::Database;
use crate::Result;

// Session analytics struct
use chrono::{DateTime, Utc};

/// Pre-computed analytics for a session.
///
/// Contains metrics from the `core.edit_churn` plugin that track
/// file modification patterns during a session.
#[derive(Debug, Clone)]
pub struct SessionAnalytics {
    /// Total number of Edit/Write tool calls
    pub edit_count: i64,
    /// Number of unique files modified
    pub unique_files: i64,
    /// Churn ratio: (edits - unique_files) / edits
    /// 0.0 = no churn (each file edited once)
    /// Higher values indicate more re-editing of same files
    pub churn_ratio: f64,
    /// Files edited 3+ times, sorted by edit count descending
    pub high_churn_files: Vec<String>,
    /// When these metrics were computed
    pub computed_at: DateTime<Utc>,
}

/// Pre-computed analytics for a thread.
///
/// Contains the same metrics as SessionAnalytics but scoped to a single thread.
/// Includes additional fields for more detailed thread-level analysis.
#[derive(Debug, Clone)]
pub struct ThreadAnalytics {
    /// Total number of Edit/Write tool calls in this thread
    pub edit_count: i64,
    /// Number of unique files modified in this thread
    pub unique_files: i64,
    /// Churn ratio: (edits - unique_files) / edits
    pub churn_ratio: f64,
    /// Files with edit counts above the statistical threshold
    pub high_churn_files: Vec<String>,
    /// Statistical threshold for high churn (median + 2*stddev)
    pub high_churn_threshold: f64,
    /// Files with burst edits (3+ edits within 2 minutes) and their burst counts
    pub burst_edit_files: std::collections::HashMap<String, i64>,
    /// Total number of burst edit incidents
    pub burst_edit_count: i64,
    /// Total lines changed (added + removed)
    pub lines_changed: i64,
    /// Percentage of files that required only one edit (first-try success rate)
    pub first_try_rate: f64,
    /// When these metrics were computed
    pub computed_at: DateTime<Utc>,
}

// Existing exports
pub use dashboard::DashboardStats;
pub use personality::Personality;
pub use project::{ProjectRow, ProjectStats};
pub use wrapped::{
    generate_wrapped, MarathonSession, ProjectRanking, StreakStats, TimePatterns, ToolRankings,
    TotalStats, TrendComparison, WrappedConfig, WrappedPeriod, WrappedStats,
};

/// Ensure session analytics using the default analytics engine.
pub fn ensure_session_analytics(session_id: &str, db: &Database) -> Result<SessionAnalytics> {
    let engine = create_default_engine();
    engine.ensure_session_analytics(session_id, db)
}

/// Ensure thread analytics using the default analytics engine.
pub fn ensure_thread_analytics(thread_id: &str, db: &Database) -> Result<ThreadAnalytics> {
    let engine = create_default_engine();
    engine.ensure_thread_analytics(thread_id, db)
}
