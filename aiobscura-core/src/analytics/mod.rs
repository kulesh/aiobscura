//! Analytics module for aiobscura
//!
//! Provides aggregate statistics and insights including:
//! - Wrapped (year/month in review)
//! - Personality classification
//! - Usage trends
//! - Project-level analytics
//! - Dashboard statistics

pub mod dashboard;
pub mod personality;
pub mod project;
pub mod wrapped;

pub use dashboard::DashboardStats;
pub use personality::Personality;
pub use project::{ProjectRow, ProjectStats};
pub use wrapped::{
    generate_wrapped, MarathonSession, ProjectRanking, StreakStats, TimePatterns, ToolRankings,
    TotalStats, TrendComparison, WrappedConfig, WrappedPeriod, WrappedStats,
};
