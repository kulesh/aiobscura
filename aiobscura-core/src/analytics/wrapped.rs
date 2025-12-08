//! Wrapped - Year/Month in Review
//!
//! Generates "Spotify Wrapped"-style summaries of AI assistant usage.

use chrono::{DateTime, Datelike, Local, Utc};

use super::Personality;

/// Time period for wrapped statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrappedPeriod {
    /// Full year (e.g., 2024)
    Year(i32),
    /// Specific month (year, month 1-12)
    Month(i32, u32),
}

impl WrappedPeriod {
    /// Get the start datetime for this period.
    pub fn start(&self) -> DateTime<Utc> {
        match self {
            WrappedPeriod::Year(year) => {
                chrono::NaiveDate::from_ymd_opt(*year, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }
            WrappedPeriod::Month(year, month) => {
                chrono::NaiveDate::from_ymd_opt(*year, *month, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }
        }
    }

    /// Get the end datetime for this period (exclusive).
    pub fn end(&self) -> DateTime<Utc> {
        match self {
            WrappedPeriod::Year(year) => {
                chrono::NaiveDate::from_ymd_opt(*year + 1, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }
            WrappedPeriod::Month(year, month) => {
                let (next_year, next_month) = if *month == 12 {
                    (*year + 1, 1)
                } else {
                    (*year, *month + 1)
                };
                chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }
        }
    }

    /// Get the previous period for trend comparison.
    pub fn previous(&self) -> Self {
        match self {
            WrappedPeriod::Year(year) => WrappedPeriod::Year(*year - 1),
            WrappedPeriod::Month(year, month) => {
                if *month == 1 {
                    WrappedPeriod::Month(*year - 1, 12)
                } else {
                    WrappedPeriod::Month(*year, *month - 1)
                }
            }
        }
    }

    /// Get display name for this period.
    pub fn display_name(&self) -> String {
        match self {
            WrappedPeriod::Year(year) => format!("{}", year),
            WrappedPeriod::Month(year, month) => {
                let month_name = match month {
                    1 => "January",
                    2 => "February",
                    3 => "March",
                    4 => "April",
                    5 => "May",
                    6 => "June",
                    7 => "July",
                    8 => "August",
                    9 => "September",
                    10 => "October",
                    11 => "November",
                    12 => "December",
                    _ => "Unknown",
                };
                format!("{} {}", month_name, year)
            }
        }
    }

    /// Create a period for the current year.
    pub fn current_year() -> Self {
        WrappedPeriod::Year(Utc::now().year())
    }

    /// Create a period for the current month.
    pub fn current_month() -> Self {
        let now = Utc::now();
        WrappedPeriod::Month(now.year(), now.month())
    }
}

/// Configuration for wrapped generation.
#[derive(Debug, Clone)]
pub struct WrappedConfig {
    /// Include fun personality and witty descriptions
    pub fun_mode: bool,
    /// Include trend comparison with previous period
    pub include_trends: bool,
    /// Number of top tools to include
    pub top_tools_count: usize,
    /// Number of top projects to include
    pub top_projects_count: usize,
}

impl Default for WrappedConfig {
    fn default() -> Self {
        Self {
            fun_mode: true,
            include_trends: true,
            top_tools_count: 5,
            top_projects_count: 5,
        }
    }
}

impl WrappedConfig {
    /// Create a "serious" config without fun elements.
    pub fn serious() -> Self {
        Self {
            fun_mode: false,
            ..Default::default()
        }
    }
}

/// Complete wrapped statistics for a period.
#[derive(Debug, Clone)]
pub struct WrappedStats {
    /// The time period these stats cover
    pub period: WrappedPeriod,
    /// Aggregate totals
    pub totals: TotalStats,
    /// Tool usage rankings
    pub tools: ToolRankings,
    /// Time-based patterns
    pub time_patterns: TimePatterns,
    /// Project activity rankings
    pub projects: Vec<ProjectRanking>,
    /// Coding personality (None if serious mode)
    pub personality: Option<Personality>,
    /// Streak statistics
    pub streaks: StreakStats,
    /// Comparison with previous period (None if not requested or no data)
    pub trends: Option<TrendComparison>,
}

/// Aggregate totals for a period.
#[derive(Debug, Clone, Default)]
pub struct TotalStats {
    /// Number of sessions
    pub sessions: i64,
    /// Total time spent (seconds)
    pub total_duration_secs: i64,
    /// Total input tokens
    pub tokens_in: i64,
    /// Total output tokens
    pub tokens_out: i64,
    /// Total tool calls
    pub tool_calls: i64,
    /// Number of plans created/used
    pub plans: i64,
    /// Number of agents spawned
    pub agents_spawned: i64,
    /// Number of unique files modified
    pub files_modified: i64,
    /// Number of unique projects worked on
    pub unique_projects: i64,
}

impl TotalStats {
    /// Total tokens (in + out).
    pub fn total_tokens(&self) -> i64 {
        self.tokens_in + self.tokens_out
    }

    /// Format total tokens for display (e.g., "14.2M").
    pub fn tokens_display(&self) -> String {
        let total = self.total_tokens();
        if total >= 1_000_000 {
            format!("{:.1}M", total as f64 / 1_000_000.0)
        } else if total >= 1_000 {
            format!("{:.1}K", total as f64 / 1_000.0)
        } else {
            total.to_string()
        }
    }

    /// Format duration for display (e.g., "312h 45m").
    pub fn duration_display(&self) -> String {
        let hours = self.total_duration_secs / 3600;
        let mins = (self.total_duration_secs % 3600) / 60;
        if hours > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}m", mins)
        }
    }
}

/// Tool usage rankings with optional witty descriptions.
#[derive(Debug, Clone, Default)]
pub struct ToolRankings {
    /// Top tools: (name, count, optional witty description)
    pub top_tools: Vec<(String, i64, Option<&'static str>)>,
}

impl ToolRankings {
    /// Get witty description for a tool (fun mode).
    pub fn witty_description(tool_name: &str) -> &'static str {
        match tool_name.to_lowercase().as_str() {
            "read" => "Your trusty magnifying glass",
            "edit" => "The surgeon's scalpel",
            "write" => "The creator's pen",
            "bash" => "Terminal warrior",
            "grep" => "Finding needles in haystacks",
            "glob" => "Pattern hunter",
            "task" => "Delegation master",
            "webfetch" => "Web explorer",
            "websearch" => "Knowledge seeker",
            "todowrite" => "The organizer",
            "multiedit" => "Bulk editor extraordinaire",
            "notebookedit" => "Jupyter juggler",
            _ => "A trusty companion",
        }
    }
}

/// Time-based usage patterns.
#[derive(Debug, Clone)]
pub struct TimePatterns {
    /// Activity count by hour (0-23)
    pub hourly_distribution: [i64; 24],
    /// Activity count by day of week (0=Sunday, 6=Saturday)
    pub daily_distribution: [i64; 7],
    /// Peak hour (0-23)
    pub peak_hour: u8,
    /// Busiest day (0=Sunday)
    pub busiest_day: u8,
    /// Quietest day (0=Sunday)
    pub quietest_day: u8,
    /// Longest single session
    pub marathon_session: Option<MarathonSession>,
}

impl Default for TimePatterns {
    fn default() -> Self {
        Self {
            hourly_distribution: [0; 24],
            daily_distribution: [0; 7],
            peak_hour: 0,
            busiest_day: 0,
            quietest_day: 0,
            marathon_session: None,
        }
    }
}

impl TimePatterns {
    /// Get day name from index.
    pub fn day_name(day: u8) -> &'static str {
        match day {
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

    /// Get hour display (e.g., "10am-11am").
    pub fn hour_display(hour: u8) -> String {
        let h = hour % 12;
        let h = if h == 0 { 12 } else { h };
        let period = if hour < 12 { "am" } else { "pm" };
        let next_h = (hour + 1) % 12;
        let next_h = if next_h == 0 { 12 } else { next_h };
        let next_period = if (hour + 1) % 24 < 12 { "am" } else { "pm" };
        format!("{}{}–{}{}", h, period, next_h, next_period)
    }

    /// Check if user is a night owl (most activity 10pm-4am).
    pub fn is_night_owl(&self) -> bool {
        let night_activity: i64 = self.hourly_distribution[22..24].iter().sum::<i64>()
            + self.hourly_distribution[0..4].iter().sum::<i64>();
        let total: i64 = self.hourly_distribution.iter().sum();
        if total == 0 {
            return false;
        }
        (night_activity as f64 / total as f64) > 0.3
    }

    /// Check if user is an early bird (most activity 5am-9am).
    pub fn is_early_bird(&self) -> bool {
        let morning_activity: i64 = self.hourly_distribution[5..9].iter().sum();
        let total: i64 = self.hourly_distribution.iter().sum();
        if total == 0 {
            return false;
        }
        (morning_activity as f64 / total as f64) > 0.3
    }
}

/// Details about the longest session.
#[derive(Debug, Clone)]
pub struct MarathonSession {
    /// Session ID
    pub session_id: String,
    /// Duration in seconds
    pub duration_secs: i64,
    /// When it occurred
    pub date: DateTime<Utc>,
    /// Project name (if available)
    pub project_name: Option<String>,
    /// Tool calls in this session
    pub tool_calls: i64,
    /// Tokens used in this session
    pub tokens: i64,
    /// Files modified in this session
    pub files_modified: i64,
}

impl MarathonSession {
    /// Format duration for display.
    pub fn duration_display(&self) -> String {
        let hours = self.duration_secs / 3600;
        let mins = (self.duration_secs % 3600) / 60;
        if hours > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}m", mins)
        }
    }

