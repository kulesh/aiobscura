//! Formatting helpers shared across UIs.

use chrono::{DateTime, Utc};

/// Format a timestamp as relative time (e.g., "2m ago").
pub fn format_relative_time(ts: DateTime<Utc>) -> String {
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

/// Format an optional timestamp as relative time, or an em dash if missing.
pub fn format_relative_time_opt(ts: Option<DateTime<Utc>>) -> String {
    match ts {
        Some(ts) => format_relative_time(ts),
        None => "—".to_string(),
    }
}
