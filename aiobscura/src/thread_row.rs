//! Row data structs for TUI display.

use aiobscura_core::format::format_relative_time_opt;
use aiobscura_core::ThreadType;
use chrono::{DateTime, Utc};

/// A denormalized thread row optimized for table display.
#[derive(Debug, Clone)]
pub struct ThreadRow {
    /// Thread ID
    pub id: String,
    /// Session ID (for fetching plans)
    pub session_id: String,
    /// Thread type (Main, Agent, Background)
    pub thread_type: ThreadType,
    /// Optional agent subtype (e.g., "plan", "explore")
    pub agent_subtype: Option<String>,
    /// Parent thread ID (None for main threads and orphan agents)
    #[allow(dead_code)]
    pub parent_thread_id: Option<String>,
    /// When the thread was last active
    pub last_activity: Option<DateTime<Utc>>,
    /// Human-readable project name
    pub project_name: String,
    /// Assistant display name (e.g., "Claude Code")
    pub assistant_name: String,
    /// Number of messages in this thread
    pub message_count: i64,
    /// Indentation level (0 for main/orphan, 1 for child agents)
    pub indent_level: usize,
    /// Whether this is the last child of its parent (for tree drawing)
    pub is_last_child: bool,
}

impl ThreadRow {
    /// Returns a truncated thread ID (first 8 characters).
    pub fn short_id(&self) -> &str {
        if self.id.len() > 8 {
            &self.id[..8]
        } else {
            &self.id
        }
    }

    /// Returns relative time since last activity (e.g., "2m ago", "1h ago").
    pub fn relative_time(&self) -> String {
        format_relative_time_opt(self.last_activity)
    }
}

/// A denormalized session row optimized for table display.
#[derive(Debug, Clone)]
pub struct SessionRow {
    /// Session ID
    pub id: String,
    /// When the session was last active
    pub last_activity: Option<DateTime<Utc>>,
    /// Session duration in seconds
    pub duration_secs: i64,
    /// Number of threads in this session
    pub thread_count: i64,
    /// Total message count across all threads
    pub message_count: i64,
    /// Model name (if known)
    pub model_name: Option<String>,
}

impl SessionRow {
    /// Returns a truncated session ID (first 8 characters).
    pub fn short_id(&self) -> &str {
        if self.id.len() > 8 {
            &self.id[..8]
        } else {
            &self.id
        }
    }

    /// Returns relative time since last activity (e.g., "2m ago", "1h ago").
    pub fn relative_time(&self) -> String {
        format_relative_time_opt(self.last_activity)
    }

    /// Returns formatted duration (e.g., "5m", "1h 30m", "2h").
    pub fn formatted_duration(&self) -> String {
        if self.duration_secs < 60 {
            format!("{}s", self.duration_secs)
        } else if self.duration_secs < 3600 {
            format!("{}m", self.duration_secs / 60)
        } else {
            let hours = self.duration_secs / 3600;
            let mins = (self.duration_secs % 3600) / 60;
            if mins > 0 {
                format!("{}h {}m", hours, mins)
            } else {
                format!("{}h", hours)
            }
        }
    }
}
