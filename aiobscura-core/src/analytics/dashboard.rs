//! Dashboard analytics for TUI startup display.
//!
//! Provides aggregate statistics, activity heatmap, and streak data
//! for the dashboard view in the TUI.

/// Dashboard statistics for the Projects view header.
#[derive(Debug, Clone, Default)]
pub struct DashboardStats {
    // Totals
    /// Total number of projects
    pub project_count: i64,
    /// Total number of sessions across all projects
    pub session_count: i64,
    /// Total tokens used (in + out)
    pub total_tokens: i64,
    /// Total duration in seconds across all sessions
    pub total_duration_secs: i64,

    // Activity heatmap (last 28 days, index 0 = oldest, 27 = today)
    /// Message/event count per day for the last 28 days
    pub daily_activity: [i64; 28],

    // Streaks
    /// Current consecutive days with activity
    pub current_streak: i64,
    /// Longest streak ever
    pub longest_streak: i64,

    // Patterns
    /// Peak activity hour (0-23)
    pub peak_hour: u8,
    /// Busiest day of week (0=Sunday, 1=Monday, ..., 6=Saturday)
    pub busiest_day: u8,
}

impl DashboardStats {
    /// Calculate streaks from daily activity data.
    /// Returns (current_streak, longest_streak).
    pub fn calculate_streaks(daily_activity: &[i64; 28]) -> (i64, i64) {
        let mut current_streak = 0i64;
        let mut longest_streak = 0i64;
        let mut streak = 0i64;

        // Iterate from oldest (0) to newest (27)
        for &count in daily_activity.iter() {
            if count > 0 {
                streak += 1;
                longest_streak = longest_streak.max(streak);
            } else {
                streak = 0;
            }
        }

        // Current streak is the streak that includes today (index 27)
        // Count backwards from today
        for &count in daily_activity.iter().rev() {
            if count > 0 {
                current_streak += 1;
            } else {
                break;
            }
        }

        (current_streak, longest_streak)
    }

    /// Format the peak hour for display (e.g., "2-3pm").
    pub fn format_peak_hour(&self) -> String {
        let hour = self.peak_hour as u32;
        let next_hour = (hour + 1) % 24;

        let format_hour = |h: u32| -> String {
            match h {
                0 => "12am".to_string(),
                1..=11 => format!("{}am", h),
                12 => "12pm".to_string(),
                13..=23 => format!("{}pm", h - 12),
                _ => format!("{}h", h),
            }
        };

        format!("{}-{}", format_hour(hour), format_hour(next_hour))
    }

    /// Format the busiest day for display.
    pub fn format_busiest_day(&self) -> &'static str {
        match self.busiest_day {
            0 => "Sunday",
            1 => "Monday",
            2 => "Tuesday",
            3 => "Wednesday",
            4 => "Thursday",
            5 => "Friday",
            6 => "Saturday",
            _ => "Unknown",
        }
    }

    /// Format total duration as hours (e.g., "847h").
    pub fn format_duration(&self) -> String {
        let hours = self.total_duration_secs / 3600;
        format!("{}h", hours)
    }
}
