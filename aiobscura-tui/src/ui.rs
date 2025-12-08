//! UI rendering for the TUI.

use aiobscura_core::analytics::{TimePatterns, WrappedStats};
use aiobscura_core::{Message, MessageType, PlanStatus};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Sparkline, Table, Wrap,
    },
    Frame,
};

use crate::app::{App, ViewMode};

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

/// Render the application UI.
pub fn render(frame: &mut Frame, app: &mut App) {
    match &app.view_mode {
        ViewMode::List => render_list_view(frame, app),
        ViewMode::Detail { thread_name, .. } => {
            render_detail_view(frame, app, thread_name.clone())
        }
        ViewMode::PlanList { session_name, .. } => {
            render_plan_list_view(frame, app, session_name.clone())
        }
        ViewMode::PlanDetail { plan_title, .. } => {
            render_plan_detail_view(frame, app, plan_title.clone())
        }
        ViewMode::Wrapped => render_wrapped_view(frame, app),
    }
}

/// Render the list view (threads table).
fn render_list_view(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Layout: header, table, footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Min(5),    // Table
        Constraint::Length(1), // Footer
    ])
    .split(area);

    render_header(frame, "aiobscura - AI Agent Activity Monitor", chunks[0]);
    render_table(frame, app, chunks[1]);
    render_list_footer(frame, app, chunks[2]);
}

/// Render the detail view (thread messages).
fn render_detail_view(frame: &mut Frame, app: &mut App, thread_name: String) {
    let area = frame.area();

    // Layout: header, metadata, messages, footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Length(5), // Metadata summary (4 rows + border)
        Constraint::Min(5),    // Messages
        Constraint::Length(1), // Footer
    ])
    .split(area);

    render_header(frame, &format!("Thread: {}", thread_name), chunks[0]);
    render_thread_metadata(frame, app, chunks[1]);
    render_messages(frame, app, chunks[2]);
    render_detail_footer(frame, app, chunks[3]);
}

/// Render the header with title.
fn render_header(frame: &mut Frame, title: &str, area: Rect) {
    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).bold())
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, area);
}

