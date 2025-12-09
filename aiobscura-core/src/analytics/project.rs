//! Project-level analytics for TUI display.
//!
//! Provides aggregate statistics and insights at the project level,
//! designed for the Project view in the TUI.

use chrono::{DateTime, Utc};

use crate::db::{FileStats, ToolStats};

/// Row for project list display (lightweight, for table view).
#[derive(Debug, Clone)]
pub struct ProjectRow {
    /// Project ID
    pub id: String,
    /// Human-readable project name
    pub name: String,
    /// Project path (shortened for display)
    pub path: String,
    /// Number of sessions in this project
    pub session_count: i64,
    /// When the project was last active
    pub last_activity: Option<DateTime<Utc>>,
    /// Total tokens used (in + out)
    pub total_tokens: i64,
}

/// Detailed statistics for a single project.
#[derive(Debug, Clone)]
pub struct ProjectStats {
    /// Project ID
    pub id: String,
    /// Human-readable project name
    pub name: String,
    /// Full project path
    pub path: String,

    // Activity summary
    /// Number of sessions
    pub session_count: i64,
    /// Number of threads (main + agent)
    pub thread_count: i64,
    /// Number of messages
    pub message_count: i64,
    /// Total duration in seconds
    pub total_duration_secs: i64,

    // Token usage
    /// Total input tokens
    pub tokens_in: i64,
    /// Total output tokens
    pub tokens_out: i64,

    // Work patterns
    /// Tool usage statistics
    pub tool_stats: ToolStats,
    /// File modification statistics
    pub file_stats: FileStats,
    /// Number of agent threads spawned
    pub agents_spawned: i64,
    /// Number of plans created
    pub plans_created: i64,

    // Time patterns
    /// Hourly activity distribution (24 hours)
    pub hourly_distribution: [i64; 24],
    /// Daily activity distribution (7 days, 0=Sunday)
    pub daily_distribution: [i64; 7],

    // Timeline
    /// First session timestamp
    pub first_session: Option<DateTime<Utc>>,
    /// Last activity timestamp
    pub last_activity: Option<DateTime<Utc>>,

    // Assistant breakdown
    /// Sessions by assistant type
    pub sessions_by_assistant: Vec<(String, i64)>,
}

impl ProjectStats {
    /// Returns the total token count (in + out).
    pub fn total_tokens(&self) -> i64 {
        self.tokens_in + self.tokens_out
    }

    /// Returns the peak activity hour (0-23).
    pub fn peak_hour(&self) -> usize {
        self.hourly_distribution
            .iter()
            .enumerate()
            .max_by_key(|(_, &count)| count)
            .map(|(hour, _)| hour)
            .unwrap_or(0)
    }

    /// Returns a formatted duration string (e.g., "47h 23m").
    pub fn formatted_duration(&self) -> String {
        // Clamp to 0 in case of negative values from timestamp issues
        let secs = self.total_duration_secs.max(0);
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        if hours > 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}m", minutes)
        }
    }
}
