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

// Existing exports
pub use dashboard::DashboardStats;
pub use personality::Personality;
pub use project::{ProjectRow, ProjectStats};
pub use wrapped::{
    generate_wrapped, MarathonSession, ProjectRanking, StreakStats, TimePatterns, ToolRankings,
    TotalStats, TrendComparison, WrappedConfig, WrappedPeriod, WrappedStats,
};