/// Render the thread metadata summary section.
fn render_thread_metadata(frame: &mut Frame, app: &App, area: Rect) {
    let meta = match &app.thread_metadata {
        Some(m) => m,
        None => {
            // Show placeholder if no metadata
            let placeholder = Paragraph::new("Loading metadata...")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL));
            frame.render_widget(placeholder, area);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    // Row 1: Source file path
    let source_display = format_path(&meta.source_path);
    lines.push(Line::from(vec![
        Span::styled("Source: ", Style::default().fg(Color::DarkGray)),
        Span::styled(source_display, Style::default().fg(Color::White)),
    ]));

    // Row 2: CWD (branch) | Model | Duration
    let mut row2_spans: Vec<Span> = Vec::new();

    // CWD with git branch
    if let Some(cwd) = &meta.cwd {
        let cwd_display = format_cwd(cwd);
        row2_spans.push(Span::styled("CWD: ", Style::default().fg(Color::DarkGray)));
        row2_spans.push(Span::styled(cwd_display, Style::default().fg(Color::White)));
        if let Some(branch) = &meta.git_branch {
            row2_spans.push(Span::styled(format!(" ({})", branch), Style::default().fg(Color::Yellow)));
        }
        row2_spans.push(Span::raw("  "));
    }

    // Model
    if let Some(model) = &meta.model_name {
        row2_spans.push(Span::styled("Model: ", Style::default().fg(Color::DarkGray)));
        row2_spans.push(Span::styled(model.clone(), Style::default().fg(Color::Cyan)));
        row2_spans.push(Span::raw("  "));
    }

    // Duration
    let duration_display = format_duration(meta.duration_secs);
    row2_spans.push(Span::styled("Duration: ", Style::default().fg(Color::DarkGray)));
    row2_spans.push(Span::styled(duration_display, Style::default().fg(Color::White)));

    lines.push(Line::from(row2_spans));

    // Row 3: Msgs | Agents | Tools | Plans
    let tools_display = format_tool_stats(&meta.tool_stats);
    lines.push(Line::from(vec![
        Span::styled("Msgs: ", Style::default().fg(Color::DarkGray)),
        Span::styled(meta.message_count.to_string(), Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("Agents: ", Style::default().fg(Color::DarkGray)),
        Span::styled(meta.agent_count.to_string(), Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("Tools: ", Style::default().fg(Color::DarkGray)),
        Span::styled(tools_display, Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("Plans: ", Style::default().fg(Color::DarkGray)),
        Span::styled(meta.plan_count.to_string(), Style::default().fg(Color::Magenta)),
        Span::styled(" (p)", Style::default().fg(Color::DarkGray)),
    ]));

    // Row 4: Files modified
    let files_display = format_file_stats(&meta.file_stats);
    lines.push(Line::from(vec![
        Span::styled("Files: ", Style::default().fg(Color::DarkGray)),
        Span::styled(files_display, Style::default().fg(Color::White)),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Session Info "));
    frame.render_widget(paragraph, area);
}

/// Format a path for display (replace $HOME with ~, truncate if needed).
fn format_path(path: &Option<String>) -> String {
    match path {
        Some(p) => {
            // Replace home directory with ~
            let home = std::env::var("HOME").unwrap_or_default();
            let display = if !home.is_empty() && p.starts_with(&home) {
                format!("~{}", &p[home.len()..])
            } else {
                p.clone()
            };

            // Truncate if too long (keep last parts)
            if display.len() > 60 {
                let parts: Vec<&str> = display.split('/').collect();
                if parts.len() > 3 {
                    format!(".../{}", parts[parts.len()-3..].join("/"))
                } else {
                    display
                }
            } else {
                display
            }
        }
        None => "(unknown source)".to_string(),
    }
}

/// Format CWD for display (replace $HOME with ~).
fn format_cwd(cwd: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() && cwd.starts_with(&home) {
        format!("~{}", &cwd[home.len()..])
    } else {
        cwd.to_string()
    }
}

/// Format duration in human-readable form.
fn format_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    } else {
        let days = secs / 86400;
        let hours = (secs % 86400) / 3600;
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
        }
    }
}

/// Format tool stats for display.
fn format_tool_stats(stats: &aiobscura_core::db::ToolStats) -> String {
    if stats.total_calls == 0 {
        return "0".to_string();
    }

    let top_tools: Vec<String> = stats.breakdown
        .iter()
        .take(3)
        .map(|(name, count)| format!("{}:{}", name, count))
        .collect();

    if top_tools.is_empty() {
        stats.total_calls.to_string()
    } else {
        let extra = if stats.breakdown.len() > 3 {
            format!(" +{}", stats.breakdown.len() - 3)
        } else {
            String::new()
        };
        format!("{} ({}{})", stats.total_calls, top_tools.join(", "), extra)
    }
}

/// Format file stats for display.
fn format_file_stats(stats: &aiobscura_core::db::FileStats) -> String {
    if stats.total_files == 0 {
        return "0 modified".to_string();
    }

    // Get basenames and top 2-3 files
    let top_files: Vec<String> = stats.breakdown
        .iter()
        .take(3)
        .map(|(path, count)| {
            let basename = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            format!("{}:{}", basename, count)
        })
        .collect();

    if top_files.is_empty() {
        format!("{} modified", stats.total_files)
    } else {
        let extra = if stats.breakdown.len() > 3 { " ..." } else { "" };
        format!("{} modified ({}{})", stats.total_files, top_files.join(", "), extra)
    }
}

/// Render the threads table.
fn render_table(frame: &mut Frame, app: &mut App, area: Rect) {
    let header_cells = ["Last Updated", "Thread ID", "Project", "Assistant", "Type", "Msgs"]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().fg(Color::Yellow).bold()));
    let header = Row::new(header_cells).height(1);

    let rows = app.threads.iter().map(|thread| {
        Row::new([
            Cell::from(thread.relative_time()),
            Cell::from(thread.short_id()),
            Cell::from(thread.project_name.as_str()),
            Cell::from(thread.assistant_name.as_str()),
            Cell::from(thread.display_thread_type()),
            Cell::from(thread.message_count.to_string()),
        ])
    });

    let widths = [
        Constraint::Length(12),  // Last Updated
        Constraint::Length(10),  // Thread ID
        Constraint::Fill(1),     // Project (flexible)
        Constraint::Length(12),  // Assistant
        Constraint::Length(10),  // Type (with indent space)
        Constraint::Length(6),   // Msgs
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Threads "))
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(Color::Cyan),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut app.table_state);
}

/// Render the messages in detail view.
fn render_messages(frame: &mut Frame, app: &mut App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    for msg in &app.messages {
        let msg_lines = format_message(msg);
        lines.extend(msg_lines);
        lines.push(Line::raw("")); // Blank line between messages
    }

    // Clamp scroll offset
    let max_scroll = lines.len().saturating_sub(area.height as usize);
    if app.scroll_offset > max_scroll {
        app.scroll_offset = max_scroll;
    }

    let paragraph = Paragraph::new(lines.clone())
        .block(Block::default().borders(Borders::ALL).title(" Messages "))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);

    // Render scrollbar
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state =
        ScrollbarState::new(lines.len()).position(app.scroll_offset);

    frame.render_stateful_widget(
        scrollbar,
        area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar_state,
    );
}

/// Format a single message into display lines.
fn format_message(msg: &Message) -> Vec<Line<'static>> {
    let (prefix, style) = match msg.message_type {
        MessageType::Prompt => (
            "[Human]".to_string(),
            Style::default().fg(Color::Cyan).bold(),
        ),
        MessageType::Response => (
            "[Assistant]".to_string(),
            Style::default().fg(Color::Green),
        ),
        MessageType::ToolCall => {
            let name = msg.tool_name.as_deref().unwrap_or("unknown");
            (
                format!("[Tool: {}]", name),
                Style::default().fg(Color::Yellow),
            )
        }
        MessageType::ToolResult => (
            "[Result]".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        MessageType::Error => ("[Error]".to_string(), Style::default().fg(Color::Red)),
        MessageType::Plan => ("[Plan]".to_string(), Style::default().fg(Color::Magenta)),
        MessageType::Summary => ("[Summary]".to_string(), Style::default().fg(Color::Blue)),
        MessageType::Context => ("[Context]".to_string(), Style::default().fg(Color::DarkGray)),
    };

    let mut lines = Vec::new();

    // Header line with prefix
    lines.push(Line::from(Span::styled(prefix, style)));

    // Content
    let content = get_message_content(msg);
    if !content.is_empty() {
        // Truncate very long content (respecting char boundaries)
        let display_content = if content.chars().count() > 2000 {
            let truncated: String = content.chars().take(2000).collect();
            format!("{}... [truncated]", truncated)
        } else {
            content
        };

        for line in display_content.lines() {
            lines.push(Line::from(Span::raw(format!("  {}", line))));
        }
    }

    lines
}