    /// Format date for display in local timezone.
    pub fn date_display(&self) -> String {
        self.date
            .with_timezone(&Local)
            .format("%b %d")
            .to_string()
    }
}

/// Project activity ranking.
#[derive(Debug, Clone)]
pub struct ProjectRanking {
    /// Project name
    pub name: String,
    /// Number of sessions
    pub sessions: i64,
    /// Total tokens
    pub tokens: i64,
    /// Total duration in seconds
    pub duration_secs: i64,
    /// Files modified
    pub files_modified: i64,
    /// When work on this project started in the period
    pub first_session: Option<DateTime<Utc>>,
}

/// Streak statistics.
#[derive(Debug, Clone, Default)]
pub struct StreakStats {
    /// Current streak (consecutive days with activity)
    pub current_streak_days: i64,
    /// Longest streak in the period
    pub longest_streak_days: i64,
    /// When the longest streak started
    pub longest_streak_start: Option<DateTime<Utc>>,
    /// When the longest streak ended
    pub longest_streak_end: Option<DateTime<Utc>>,
    /// Total days with activity
    pub active_days: i64,
    /// Total days in period
    pub total_days: i64,
}

impl StreakStats {
    /// Calculate activity percentage.
    pub fn activity_percentage(&self) -> f64 {
        if self.total_days == 0 {
            0.0
        } else {
            (self.active_days as f64 / self.total_days as f64) * 100.0
        }
    }
}

