//! Analytics module for aiobscura
//!
//! Provides aggregate statistics and insights including:
//! - Wrapped (year/month in review)
//! - Personality classification
//! - Usage trends

pub mod personality;
pub mod wrapped;

pub use personality::Personality;
pub use wrapped::{
    generate_wrapped, MarathonSession, ProjectRanking, StreakStats, TimePatterns, ToolRankings,
    TotalStats, TrendComparison, WrappedConfig, WrappedPeriod, WrappedStats,
};