/// Extract displayable content from a message.
fn get_message_content(msg: &Message) -> String {
    // For tool calls, show the tool input
    if msg.message_type == MessageType::ToolCall {
        if let Some(input) = &msg.tool_input {
            return serde_json::to_string_pretty(input).unwrap_or_default();
        }
    }

    // For tool results, show the result
    if msg.message_type == MessageType::ToolResult {
        if let Some(result) = &msg.tool_result {
            return result.clone();
        }
    }

    // Otherwise show content
    msg.content.clone().unwrap_or_default()
}

/// Render the footer for list view.
fn render_list_footer(frame: &mut Frame, app: &App, area: Rect) {
    let thread_count = app.threads.len();
    let selected = app
        .table_state
        .selected()
        .map(|i| i + 1)
        .unwrap_or(0);

    let footer = Line::from(vec![
        Span::styled(" q", Style::default().fg(Color::Yellow)),
        Span::raw(" quit  "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" open  "),
        Span::styled("p", Style::default().fg(Color::Yellow)),
        Span::raw(" plans  "),
        Span::styled("w", Style::default().fg(Color::Yellow)),
        Span::raw(" wrapped  "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(" navigate  "),
        Span::raw("│ "),
        Span::styled(
            format!("{}/{} threads", selected, thread_count),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    frame.render_widget(Paragraph::new(footer), area);
}

/// Render the footer for detail view.
fn render_detail_footer(frame: &mut Frame, app: &App, area: Rect) {
    let msg_count = app.messages.len();

    let footer = Line::from(vec![
        Span::styled(" Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" back  "),
        Span::styled("p", Style::default().fg(Color::Yellow)),
        Span::raw(" plans  "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(" scroll  "),
        Span::styled("g/G", Style::default().fg(Color::Yellow)),
        Span::raw(" top/bottom  "),
        Span::styled("u/d", Style::default().fg(Color::Yellow)),
        Span::raw(" page up/down  "),
        Span::raw("│ "),
        Span::styled(
            format!("{} messages", msg_count),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    frame.render_widget(Paragraph::new(footer), area);
}

// ========== Plan Views ==========

/// Render the plan list view.
fn render_plan_list_view(frame: &mut Frame, app: &mut App, session_name: String) {
    let area = frame.area();

    // Layout: header, table, footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Min(5),    // Table
        Constraint::Length(1), // Footer
    ])
    .split(area);

    render_header(frame, &format!("Plans for: {}", session_name), chunks[0]);
    render_plan_table(frame, app, chunks[1]);
    render_plan_list_footer(frame, app, chunks[2]);
}

/// Render the plan detail view.
fn render_plan_detail_view(frame: &mut Frame, app: &mut App, plan_title: String) {
    let area = frame.area();

    // Layout: header, content, footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Min(5),    // Content
        Constraint::Length(1), // Footer
    ])
    .split(area);

    render_header(frame, &format!("Plan: {}", plan_title), chunks[0]);
    render_plan_content(frame, app, chunks[1]);
    render_plan_detail_footer(frame, app, chunks[2]);
}

/// Render the plans table.
fn render_plan_table(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.plans.is_empty() {
        let empty_msg = Paragraph::new("No plans found for this session")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" Plans "));
        frame.render_widget(empty_msg, area);
        return;
    }

    let header_cells = ["Slug", "Title", "Status", "Modified"]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().fg(Color::Yellow).bold()));
    let header = Row::new(header_cells).height(1);

    let rows = app.plans.iter().map(|plan| {
        let slug = &plan.id;
        let title = plan.title.as_deref().unwrap_or("(untitled)");
        let status = format_plan_status(&plan.status);
        let modified = format_relative_time(plan.modified_at);

        Row::new([
            Cell::from(slug.as_str()),
            Cell::from(title),
            Cell::from(status),
            Cell::from(modified),
        ])
    });

    let widths = [
        Constraint::Length(20),  // Slug
        Constraint::Fill(1),     // Title (flexible)
        Constraint::Length(12),  // Status
        Constraint::Length(12),  // Modified
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Plans "))
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(Color::Magenta),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut app.plan_table_state);
}

/// Render plan content.
fn render_plan_content(frame: &mut Frame, app: &mut App, area: Rect) {
    let content = match &app.selected_plan {
        Some(plan) => plan.content.as_deref().unwrap_or("(empty plan)"),
        None => "(no plan selected)",
    };

    let lines: Vec<Line> = content.lines().map(|l| Line::raw(l.to_string())).collect();

    // Clamp scroll offset
    let max_scroll = lines.len().saturating_sub(area.height.saturating_sub(2) as usize);
    if app.plan_scroll_offset > max_scroll {
        app.plan_scroll_offset = max_scroll;
    }

    let paragraph = Paragraph::new(lines.clone())
        .block(Block::default().borders(Borders::ALL).title(" Content "))
        .wrap(Wrap { trim: false })
        .scroll((app.plan_scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);

    // Render scrollbar
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state =
        ScrollbarState::new(lines.len()).position(app.plan_scroll_offset);

    frame.render_stateful_widget(
        scrollbar,
        area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar_state,
    );
}

/// Render the footer for plan list view.
fn render_plan_list_footer(frame: &mut Frame, app: &App, area: Rect) {
    let plan_count = app.plans.len();
    let selected = app
        .plan_table_state
        .selected()
        .map(|i| i + 1)
        .unwrap_or(0);

    let footer = Line::from(vec![
        Span::styled(" Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" back  "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" view  "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(" navigate  "),
        Span::raw("│ "),
        Span::styled(
            format!("{}/{} plans", selected, plan_count),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    frame.render_widget(Paragraph::new(footer), area);
}

/// Render the footer for plan detail view.
fn render_plan_detail_footer(frame: &mut Frame, app: &App, area: Rect) {
    let line_count = app
        .selected_plan
        .as_ref()
        .map(|p| p.content.as_ref().map(|c| c.lines().count()).unwrap_or(0))
        .unwrap_or(0);

    let footer = Line::from(vec![
        Span::styled(" Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" back  "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(" scroll  "),
        Span::styled("g/G", Style::default().fg(Color::Yellow)),
        Span::raw(" top/bottom  "),
        Span::styled("u/d", Style::default().fg(Color::Yellow)),
        Span::raw(" page up/down  "),
        Span::raw("│ "),
        Span::styled(
            format!("{} lines", line_count),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    frame.render_widget(Paragraph::new(footer), area);
}

/// Format PlanStatus for display.
fn format_plan_status(status: &PlanStatus) -> String {
    match status {
        PlanStatus::Active => "Active".to_string(),
        PlanStatus::Completed => "Completed".to_string(),
        PlanStatus::Abandoned => "Abandoned".to_string(),
        PlanStatus::Unknown => "Unknown".to_string(),
    }
}

/// Format a timestamp as relative time.
fn format_relative_time(ts: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
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

// ========== Wrapped View ==========

/// Render the wrapped view with paginated cards.
fn render_wrapped_view(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Layout: header, card content, footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Min(10),   // Card content
        Constraint::Length(1), // Footer
    ])
    .split(area);

    let stats = match &app.wrapped_stats {
        Some(s) => s,
        None => {
            render_header(frame, "AI Wrapped - Loading...", chunks[0]);
            return;
        }
    };

    // Header with period
    let title = format!("AI Wrapped - {}", stats.period.display_name());
    render_header(frame, &title, chunks[0]);

    // Render the current card
    render_wrapped_card(frame, stats, app.wrapped_card_index, chunks[1]);

    // Footer
    render_wrapped_footer(frame, app, chunks[2]);
}

/// Render a specific wrapped card by index.
fn render_wrapped_card(frame: &mut Frame, stats: &WrappedStats, card_index: usize, area: Rect) {
    // Card types in order: Title, Tools, Time, Streaks, Projects, [Trends], [Personality]
    let card_type = get_wrapped_card_type(stats, card_index);

    match card_type {
        WrappedCardType::Title => render_wrapped_title_card(frame, stats, area),
        WrappedCardType::Tools => render_wrapped_tools_card(frame, stats, area),
        WrappedCardType::Time => render_wrapped_time_card(frame, stats, area),
        WrappedCardType::Streaks => render_wrapped_streaks_card(frame, stats, area),
        WrappedCardType::Projects => render_wrapped_projects_card(frame, stats, area),
        WrappedCardType::Trends => render_wrapped_trends_card(frame, stats, area),
        WrappedCardType::Personality => render_wrapped_personality_card(frame, stats, area),
    }
}

/// Card types for wrapped view.
#[derive(Debug, Clone, Copy)]
enum WrappedCardType {
    Title,
    Tools,
    Time,
    Streaks,
    Projects,
    Trends,
    Personality,
}

/// Get the card type for a given index.
fn get_wrapped_card_type(stats: &WrappedStats, index: usize) -> WrappedCardType {
    // Fixed cards: Title(0), Tools(1), Time(2), Streaks(3), Projects(4)
    // Optional: Trends, Personality
    match index {
        0 => WrappedCardType::Title,
        1 => WrappedCardType::Tools,
        2 => WrappedCardType::Time,
        3 => WrappedCardType::Streaks,
        4 => WrappedCardType::Projects,
        5 => {
            if stats.trends.is_some() {
                WrappedCardType::Trends
            } else if stats.personality.is_some() {
                WrappedCardType::Personality
            } else {
                WrappedCardType::Title // fallback
            }
        }
        6 => {
            if stats.trends.is_some() && stats.personality.is_some() {
                WrappedCardType::Personality
            } else {
                WrappedCardType::Title // fallback
            }
        }
        _ => WrappedCardType::Title,
    }
}

/// Render the title/totals card.
fn render_wrapped_title_card(frame: &mut Frame, stats: &WrappedStats, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Decorative sparkles and big title
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("        ✨ ", Style::default().fg(WRAPPED_GOLD)),
        Span::styled(
            format!("YOUR {} AI WRAPPED", stats.period.display_name().to_uppercase()),
            Style::default().fg(WRAPPED_CYAN).bold(),
        ),
        Span::styled(" ✨", Style::default().fg(WRAPPED_GOLD)),
    ]));
    lines.push(Line::raw(""));

    if stats.totals.sessions == 0 {
        lines.push(Line::styled(
            "        No activity found for this period.",
            Style::default().fg(WRAPPED_DIM),
        ));
    } else {
        // Stats in a celebratory grid layout with big numbers
        lines.push(Line::from(vec![
            Span::styled("   ◆ Sessions  ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("{:<8}", stats.totals.sessions),
                Style::default().fg(WRAPPED_GOLD).bold(),
            ),
            Span::styled("   ◆ Time      ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                stats.totals.duration_display(),
                Style::default().fg(WRAPPED_CORAL).bold(),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("   ◆ Tokens    ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("{:<8}", stats.totals.tokens_display()),
                Style::default().fg(WRAPPED_CYAN).bold(),
            ),
            Span::styled("   ◆ Projects  ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("{}", stats.totals.unique_projects),
                Style::default().fg(WRAPPED_PURPLE).bold(),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("   ◆ Tools     ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("{:<8}", stats.totals.tool_calls),
                Style::default().fg(WRAPPED_LIME).bold(),
            ),
            Span::styled("   ◆ Plans     ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("{}", stats.totals.plans),
                Style::default().fg(WRAPPED_MAGENTA).bold(),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("   ◆ Agents    ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("{:<8}", stats.totals.agents_spawned),
                Style::default().fg(WRAPPED_CORAL).bold(),
            ),
            Span::styled("   ◆ Files     ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("{}", stats.totals.files_modified),
                Style::default().fg(WRAPPED_WHITE).bold(),
            ),
        ]));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(WRAPPED_CYAN))
        .title(Span::styled(" ★ The Numbers ★ ", Style::default().fg(WRAPPED_GOLD).bold()));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left);
    frame.render_widget(paragraph, area);
}

/// Render the top tools card with medals and visual bars.
fn render_wrapped_tools_card(frame: &mut Frame, stats: &WrappedStats, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if stats.tools.top_tools.is_empty() {
        lines.push(Line::styled(
            "     No tool usage recorded.",
            Style::default().fg(WRAPPED_DIM),
        ));
    } else {
        // Find max count for bar scaling
        let max_count = stats.tools.top_tools.iter().map(|(_, c, _)| *c).max().unwrap_or(1);

        for (i, (name, count, desc)) in stats.tools.top_tools.iter().enumerate() {
            // Medal emoji for top 3
            let (medal, rank_color) = match i {
                0 => ("  🥇 ", WRAPPED_GOLD),
                1 => ("  🥈 ", WRAPPED_SILVER),
                2 => ("  🥉 ", WRAPPED_BRONZE),
                3 => ("   4 ", WRAPPED_DIM),
                4 => ("   5 ", WRAPPED_DIM),
                _ => ("     ", WRAPPED_DIM),
            };

            // Visual bar showing relative usage
            let bar_width = 12;
            let filled = (((*count as f64 / max_count as f64) * bar_width as f64) as usize).max(1);
            let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);

            let spans = vec![
                Span::styled(medal, Style::default().fg(rank_color)),
                Span::styled(format!("{:<10}", name), Style::default().fg(WRAPPED_WHITE).bold()),
                Span::styled(format!("{:>6} ", count), Style::default().fg(rank_color).bold()),
                Span::styled(bar, Style::default().fg(rank_color)),
            ];

            lines.push(Line::from(spans));

            // Witty description on second line for top 3
            if i < 3 {
                if let Some(description) = desc {
                    lines.push(Line::from(vec![
                        Span::raw("       "),
                        Span::styled(
                            format!("\"{}\"", description),
                            Style::default().fg(WRAPPED_DIM).italic(),
                        ),
                    ]));
                }
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(WRAPPED_GOLD))
        .title(Span::styled(" 🏆 Top Tools ", Style::default().fg(WRAPPED_GOLD).bold()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Render the time patterns card with sparkline visualization.
fn render_wrapped_time_card(frame: &mut Frame, stats: &WrappedStats, area: Rect) {
    // Create inner layout: text info at top, sparkline at bottom
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(WRAPPED_PURPLE))
        .title(Span::styled(" ⏰ Time Patterns ", Style::default().fg(WRAPPED_PURPLE).bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split inner area: text section and sparkline section
    let chunks = Layout::vertical([
        Constraint::Min(6),    // Text info
        Constraint::Length(4), // Sparkline + labels
    ])
    .split(inner);

    // Text info section
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    // Peak hour with celebration
    let peak_comment = match stats.time_patterns.peak_hour {
        0..=5 => " (night owl! 🦉)",
        6..=9 => " (early bird! 🐦)",
        10..=12 => " (morning person!)",
        13..=17 => " (afternoon coder!)",
        18..=21 => " (evening warrior!)",
        _ => " (night owl! 🦉)",
    };
    lines.push(Line::from(vec![
        Span::styled("   ◆ Peak hour:    ", Style::default().fg(WRAPPED_DIM)),
        Span::styled(
            TimePatterns::hour_display(stats.time_patterns.peak_hour),
            Style::default().fg(WRAPPED_GOLD).bold(),
        ),
        Span::styled(peak_comment, Style::default().fg(WRAPPED_CYAN)),
    ]));

    // Busiest day
    lines.push(Line::from(vec![
        Span::styled("   ◆ Busiest day:  ", Style::default().fg(WRAPPED_DIM)),
        Span::styled(
            TimePatterns::day_name(stats.time_patterns.busiest_day),
            Style::default().fg(WRAPPED_CORAL).bold(),
        ),
    ]));

    // Marathon session with special highlight
    if let Some(marathon) = &stats.time_patterns.marathon_session {
        let project = marathon.project_name.as_deref().unwrap_or("unknown");
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("   🏃 Marathon:    ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("{} on ", marathon.date_display()),
                Style::default().fg(WRAPPED_WHITE),
            ),
            Span::styled(project, Style::default().fg(WRAPPED_CYAN).bold()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("                   ", Style::default()),
            Span::styled(
                format!("{} of intense coding!", marathon.duration_display()),
                Style::default().fg(WRAPPED_MAGENTA).bold(),
            ),
        ]));
    }

    let text = Paragraph::new(lines);
    frame.render_widget(text, chunks[0]);

    // Sparkline section
    let sparkline_data: Vec<u64> = stats.time_patterns.hourly_distribution
        .iter()
        .map(|&x| x as u64)
        .collect();

    let sparkline_chunks = Layout::vertical([
        Constraint::Length(1), // Label
        Constraint::Length(2), // Sparkline
        Constraint::Length(1), // Time labels
    ])
    .split(chunks[1]);

    let label = Paragraph::new(Line::from(vec![
        Span::styled("   Activity by hour: ", Style::default().fg(WRAPPED_DIM)),
    ]));
    frame.render_widget(label, sparkline_chunks[0]);

    // Use Sparkline widget
    let sparkline = Sparkline::default()
        .data(&sparkline_data)
        .style(Style::default().fg(WRAPPED_CYAN))
        .bar_set(symbols::bar::NINE_LEVELS);

    // Add padding for sparkline
    let sparkline_area = Rect {
        x: sparkline_chunks[1].x + 3,
        y: sparkline_chunks[1].y,
        width: sparkline_chunks[1].width.saturating_sub(6),
        height: sparkline_chunks[1].height,
    };
    frame.render_widget(sparkline, sparkline_area);

    let time_labels = Paragraph::new(Line::from(vec![
        Span::styled("   0h        6h        12h       18h       23h", Style::default().fg(WRAPPED_DIM)),
    ]));
    frame.render_widget(time_labels, sparkline_chunks[2]);
}

/// Render the streaks card with gauge visualization.
fn render_wrapped_streaks_card(frame: &mut Frame, stats: &WrappedStats, area: Rect) {
    // Create block and get inner area
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(WRAPPED_CORAL))
        .title(Span::styled(" 🔥 Streaks ", Style::default().fg(WRAPPED_CORAL).bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into text and gauge areas
    let chunks = Layout::vertical([
        Constraint::Min(5),    // Streak info
        Constraint::Length(3), // Gauge
    ])
    .split(inner);

    // Streak info section
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    // Current streak with fire emoji celebration
    let fire_emoji = if stats.streaks.current_streak_days >= 7 {
        " 🔥🔥🔥"
    } else if stats.streaks.current_streak_days >= 3 {
        " 🔥🔥"
    } else if stats.streaks.current_streak_days >= 1 {
        " 🔥"
    } else {
        ""
    };
    lines.push(Line::from(vec![
        Span::styled("   ◆ Current streak:  ", Style::default().fg(WRAPPED_DIM)),
        Span::styled(
            format!("{}", stats.streaks.current_streak_days),
            Style::default().fg(WRAPPED_GOLD).bold(),
        ),
        Span::styled(
            format!(" day{}", if stats.streaks.current_streak_days == 1 { "" } else { "s" }),
            Style::default().fg(WRAPPED_WHITE),
        ),
        Span::styled(fire_emoji, Style::default()),
    ]));

    // Longest streak with celebration
    if stats.streaks.longest_streak_days > 0 {
        let streak_dates = match (&stats.streaks.longest_streak_start, &stats.streaks.longest_streak_end) {
            (Some(start), Some(end)) => {
                format!(" ({} – {})", start.format("%b %d"), end.format("%b %d"))
            }
            _ => String::new(),
        };
        lines.push(Line::from(vec![
            Span::styled("   ◆ Longest streak:  ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("{}", stats.streaks.longest_streak_days),
                Style::default().fg(WRAPPED_CYAN).bold(),
            ),
            Span::styled(
                format!(" day{}", if stats.streaks.longest_streak_days == 1 { "" } else { "s" }),
                Style::default().fg(WRAPPED_WHITE),
            ),
            Span::styled(streak_dates, Style::default().fg(WRAPPED_DIM)),
        ]));
    }

    // Active days summary
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("   ◆ Active days:     ", Style::default().fg(WRAPPED_DIM)),
        Span::styled(
            format!("{}", stats.streaks.active_days),
            Style::default().fg(WRAPPED_LIME).bold(),
        ),
        Span::styled(
            format!(" of {} days", stats.streaks.total_days),
            Style::default().fg(WRAPPED_WHITE),
        ),
    ]));

    let text = Paragraph::new(lines);
    frame.render_widget(text, chunks[0]);

    // Gauge for activity percentage
    let activity_pct = stats.streaks.activity_percentage();
    let gauge_color = if activity_pct >= 75.0 {
        WRAPPED_LIME
    } else if activity_pct >= 50.0 {
        WRAPPED_GOLD
    } else if activity_pct >= 25.0 {
        WRAPPED_CORAL
    } else {
        WRAPPED_MAGENTA
    };

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color).bg(Color::Rgb(40, 40, 40)))
        .ratio(activity_pct / 100.0)
        .label(Span::styled(
            format!("{:.0}% active", activity_pct),
            Style::default().fg(WRAPPED_WHITE).bold(),
        ));

    let gauge_area = Rect {
        x: chunks[1].x + 3,
        y: chunks[1].y,
        width: chunks[1].width.saturating_sub(6),
        height: chunks[1].height,
    };
    frame.render_widget(gauge, gauge_area);
}

/// Render the projects card with visual bars.
fn render_wrapped_projects_card(frame: &mut Frame, stats: &WrappedStats, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if stats.projects.is_empty() {
        lines.push(Line::styled(
            "     No project data available.",
            Style::default().fg(WRAPPED_DIM),
        ));
    } else {
        // Find max tokens for bar scaling
        let max_tokens = stats.projects.iter().map(|p| p.tokens).max().unwrap_or(1);

        for (i, project) in stats.projects.iter().take(5).enumerate() {
            // Visual bar showing relative activity
            let bar_width = 15;
            let filled = (((project.tokens as f64 / max_tokens as f64) * bar_width as f64) as usize).max(1);
            let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);

            // Rank indicator with special treatment for #1
            let (rank_indicator, name_color, bar_color) = match i {
                0 => ("  🏆 ", WRAPPED_GOLD, WRAPPED_GOLD),
                1 => ("   2 ", WRAPPED_SILVER, WRAPPED_SILVER),
                2 => ("   3 ", WRAPPED_BRONZE, WRAPPED_BRONZE),
                _ => (if i == 3 { "   4 " } else { "   5 " }, WRAPPED_DIM, WRAPPED_DIM),
            };

            lines.push(Line::from(vec![
                Span::styled(rank_indicator, Style::default().fg(name_color)),
                Span::styled(
                    format!("{:<20}", &project.name),
                    Style::default().fg(WRAPPED_WHITE).bold(),
                ),
                Span::styled(bar, Style::default().fg(bar_color)),
            ]));

            lines.push(Line::from(vec![
                Span::raw("       "),
                Span::styled(
                    format!("{} sessions", project.sessions),
                    Style::default().fg(WRAPPED_DIM),
                ),
                Span::styled(" · ", Style::default().fg(WRAPPED_DIM)),
                Span::styled(
                    format!("{} tokens", format_tokens(project.tokens)),
                    Style::default().fg(WRAPPED_CYAN),
                ),
            ]));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(WRAPPED_LIME))
        .title(Span::styled(" 📁 Top Projects ", Style::default().fg(WRAPPED_LIME).bold()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Render the trends comparison card with arrows and visual impact.
fn render_wrapped_trends_card(frame: &mut Frame, stats: &WrappedStats, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if let Some(trends) = &stats.trends {
        lines.push(Line::from(vec![
            Span::styled("   Compared to previous period:", Style::default().fg(WRAPPED_DIM)),
        ]));
        lines.push(Line::raw(""));

        // Helper function to format trend with arrow
        fn trend_line(label: &str, delta: f64) -> Line<'static> {
            let (arrow, color) = if delta > 0.0 {
                ("↑", WRAPPED_LIME)
            } else if delta < 0.0 {
                ("↓", Color::Rgb(255, 99, 71)) // Tomato red
            } else {
                ("→", WRAPPED_DIM)
            };

            Line::from(vec![
                Span::styled(format!("   {} ", arrow), Style::default().fg(color).bold()),
                Span::styled(format!("{:<12}", label), Style::default().fg(WRAPPED_DIM)),
                Span::styled(
                    format!("{:>+.0}%", delta),
                    Style::default().fg(color).bold(),
                ),
            ])
        }

        lines.push(trend_line("Sessions", trends.sessions_delta_pct));
        lines.push(trend_line("Tokens", trends.tokens_delta_pct));
        lines.push(trend_line("Tools", trends.tools_delta_pct));
        lines.push(trend_line("Duration", trends.duration_delta_pct));

        // Summary message
        lines.push(Line::raw(""));
        let overall_trend = (trends.sessions_delta_pct + trends.tokens_delta_pct) / 2.0;
        let message = if overall_trend > 20.0 {
            ("🚀 Major growth!", WRAPPED_LIME)
        } else if overall_trend > 0.0 {
            ("📈 Trending up!", WRAPPED_CYAN)
        } else if overall_trend > -20.0 {
            ("📉 Slight dip", WRAPPED_CORAL)
        } else {
            ("💤 Taking it easy", WRAPPED_DIM)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("   {}", message.0), Style::default().fg(message.1)),
        ]));
    } else {
        lines.push(Line::styled(
            "     No previous period data available.",
            Style::default().fg(WRAPPED_DIM),
        ));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(WRAPPED_CYAN))
        .title(Span::styled(" 📈 vs Previous Period ", Style::default().fg(WRAPPED_CYAN).bold()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Render the personality card as the grand finale with dramatic presentation.
fn render_wrapped_personality_card(frame: &mut Frame, stats: &WrappedStats, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(WRAPPED_MAGENTA))
        .title(Span::styled(
            " ✨ Your Coding Personality ✨ ",
            Style::default().fg(WRAPPED_MAGENTA).bold(),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(personality) = &stats.personality {
        // Split inner area for dramatic layout
        let chunks = Layout::vertical([
            Constraint::Length(1), // Top padding
            Constraint::Length(1), // "And your personality is..."
            Constraint::Length(1), // spacing
            Constraint::Length(3), // HUGE emoji display
            Constraint::Length(1), // spacing
            Constraint::Length(1), // Personality name
            Constraint::Length(1), // spacing
            Constraint::Min(1),    // Tagline
        ])
        .split(inner);

        // Reveal text
        let reveal = Paragraph::new(Line::from(vec![
            Span::styled("★ ", Style::default().fg(WRAPPED_GOLD)),
            Span::styled(
                "And your coding personality is...",
                Style::default().fg(WRAPPED_DIM).italic(),
            ),
            Span::styled(" ★", Style::default().fg(WRAPPED_GOLD)),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(reveal, chunks[1]);

        // Large emoji display with decorative frame
        let emoji = personality.emoji();
        let emoji_lines = vec![
            Line::from(Span::styled(
                "╔═══════════════════╗",
                Style::default().fg(WRAPPED_PURPLE),
            )),
            Line::from(vec![
                Span::styled("║       ", Style::default().fg(WRAPPED_PURPLE)),
                Span::raw(emoji),
                Span::styled("        ║", Style::default().fg(WRAPPED_PURPLE)),
            ]),
            Line::from(Span::styled(
                "╚═══════════════════╝",
                Style::default().fg(WRAPPED_PURPLE),
            )),
        ];
        let emoji_para = Paragraph::new(emoji_lines).alignment(Alignment::Center);
        frame.render_widget(emoji_para, chunks[3]);

        // Personality name in bold magenta
        let name_line = Line::from(vec![
            Span::styled("✦ ", Style::default().fg(WRAPPED_CORAL)),
            Span::styled(
                personality.name().to_uppercase(),
                Style::default()
                    .fg(WRAPPED_MAGENTA)
                    .bold()
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ✦", Style::default().fg(WRAPPED_CORAL)),
        ]);
        let name_para = Paragraph::new(name_line).alignment(Alignment::Center);
        frame.render_widget(name_para, chunks[5]);

        // Tagline in styled italic
        let tagline = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("\"{}\"", personality.tagline()),
                Style::default().fg(WRAPPED_WHITE).italic(),
            ),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(tagline, chunks[7]);
    } else {
        let no_data = Paragraph::new(Line::styled(
            "Personality not available - need more data!",
            Style::default().fg(WRAPPED_DIM),
        ))
        .alignment(Alignment::Center);
        frame.render_widget(no_data, inner);
    }
}

/// Format tokens for display.
fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

/// Render the wrapped footer with card position dots.
fn render_wrapped_footer(frame: &mut Frame, app: &App, area: Rect) {
    let card_count = app.wrapped_card_count();
    let current_index = app.wrapped_card_index;

    let period_hint = match app.wrapped_period {
        aiobscura_core::analytics::WrappedPeriod::Year(_) => "year",
        aiobscura_core::analytics::WrappedPeriod::Month(_, _) => "month",
    };

    // Build card position dots (●○○○○)
    let mut dots: Vec<Span> = Vec::new();
    for i in 0..card_count {
        if i == current_index {
            dots.push(Span::styled("●", Style::default().fg(WRAPPED_CYAN)));
        } else {
            dots.push(Span::styled("○", Style::default().fg(WRAPPED_DIM)));
        }
    }

    let mut footer_spans = vec![
        Span::styled(" Esc", Style::default().fg(WRAPPED_GOLD)),
        Span::styled(" back  ", Style::default().fg(WRAPPED_DIM)),
        Span::styled("←/→", Style::default().fg(WRAPPED_GOLD)),
        Span::styled(" navigate  ", Style::default().fg(WRAPPED_DIM)),
        Span::styled("m", Style::default().fg(WRAPPED_GOLD)),
        Span::styled(format!(" {} ", period_hint), Style::default().fg(WRAPPED_DIM)),
        Span::styled("│ ", Style::default().fg(WRAPPED_DIM)),
    ];
    footer_spans.extend(dots);

    let footer = Line::from(footer_spans);
    frame.render_widget(Paragraph::new(footer), area);
}