/// Trend comparison with previous period.
#[derive(Debug, Clone, Default)]
pub struct TrendComparison {
    /// Sessions change percentage
    pub sessions_delta_pct: f64,
    /// Tokens change percentage
    pub tokens_delta_pct: f64,
    /// Tool calls change percentage
    pub tools_delta_pct: f64,
    /// Duration change percentage
    pub duration_delta_pct: f64,
    /// Previous period totals (for context)
    pub previous_totals: TotalStats,
}

impl TrendComparison {
    /// Calculate delta percentage between two values.
    pub fn calc_delta(current: i64, previous: i64) -> f64 {
        if previous == 0 {
            if current == 0 {
                0.0
            } else {
                100.0 // Infinite growth shown as 100%
            }
        } else {
            ((current - previous) as f64 / previous as f64) * 100.0
        }
    }

    /// Format delta for display (e.g., "+23%" or "-15%").
    pub fn format_delta(delta: f64) -> String {
        if delta >= 0.0 {
            format!("+{:.0}%", delta)
        } else {
            format!("{:.0}%", delta)
        }
    }
}

/// Generate wrapped statistics for a period.
pub fn generate_wrapped(
    db: &crate::Database,
    period: WrappedPeriod,
    config: &WrappedConfig,
) -> crate::Result<WrappedStats> {
    let start = period.start();
    let end = period.end();

    // Get all the raw data from the database
    let totals = db.get_wrapped_totals(start, end)?;
    let tool_rankings_raw = db.get_wrapped_tool_rankings(start, end, config.top_tools_count)?;
    let hourly_distribution = db.get_wrapped_hourly_distribution(start, end)?;
    let daily_distribution = db.get_wrapped_daily_distribution(start, end)?;
    let projects = db.get_wrapped_project_rankings(start, end, config.top_projects_count)?;
    let marathon_session = db.get_wrapped_marathon_session(start, end)?;
    let streaks = db.get_wrapped_streak_stats(start, end)?;

    // Build tool rankings with witty descriptions if fun mode
    let top_tools: Vec<(String, i64, Option<&'static str>)> = tool_rankings_raw
        .into_iter()
        .map(|(name, count)| {
            let desc = if config.fun_mode {
                Some(ToolRankings::witty_description(&name))
            } else {
                None
            };
            (name, count, desc)
        })
        .collect();

    // Calculate peak hour and busiest/quietest day
    let peak_hour = hourly_distribution
        .iter()
        .enumerate()
        .max_by_key(|(_, &count)| count)
        .map(|(hour, _)| hour as u8)
        .unwrap_or(0);

    let busiest_day = daily_distribution
        .iter()
        .enumerate()
        .max_by_key(|(_, &count)| count)
        .map(|(day, _)| day as u8)
        .unwrap_or(0);

    let quietest_day = daily_distribution
        .iter()
        .enumerate()
        .filter(|(_, &count)| count > 0) // Only consider days with activity
        .min_by_key(|(_, &count)| count)
        .map(|(day, _)| day as u8)
        .unwrap_or(0);

    let time_patterns = TimePatterns {
        hourly_distribution,
        daily_distribution,
        peak_hour,
        busiest_day,
        quietest_day,
        marathon_session,
    };

    // Calculate personality if fun mode
    let personality = if config.fun_mode {
        let profile = db.get_wrapped_usage_profile(start, end)?;
        Some(profile.classify())
    } else {
        None
    };

    // Calculate trends if requested
    let trends = if config.include_trends {
        let prev_period = period.previous();
        let prev_start = prev_period.start();
        let prev_end = prev_period.end();

        if let Ok(prev_totals) = db.get_wrapped_totals(prev_start, prev_end) {
            // Only include trends if there's previous data
            if prev_totals.sessions > 0 {
                Some(TrendComparison {
                    sessions_delta_pct: TrendComparison::calc_delta(
                        totals.sessions,
                        prev_totals.sessions,
                    ),
                    tokens_delta_pct: TrendComparison::calc_delta(
                        totals.total_tokens(),
                        prev_totals.total_tokens(),
                    ),
                    tools_delta_pct: TrendComparison::calc_delta(
                        totals.tool_calls,
                        prev_totals.tool_calls,
                    ),
                    duration_delta_pct: TrendComparison::calc_delta(
                        totals.total_duration_secs,
                        prev_totals.total_duration_secs,
                    ),
                    previous_totals: prev_totals,
                })
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    Ok(WrappedStats {
        period,
        totals,
        tools: ToolRankings { top_tools },
        time_patterns,
        projects,
        personality,
        streaks,
        trends,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrapped_period_year() {
        let period = WrappedPeriod::Year(2024);
        assert_eq!(period.display_name(), "2024");
        assert_eq!(period.previous(), WrappedPeriod::Year(2023));
    }

    #[test]
    fn test_wrapped_period_month() {
        let period = WrappedPeriod::Month(2024, 12);
        assert_eq!(period.display_name(), "December 2024");
        assert_eq!(period.previous(), WrappedPeriod::Month(2024, 11));

        let jan = WrappedPeriod::Month(2024, 1);
        assert_eq!(jan.previous(), WrappedPeriod::Month(2023, 12));
    }

    #[test]
    fn test_total_stats_display() {
        let stats = TotalStats {
            tokens_in: 7_500_000,
            tokens_out: 6_700_000,
            total_duration_secs: 3600 * 312 + 45 * 60,
            ..Default::default()
        };
        assert_eq!(stats.tokens_display(), "14.2M");
        assert_eq!(stats.duration_display(), "312h 45m");
    }

    #[test]
    fn test_trend_delta() {
        assert_eq!(TrendComparison::calc_delta(123, 100), 23.0);
        assert_eq!(TrendComparison::calc_delta(80, 100), -20.0);
        assert_eq!(TrendComparison::calc_delta(100, 0), 100.0);
        assert_eq!(TrendComparison::calc_delta(0, 0), 0.0);
    }

    #[test]
    fn test_hour_display() {
        assert_eq!(TimePatterns::hour_display(0), "12am–1am");
        assert_eq!(TimePatterns::hour_display(10), "10am–11am");
        assert_eq!(TimePatterns::hour_display(12), "12pm–1pm");
        assert_eq!(TimePatterns::hour_display(23), "11pm–12am");
    }
}
