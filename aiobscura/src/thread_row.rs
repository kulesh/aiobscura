//! Thread row data for TUI display.

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
        match self.last_activity {
            Some(ts) => {
                let now = Utc::now();
                let duration = now.signed_duration_since(ts);

                if duration.num_seconds() < 0 {
                    "just now".to_string()
                } else if duration.num_seconds() < 60 {
                    format!("{}s ago", duration.num_seconds())
                } else if duration.num_minutes() < 60 {
                    format!("{}m ago", duration.num_minutes())
                } else if duration.num_hours() < 24 {
                    format!("{}h ago", duration.num_hours())
                } else if duration.num_days() < 7 {
                    format!("{}d ago", duration.num_days())
                } else {
                    ts.format("%b %d").to_string()
                }
            }
            None => "—".to_string(),
        }
    }
}
