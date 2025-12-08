//! UI rendering for the TUI.

use aiobscura_core::{Message, MessageType, PlanStatus};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, Wrap},
    Frame,
};

use crate::app::{App, ViewMode};

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
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(" navigate  "),
        Span::styled("g/G", Style::default().fg(Color::Yellow)),
        Span::raw(" top/bottom  "),
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
