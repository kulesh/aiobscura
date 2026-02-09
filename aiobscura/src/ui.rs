//! UI rendering for the TUI.

mod detail;
mod live;
mod project;
mod wrapped;

use aiobscura_core::analytics::{TimePatterns, WrappedStats};
use aiobscura_core::format::format_relative_time;
use aiobscura_core::{
    ActiveSession, Assistant, Message, MessageType, MessageWithContext, PlanStatus, ThreadType,
};
use chrono::{DateTime, Local, Utc};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Sparkline, Table, Wrap,
    },
    Frame,
};

use crate::app::{App, ProjectSubTab, ViewMode};
use crate::message_format::{
    detail_content, detail_preview, live_preview, live_role_label, session_role_prefix,
};
use aiobscura_core::db::EnvironmentHealth;
use detail::{
    format_plan_status, render_header, render_list_footer, render_tab_header, render_table,
    truncate_string, ActiveTab,
};

// ========== Wrapped Color Palette ==========
// Vibrant colors for a Spotify Wrapped-inspired experience

/// Gold for achievements and #1 rankings
const WRAPPED_GOLD: Color = Color::Rgb(255, 215, 0);
/// Bright cyan for highlights and accents
const WRAPPED_CYAN: Color = Color::Rgb(0, 255, 255);
/// Magenta for personality and special reveals
const WRAPPED_MAGENTA: Color = Color::Rgb(255, 0, 255);
/// Lime green for positive trends
const WRAPPED_LIME: Color = Color::Rgb(50, 205, 50);
/// Coral for warm accents
const WRAPPED_CORAL: Color = Color::Rgb(255, 127, 80);
/// Silver for #2 rankings
const WRAPPED_SILVER: Color = Color::Rgb(192, 192, 192);
/// Bronze for #3 rankings
const WRAPPED_BRONZE: Color = Color::Rgb(205, 127, 50);
/// Purple for secondary highlights
const WRAPPED_PURPLE: Color = Color::Rgb(138, 43, 226);
/// Soft white for primary text
const WRAPPED_WHITE: Color = Color::Rgb(250, 250, 250);
/// Dim gray for secondary text
const WRAPPED_DIM: Color = Color::Rgb(128, 128, 128);

// ========== Standard View Colors ==========
// Consistent colors for main TUI views

/// Main thread badge color
const BADGE_MAIN: Color = Color::Rgb(0, 180, 180);
/// Agent thread badge color
const BADGE_AGENT: Color = Color::Rgb(220, 180, 0);
/// Background thread badge color
const BADGE_BG: Color = Color::Rgb(120, 120, 120);
/// Separator line color
const SEPARATOR_COLOR: Color = Color::Rgb(60, 60, 60);
/// Border color for Session Info block
const BORDER_INFO: Color = Color::Rgb(0, 150, 150);
/// Border color for Messages block
const BORDER_MESSAGES: Color = Color::Rgb(80, 160, 80);
/// Label color for metadata attributes
const LABEL_COLOR: Color = Color::Rgb(100, 180, 180);
/// Border color for Plan/Content blocks
const BORDER_PLAN: Color = Color::Rgb(180, 100, 180);
/// Markdown header color
const MD_HEADER: Color = Color::Rgb(255, 180, 100);
/// Markdown code block color
const MD_CODE: Color = Color::Rgb(150, 150, 150);

/// Border color for Project blocks
const BORDER_PROJECT: Color = Color::Rgb(100, 180, 100);

/// Render the application UI.
pub fn render(frame: &mut Frame, app: &mut App) {
    match &app.view_mode {
        ViewMode::List => render_list_view(frame, app),
        ViewMode::Detail { thread_name, .. } => {
            detail::render_detail_view(frame, app, thread_name.clone())
        }
        ViewMode::PlanList { session_name, .. } => {
            detail::render_plan_list_view(frame, app, session_name.clone())
        }
        ViewMode::PlanDetail { plan_title, .. } => {
            detail::render_plan_detail_view(frame, app, plan_title.clone())
        }
        ViewMode::Wrapped => wrapped::render_wrapped_view(frame, app),
        ViewMode::Live => live::render_live_view(frame, app),
        ViewMode::ProjectList => project::render_project_list_view(frame, app),
        ViewMode::ProjectDetail {
            project_name,
            sub_tab,
            ..
        } => project::render_project_detail_view(frame, app, project_name.clone(), *sub_tab),
        ViewMode::SessionDetail { session_name, .. } => {
            detail::render_session_detail_view(frame, app, session_name.clone())
        }
    }
}

/// Render the list view (threads table).
fn render_list_view(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Layout: tab header, table, footer
    let chunks = Layout::vertical([
        Constraint::Length(2), // Tab header
        Constraint::Min(5),    // Table
        Constraint::Length(1), // Footer
    ])
    .split(area);

    render_tab_header(frame, ActiveTab::Threads, chunks[0]);
    render_table(frame, app, chunks[1]);
    render_list_footer(frame, app, chunks[2]);
}

/// Render the detail view (thread messages).
fn render_dashboard_panel(frame: &mut Frame, app: &App, area: Rect) {
    // Split into two columns: Activity (left) | Stats (right)
    let chunks = Layout::horizontal([
        Constraint::Percentage(60), // Activity heatmap
        Constraint::Percentage(40), // Stats summary
    ])
    .split(area);

    render_activity_panel(frame, app, chunks[0]);
    render_stats_panel(frame, app, chunks[1]);
}

/// Render the activity panel with heatmap and streak info.
fn render_activity_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Activity ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(stats) = &app.dashboard_stats {
        // Build heatmap line (28 chars for 28 days)
        let heatmap_spans = render_heatmap_spans(&stats.daily_activity);

        // Day labels (4 weeks)
        let day_labels = "M T W T F S S  M T W T F S S  M T W T F S S  M T W T F S S";

        // Streak info line
        let streak_line = vec![
            Span::raw("Streak: "),
            Span::styled(
                format!("{} days", stats.current_streak),
                Style::default().fg(WRAPPED_LIME).bold(),
            ),
            Span::raw("  │  Longest: "),
            Span::styled(
                format!("{} days", stats.longest_streak),
                Style::default().fg(WRAPPED_GOLD).bold(),
            ),
        ];

        let lines = vec![
            Line::from(heatmap_spans),
            Line::from(Span::styled(day_labels, Style::default().fg(WRAPPED_DIM))),
            Line::raw(""),
            Line::from(streak_line),
        ];

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    } else {
        // No stats available
        let placeholder =
            Paragraph::new("Loading activity data...").style(Style::default().fg(WRAPPED_DIM));
        frame.render_widget(placeholder, inner);
    }
}

/// Render the heatmap as styled spans (28 days) with relative shading.
fn render_heatmap_spans(daily_activity: &[i64; 28]) -> Vec<Span<'static>> {
    // Calculate relative thresholds based on actual data
    // Use quartiles for adaptive shading
    let mut non_zero: Vec<i64> = daily_activity.iter().copied().filter(|&x| x > 0).collect();

    // Determine thresholds based on data distribution
    let (low_thresh, med_thresh) = if non_zero.is_empty() {
        (1, 2) // Defaults if no activity
    } else {
        non_zero.sort_unstable();
        let len = non_zero.len();
        // Q1 (25th percentile) and Q2 (50th percentile / median)
        let q1 = non_zero[len / 4];
        let q2 = non_zero[len / 2];
        // Ensure thresholds are distinct and sensible
        let low = q1.max(1);
        let med = q2.max(low + 1);
        (low, med)
    };

    // Convert activity counts to intensity blocks with spacing
    // ░ (none), ▒ (low), ▓ (medium), █ (high)
    let mut spans = Vec::new();

    for (i, &count) in daily_activity.iter().enumerate() {
        let (ch, color) = if count == 0 {
            ('░', WRAPPED_DIM)
        } else if count <= low_thresh {
            ('▒', Color::Rgb(0, 100, 0)) // Dark green - below Q1
        } else if count <= med_thresh {
            ('▓', Color::Rgb(0, 180, 0)) // Medium green - Q1 to Q2
        } else {
            ('█', WRAPPED_LIME) // Bright green - above Q2
        };

        spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));

        // Add space after each day, extra space after each week (every 7 days)
        if (i + 1) % 7 == 0 && i < 27 {
            spans.push(Span::raw("  ")); // Extra space between weeks
        } else {
            spans.push(Span::raw(" "));
        }
    }

    spans
}

/// Render the stats panel with totals and patterns.
fn render_stats_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Stats ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(stats) = &app.dashboard_stats {
        let lines = vec![
            // Row 1: Projects & Sessions
            Line::from(vec![
                Span::styled(
                    format!("{}", stats.project_count),
                    Style::default().fg(WRAPPED_CYAN).bold(),
                ),
                Span::raw(" projects  "),
                Span::styled(
                    format!("{}", stats.session_count),
                    Style::default().fg(WRAPPED_LIME).bold(),
                ),
                Span::raw(" sessions"),
            ]),
            // Row 2: Tokens & Time
            Line::from(vec![
                Span::styled(
                    format_tokens_short(stats.total_tokens),
                    Style::default().fg(WRAPPED_GOLD).bold(),
                ),
                Span::raw(" tokens  "),
                Span::styled(
                    stats.format_duration(),
                    Style::default().fg(WRAPPED_PURPLE).bold(),
                ),
                Span::raw(" total"),
            ]),
            Line::raw(""),
            // Row 3: Peak patterns
            Line::from(vec![
                Span::raw("Peak: "),
                Span::styled(stats.format_peak_hour(), Style::default().fg(WRAPPED_CORAL)),
                Span::raw("  Busiest: "),
                Span::styled(
                    stats.format_busiest_day(),
                    Style::default().fg(WRAPPED_CORAL),
                ),
            ]),
        ];

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    } else {
        // No stats available
        let placeholder =
            Paragraph::new("Loading stats...").style(Style::default().fg(WRAPPED_DIM));
        frame.render_widget(placeholder, inner);
    }
}

/// Format token count in short form (e.g., "5.2M", "847K").
fn format_tokens_short(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.0}K", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
    }
}

/// Format token count for detail rows (e.g., "5.2M", "847.0K").
fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

// ========== Live View ==========

/// Live indicator color (pulsing green)
const LIVE_INDICATOR: Color = Color::Rgb(50, 255, 50);
/// Live view border color
const BORDER_LIVE: Color = Color::Rgb(50, 200, 50);
