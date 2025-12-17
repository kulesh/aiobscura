//! UI rendering for the TUI.

use aiobscura_core::analytics::{TimePatterns, WrappedStats};
use aiobscura_core::{
    ActiveSession, Assistant, AuthorRole, Message, MessageType, MessageWithContext, PlanStatus,
    ThreadType,
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
        ViewMode::Detail { thread_name, .. } => render_detail_view(frame, app, thread_name.clone()),
        ViewMode::PlanList { session_name, .. } => {
            render_plan_list_view(frame, app, session_name.clone())
        }
        ViewMode::PlanDetail { plan_title, .. } => {
            render_plan_detail_view(frame, app, plan_title.clone())
        }
        ViewMode::Wrapped => render_wrapped_view(frame, app),
        ViewMode::Live => render_live_view(frame, app),
        ViewMode::ProjectList => render_project_list_view(frame, app),
        ViewMode::ProjectDetail {
            project_name,
            sub_tab,
            ..
        } => render_project_detail_view(frame, app, project_name.clone(), *sub_tab),
        ViewMode::SessionDetail { session_name, .. } => {
            render_session_detail_view(frame, app, session_name.clone())
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
fn render_detail_view(frame: &mut Frame, app: &mut App, thread_name: String) {
    let area = frame.area();

    // Layout: header, metadata, analytics, messages, footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Length(5), // Metadata summary (4 rows + border)
        Constraint::Length(6), // Analytics panel (4 lines + 2 border)
        Constraint::Min(5),    // Messages
        Constraint::Length(1), // Footer
    ])
    .split(area);

    render_header(frame, &format!("Thread: {}", thread_name), chunks[0]);
    render_thread_metadata(frame, app, chunks[1]);
    render_analytics_panel(frame, app, chunks[2]);
    render_messages(frame, app, chunks[3]);
    render_detail_footer(frame, app, chunks[4]);
}

/// Render the session detail view (merged messages across all threads).
fn render_session_detail_view(frame: &mut Frame, app: &mut App, session_name: String) {
    let area = frame.area();

    // Layout: header, analytics, messages, footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Length(5), // Analytics panel
        Constraint::Min(5),    // Messages
        Constraint::Length(1), // Footer
    ])
    .split(area);

    render_header(frame, &format!("Session: {}", session_name), chunks[0]);
    render_session_analytics_panel(frame, app, chunks[1]);
    render_session_messages(frame, app, chunks[2]);
    render_session_detail_footer(frame, chunks[3]);
}

/// Render session-level analytics panel (no toggle, just session stats).
fn render_session_analytics_panel(frame: &mut Frame, app: &App, area: Rect) {
    // Check for error state
    if let Some(ref error) = app.session_analytics_error {
        let error_line = Line::from(vec![
            Span::styled("Error: ", Style::default().fg(Color::Red)),
            Span::styled(
                truncate_string(error, 60),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        let paragraph = Paragraph::new(error_line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Red))
                .title(" Session Analytics ")
                .title_style(Style::default().fg(Color::Red)),
        );
        frame.render_widget(paragraph, area);
        return;
    }

    let lines = build_session_analytics_lines(app);

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER_INFO))
            .title(" Session Analytics ")
            .title_style(Style::default().fg(BORDER_INFO).bold()),
    );
    frame.render_widget(paragraph, area);
}

/// Render the session messages with thread segmentation.
fn render_session_messages(frame: &mut Frame, app: &App, area: Rect) {
    if app.session_messages.is_empty() {
        let empty = Paragraph::new("No messages in this session")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(BORDER_MESSAGES)),
            );
        frame.render_widget(empty, area);
        return;
    }

    // Build message groups by consecutive thread
    let groups = group_messages_by_thread(&app.session_messages);

    // Build lines with thread headers
    let mut lines: Vec<Line> = Vec::new();
    for (thread_id, messages) in groups {
        // Add thread header
        let thread_label = if thread_id.len() > 8 {
            thread_id[..8].to_string()
        } else {
            thread_id.clone()
        };

        // Determine thread type, badge, and color
        let (thread_type, thread_color, badge) =
            if let Some(thread) = app.session_threads.iter().find(|t| t.id == thread_id) {
                match thread.thread_type {
                    ThreadType::Main => (ThreadType::Main, BADGE_MAIN, "●"),
                    ThreadType::Agent => (ThreadType::Agent, BADGE_AGENT, "◎"),
                    ThreadType::Background => (ThreadType::Background, BADGE_BG, "◇"),
                }
            } else {
                (ThreadType::Main, Color::DarkGray, "●")
            };

        // For non-main threads, calculate and show duration
        let duration_str = if !matches!(thread_type, ThreadType::Main) && !messages.is_empty() {
            let first_ts = messages.first().unwrap().ts;
            let last_ts = messages.last().unwrap().ts;
            format!(" ({})", format_group_duration(first_ts, last_ts))
        } else {
            String::new()
        };

        // Build thread header line
        lines.push(Line::from(vec![
            Span::styled("─── ", Style::default().fg(thread_color)),
            Span::styled(format!("{} ", badge), Style::default().fg(thread_color)),
            Span::styled(thread_label, Style::default().fg(thread_color).bold()),
            Span::styled(duration_str, Style::default().fg(thread_color)),
            Span::styled(
                " ───────────────────────────────────",
                Style::default().fg(thread_color),
            ),
        ]));

        // Add messages for this thread
        for msg in messages {
            // Map author role directly to display style and label.
            // The parser layer determines the correct role (Human vs Caller)
            // based on context, so UI just needs a 1:1 mapping.
            let (role_style, role_prefix) = match msg.author_role {
                AuthorRole::Human => (Style::default().fg(Color::Green), "[human]"),
                AuthorRole::Caller => (Style::default().fg(Color::Cyan), "[caller]"),
                AuthorRole::Assistant => (Style::default().fg(Color::Blue), "[assistant]"),
                AuthorRole::Agent => (Style::default().fg(Color::Cyan), "[agent]"),
                AuthorRole::Tool => (Style::default().fg(Color::Magenta), "[tool]"),
                AuthorRole::System => {
                    // System is for true system events like snapshots
                    if msg.author_name.as_deref() == Some("snapshot") {
                        (Style::default().fg(Color::DarkGray), "[snapshot]")
                    } else {
                        (Style::default().fg(Color::DarkGray), "[system]")
                    }
                }
            };

            // Get content preview (shortened to leave room for timestamp)
            let content_preview = msg.preview(60);

            // Format timestamp (HH:MM in local time)
            let time_str = format_message_time(msg.ts);

            // Calculate padding for right-aligned timestamp
            let role_part = format!("{} ", role_prefix);
            let content_len = role_part.chars().count() + content_preview.chars().count();
            let target_width: usize = 72; // Target width before timestamp
            let padding_needed = target_width.saturating_sub(content_len);
            let padding = " ".repeat(padding_needed);

            lines.push(Line::from(vec![
                Span::styled(role_part, role_style),
                Span::styled(content_preview, Style::default().fg(Color::White)),
                Span::raw(padding),
                Span::styled(time_str, Style::default().fg(Color::DarkGray)),
            ]));
        }

        lines.push(Line::from("")); // Blank line between groups
    }

    // Calculate scrolling
    let visible_height = area.height.saturating_sub(2) as usize; // Subtract border
    let max_scroll = lines.len().saturating_sub(visible_height);
    let scroll_offset = app.session_scroll_offset.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .scroll((scroll_offset as u16, 0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER_MESSAGES))
                .title(format!(" Messages ({}) ", app.session_messages.len()))
                .title_style(Style::default().fg(BORDER_MESSAGES).bold()),
        );
    frame.render_widget(paragraph, area);
}

/// Group messages by consecutive thread_id.
fn group_messages_by_thread(messages: &[Message]) -> Vec<(String, Vec<&Message>)> {
    let mut groups: Vec<(String, Vec<&Message>)> = Vec::new();

    for msg in messages {
        if let Some((last_thread_id, last_group)) = groups.last_mut() {
            if *last_thread_id == msg.thread_id {
                last_group.push(msg);
            } else {
                groups.push((msg.thread_id.clone(), vec![msg]));
            }
        } else {
            groups.push((msg.thread_id.clone(), vec![msg]));
        }
    }

    groups
}

/// Render the footer for session detail view.
fn render_session_detail_footer(frame: &mut Frame, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" back  "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(" scroll  "),
        Span::styled("g/G", Style::default().fg(Color::Yellow)),
        Span::raw(" top/bottom  "),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(" quit"),
    ]))
    .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, area);
}

/// Render the header with title.
fn render_header(frame: &mut Frame, title: &str, area: Rect) {
    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).bold())
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, area);
}

/// Which tab is currently active.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ActiveTab {
    Live,
    Projects,
    Threads,
}

/// Render the tab bar header with Live, Projects, and Threads tabs.
fn render_tab_header(frame: &mut Frame, active: ActiveTab, area: Rect) {
    // Layout: app name on left, tabs in center/right
    let chunks = Layout::horizontal([
        Constraint::Length(12), // App name
        Constraint::Min(1),     // Tabs
    ])
    .split(area);

    // App name
    let app_name = Paragraph::new(" aiobscura").style(Style::default().fg(Color::Cyan).bold());
    frame.render_widget(app_name, chunks[0]);

    // Tab styling
    let active_style = Style::default()
        .fg(Color::Cyan)
        .bold()
        .add_modifier(Modifier::UNDERLINED);
    let inactive_style = Style::default().fg(Color::DarkGray);

    let live_style = if active == ActiveTab::Live {
        active_style
    } else {
        inactive_style
    };
    let projects_style = if active == ActiveTab::Projects {
        active_style
    } else {
        inactive_style
    };
    let threads_style = if active == ActiveTab::Threads {
        active_style
    } else {
        inactive_style
    };

    let tabs = Line::from(vec![
        Span::styled(" Live ", live_style),
        Span::styled("  ", Style::default()),
        Span::styled(" Projects ", projects_style),
        Span::styled("  ", Style::default()),
        Span::styled(" Threads ", threads_style),
    ]);

    let tabs_para = Paragraph::new(tabs).block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(tabs_para, chunks[1]);
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
        Span::styled("Source: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(source_display, Style::default().fg(Color::White)),
    ]));

    // Row 2: CWD (branch) | Model | Duration
    let mut row2_spans: Vec<Span> = Vec::new();

    // CWD with git branch
    if let Some(cwd) = &meta.cwd {
        let cwd_display = format_cwd(cwd);
        row2_spans.push(Span::styled("CWD: ", Style::default().fg(LABEL_COLOR)));
        row2_spans.push(Span::styled(cwd_display, Style::default().fg(Color::White)));
        if let Some(branch) = &meta.git_branch {
            row2_spans.push(Span::styled(
                format!(" ({})", branch),
                Style::default().fg(Color::Yellow),
            ));
        }
        row2_spans.push(Span::raw("  "));
    }

    // Model
    if let Some(model) = &meta.model_name {
        row2_spans.push(Span::styled("Model: ", Style::default().fg(LABEL_COLOR)));
        row2_spans.push(Span::styled(
            model.clone(),
            Style::default().fg(Color::Cyan),
        ));
        row2_spans.push(Span::raw("  "));
    }

    // Duration
    let duration_display = format_duration(meta.duration_secs);
    row2_spans.push(Span::styled("Duration: ", Style::default().fg(LABEL_COLOR)));
    row2_spans.push(Span::styled(
        duration_display,
        Style::default().fg(Color::White),
    ));

    lines.push(Line::from(row2_spans));

    // Row 3: Msgs | Agents | Tools | Plans
    let tools_display = format_tool_stats(&meta.tool_stats);
    lines.push(Line::from(vec![
        Span::styled("Msgs: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(
            meta.message_count.to_string(),
            Style::default().fg(Color::White),
        ),
        Span::raw("  "),
        Span::styled("Agents: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(
            meta.agent_count.to_string(),
            Style::default().fg(Color::White),
        ),
        Span::raw("  "),
        Span::styled("Tools: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(tools_display, Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("Plans: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(
            meta.plan_count.to_string(),
            Style::default().fg(Color::Magenta),
        ),
        Span::styled(" (p)", Style::default().fg(Color::DarkGray)),
    ]));

    // Row 4: Files modified
    let files_display = format_file_stats(&meta.file_stats);
    lines.push(Line::from(vec![
        Span::styled("Files: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(files_display, Style::default().fg(Color::White)),
    ]));

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER_INFO))
            .title(" Session Info ")
            .title_style(Style::default().fg(BORDER_INFO).bold()),
    );
    frame.render_widget(paragraph, area);
}

/// Render the analytics panel showing thread-level analytics.
fn render_analytics_panel(frame: &mut Frame, app: &App, area: Rect) {
    // Check for error state
    if let Some(ref error) = app.thread_analytics_error {
        let error_line = Line::from(vec![
            Span::styled("Error: ", Style::default().fg(Color::Red)),
            Span::styled(
                truncate_string(error, 60),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        let paragraph = Paragraph::new(error_line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Red))
                .title(" Thread Analytics ")
                .title_style(Style::default().fg(Color::Red)),
        );
        frame.render_widget(paragraph, area);
        return;
    }

    // Check if we have analytics
    if app.thread_analytics.is_none() {
        let placeholder = Paragraph::new("No analytics computed")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(BORDER_INFO))
                    .title(" Thread Analytics ")
                    .title_style(Style::default().fg(BORDER_INFO)),
            );
        frame.render_widget(placeholder, area);
        return;
    }

    let lines = build_thread_analytics_lines(app);

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER_INFO))
            .title(" Thread Analytics ")
            .title_style(Style::default().fg(BORDER_INFO).bold()),
    );
    frame.render_widget(paragraph, area);
}

/// Build lines for session analytics display.
fn build_session_analytics_lines(app: &App) -> Vec<Line<'static>> {
    let analytics = match &app.session_analytics {
        Some(a) => a,
        None => return vec![Line::from("No data")],
    };

    let mut lines = Vec::new();

    // Line 1: Edits, files, churn
    let mut line1_spans: Vec<Span> = Vec::new();
    line1_spans.push(Span::styled("Edits: ", Style::default().fg(LABEL_COLOR)));
    line1_spans.push(Span::styled(
        format!("{}", analytics.edit_count),
        Style::default().fg(Color::White),
    ));
    line1_spans.push(Span::styled(
        format!(" ({} files)", analytics.unique_files),
        Style::default().fg(Color::DarkGray),
    ));
    line1_spans.push(Span::raw("  "));

    let (churn_color, churn_label) = churn_level(analytics.churn_ratio);
    let churn_pct = (analytics.churn_ratio * 100.0).round() as i64;
    line1_spans.push(Span::styled("Churn: ", Style::default().fg(LABEL_COLOR)));
    line1_spans.push(Span::styled(
        format!("{}%", churn_pct),
        Style::default().fg(churn_color),
    ));
    line1_spans.push(Span::styled(
        format!(" [{}]", churn_label),
        Style::default()
            .fg(churn_color)
            .add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::from(line1_spans));

    // Line 2: Hot files
    let mut line2_spans: Vec<Span> = Vec::new();
    if !analytics.high_churn_files.is_empty() {
        line2_spans.push(Span::styled(
            "Hot files: ",
            Style::default().fg(LABEL_COLOR),
        ));
        let max_files = 4;
        let files_to_show: Vec<&str> = analytics
            .high_churn_files
            .iter()
            .take(max_files)
            .map(|p| extract_basename(p))
            .collect();
        line2_spans.push(Span::styled(
            files_to_show.join(", "),
            Style::default().fg(Color::Yellow),
        ));
        let remaining = analytics.high_churn_files.len().saturating_sub(max_files);
        if remaining > 0 {
            line2_spans.push(Span::styled(
                format!(" +{}", remaining),
                Style::default().fg(Color::DarkGray),
            ));
        }
    } else {
        line2_spans.push(Span::styled(
            "No hot files detected",
            Style::default().fg(Color::DarkGray),
        ));
    }
    lines.push(Line::from(line2_spans));

    // Line 3: Additional session info
    let line3_spans = vec![Span::styled(
        "Session-level aggregate across all threads",
        Style::default().fg(Color::DarkGray).italic(),
    )];
    lines.push(Line::from(line3_spans));

    lines
}

/// Build lines for thread analytics display.
fn build_thread_analytics_lines(app: &App) -> Vec<Line<'static>> {
    let analytics = match &app.thread_analytics {
        Some(a) => a,
        None => return vec![Line::from("No data")],
    };

    let mut lines = Vec::new();

    // Line 1: Edits, files, churn
    let mut line1_spans: Vec<Span> = Vec::new();
    line1_spans.push(Span::styled("Edits: ", Style::default().fg(LABEL_COLOR)));
    line1_spans.push(Span::styled(
        format!("{}", analytics.edit_count),
        Style::default().fg(Color::White),
    ));
    line1_spans.push(Span::styled(
        format!(" ({} files)", analytics.unique_files),
        Style::default().fg(Color::DarkGray),
    ));
    line1_spans.push(Span::raw("  "));

    let (churn_color, churn_label) = churn_level(analytics.churn_ratio);
    let churn_pct = (analytics.churn_ratio * 100.0).round() as i64;
    line1_spans.push(Span::styled("Churn: ", Style::default().fg(LABEL_COLOR)));
    line1_spans.push(Span::styled(
        format!("{}%", churn_pct),
        Style::default().fg(churn_color),
    ));
    line1_spans.push(Span::styled(
        format!(" [{}]", churn_label),
        Style::default()
            .fg(churn_color)
            .add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::from(line1_spans));

    // Line 2: Lines changed and first-try rate
    let mut line2_spans: Vec<Span> = Vec::new();
    line2_spans.push(Span::styled("Lines: ", Style::default().fg(LABEL_COLOR)));
    line2_spans.push(Span::styled(
        format!("{}", analytics.lines_changed),
        Style::default().fg(Color::White),
    ));
    line2_spans.push(Span::raw("  "));
    line2_spans.push(Span::styled(
        "First-try: ",
        Style::default().fg(LABEL_COLOR),
    ));
    let first_try_pct = (analytics.first_try_rate * 100.0).round() as i64;
    let first_try_color = if first_try_pct >= 70 {
        Color::Green
    } else if first_try_pct >= 40 {
        Color::Yellow
    } else {
        Color::Red
    };
    line2_spans.push(Span::styled(
        format!("{}%", first_try_pct),
        Style::default().fg(first_try_color),
    ));
    if analytics.burst_edit_count > 0 {
        line2_spans.push(Span::raw("  "));
        line2_spans.push(Span::styled("Bursts: ", Style::default().fg(LABEL_COLOR)));
        line2_spans.push(Span::styled(
            format!("{}", analytics.burst_edit_count),
            Style::default().fg(Color::Red),
        ));
    }
    lines.push(Line::from(line2_spans));

    // Line 3: Hot files for this thread
    let mut line3_spans: Vec<Span> = Vec::new();
    if !analytics.high_churn_files.is_empty() {
        line3_spans.push(Span::styled("Hot: ", Style::default().fg(LABEL_COLOR)));
        let max_files = 5;
        let files_to_show: Vec<&str> = analytics
            .high_churn_files
            .iter()
            .take(max_files)
            .map(|p| extract_basename(p))
            .collect();
        line3_spans.push(Span::styled(
            files_to_show.join(", "),
            Style::default().fg(Color::Yellow),
        ));
        let remaining = analytics.high_churn_files.len().saturating_sub(max_files);
        if remaining > 0 {
            line3_spans.push(Span::styled(
                format!(" +{}", remaining),
                Style::default().fg(Color::DarkGray),
            ));
        }
    } else {
        line3_spans.push(Span::styled(
            "No hot files in this thread",
            Style::default().fg(Color::DarkGray),
        ));
    }
    lines.push(Line::from(line3_spans));

    lines
}

/// Get color and label for churn ratio.
fn churn_level(ratio: f64) -> (Color, &'static str) {
    if ratio <= 0.3 {
        (Color::Green, "LOW")
    } else if ratio <= 0.6 {
        (Color::Yellow, "MOD")
    } else {
        (Color::Red, "HIGH")
    }
}

/// Extract the basename from a file path.
fn extract_basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Truncate a string to max length with ellipsis.
/// Handles multi-byte UTF-8 characters safely.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find a valid char boundary at or before max_len - 3 (for "...")
        let target = max_len.saturating_sub(3);
        let mut end = target;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
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
                    format!(".../{}", parts[parts.len() - 3..].join("/"))
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

/// Format a message timestamp as local time HH:MM (24-hour compact).
fn format_message_time(ts: DateTime<Utc>) -> String {
    ts.with_timezone(&Local).format("%H:%M").to_string()
}

/// Format duration between two timestamps for thread group headers.
fn format_group_duration(first: DateTime<Utc>, last: DateTime<Utc>) -> String {
    let secs = (last - first).num_seconds().max(0);
    if secs == 0 {
        "<1 second".to_string()
    } else if secs == 1 {
        "1 second".to_string()
    } else if secs < 60 {
        format!("{} seconds", secs)
    } else if secs < 3600 {
        let mins = secs / 60;
        let remaining_secs = secs % 60;
        if remaining_secs > 0 {
            format!("{} min {} sec", mins, remaining_secs)
        } else if mins == 1 {
            "1 minute".to_string()
        } else {
            format!("{} minutes", mins)
        }
    } else {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        if mins > 0 {
            format!("{} hr {} min", hours, mins)
        } else if hours == 1 {
            "1 hour".to_string()
        } else {
            format!("{} hours", hours)
        }
    }
}

/// Format tool stats for display.
fn format_tool_stats(stats: &aiobscura_core::db::ToolStats) -> String {
    if stats.total_calls == 0 {
        return "0".to_string();
    }

    let top_tools: Vec<String> = stats
        .breakdown
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
    let top_files: Vec<String> = stats
        .breakdown
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
        let extra = if stats.breakdown.len() > 3 {
            " ..."
        } else {
            ""
        };
        format!(
            "{} modified ({}{})",
            stats.total_files,
            top_files.join(", "),
            extra
        )
    }
}

/// Render the threads table.
fn render_table(frame: &mut Frame, app: &mut App, area: Rect) {
    let header_cells = [
        "Last Updated",
        "Thread ID",
        "Project",
        "Assistant",
        "Type",
        "Msgs",
    ]
    .into_iter()
    .map(|h| Cell::from(h).style(Style::default().fg(Color::Yellow).bold()));
    let header = Row::new(header_cells).height(1);

    let rows = app.threads.iter().map(|thread| {
        // Create styled type cell with badge and tree chars
        let (badge, type_text, color) = match thread.thread_type {
            ThreadType::Main => ("●", "main", BADGE_MAIN),
            ThreadType::Agent => ("◎", "agent", BADGE_AGENT),
            ThreadType::Background => ("◇", "bg", BADGE_BG),
        };

        // Use tree-drawing characters for hierarchy (single space indent)
        let tree_prefix = if thread.indent_level > 0 {
            if thread.is_last_child {
                "└"
            } else {
                "├"
            }
        } else {
            ""
        };

        let type_cell = Cell::from(Line::from(vec![
            Span::styled(tree_prefix, Style::default().fg(SEPARATOR_COLOR)),
            Span::styled(format!("{} ", badge), Style::default().fg(color)),
            Span::styled(type_text, Style::default().fg(color)),
        ]));

        // Color-code message count (high activity = brighter)
        let msg_style = if thread.message_count > 100 {
            Style::default().fg(Color::Yellow)
        } else if thread.message_count > 50 {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        Row::new([
            Cell::from(thread.relative_time()),
            Cell::from(thread.short_id()),
            Cell::from(thread.project_name.as_str()),
            Cell::from(thread.assistant_name.as_str()),
            type_cell,
            Cell::from(thread.message_count.to_string()).style(msg_style),
        ])
    });

    let widths = [
        Constraint::Length(12), // Last Updated
        Constraint::Length(10), // Thread ID
        Constraint::Fill(1),    // Project (flexible)
        Constraint::Length(12), // Assistant
        Constraint::Length(10), // Type (with indent space)
        Constraint::Length(6),  // Msgs
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Threads "),
        )
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
    let total = app.messages.len();

    for (idx, msg) in app.messages.iter().enumerate() {
        // Add separator before each message (except first)
        if idx > 0 {
            let separator = "─".repeat(40);
            lines.push(Line::from(Span::styled(
                separator,
                Style::default().fg(SEPARATOR_COLOR),
            )));
        }

        let msg_lines = format_message(msg, idx + 1, total);
        lines.extend(msg_lines);
        lines.push(Line::raw("")); // Blank line after content
    }

    // Clamp scroll offset
    let max_scroll = lines.len().saturating_sub(area.height as usize);
    if app.scroll_offset > max_scroll {
        app.scroll_offset = max_scroll;
    }

    let paragraph = Paragraph::new(lines.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER_MESSAGES))
                .title(" Messages ")
                .title_style(Style::default().fg(BORDER_MESSAGES).bold()),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);

    // Render scrollbar
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state = ScrollbarState::new(lines.len()).position(app.scroll_offset);

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
fn format_message(msg: &Message, index: usize, total: usize) -> Vec<Line<'static>> {
    let (icon, label, style) = match msg.message_type {
        MessageType::Prompt => ("💬", "Human", Style::default().fg(Color::Cyan).bold()),
        MessageType::Response => ("🤖", "Assistant", Style::default().fg(Color::Green)),
        MessageType::ToolCall => {
            let name = msg.tool_name.as_deref().unwrap_or("unknown");
            return format_tool_message(name, msg, index, total);
        }
        MessageType::ToolResult => ("📋", "Result", Style::default().fg(Color::DarkGray)),
        MessageType::Error => ("❌", "Error", Style::default().fg(Color::Red)),
        MessageType::Plan => ("📝", "Plan", Style::default().fg(Color::Magenta)),
        MessageType::Summary => ("📊", "Summary", Style::default().fg(Color::Blue)),
        MessageType::Context => ("📎", "Context", Style::default().fg(Color::DarkGray)),
    };

    let mut lines = Vec::new();

    // Format timestamp
    let time_str = format_message_time(msg.ts);

    // Header line with icon, label, index, and timestamp
    let counter = format!("[{}/{}]", index, total);
    // Calculate padding: icon (2) + space + label + space + counter
    let header_len = 2 + 1 + label.chars().count() + 1 + counter.chars().count();
    let target_width: usize = 50;
    let padding_needed = target_width.saturating_sub(header_len);
    let padding = " ".repeat(padding_needed);

    lines.push(Line::from(vec![
        Span::raw(format!("{} ", icon)),
        Span::styled(label, style),
        Span::styled(
            format!(" {}", counter),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(padding),
        Span::styled(time_str, Style::default().fg(Color::DarkGray)),
    ]));

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

/// Format a tool call message with special handling for the tool name.
fn format_tool_message(
    tool_name: &str,
    msg: &Message,
    index: usize,
    total: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Format timestamp
    let time_str = format_message_time(msg.ts);

    let counter = format!("[{}/{}]", index, total);
    // Calculate padding: icon (2) + space + "Tool: " (6) + tool_name + space + counter
    let header_len = 2 + 1 + 6 + tool_name.chars().count() + 1 + counter.chars().count();
    let target_width: usize = 50;
    let padding_needed = target_width.saturating_sub(header_len);
    let padding = " ".repeat(padding_needed);

    lines.push(Line::from(vec![
        Span::raw("🔧 "),
        Span::styled("Tool: ", Style::default().fg(Color::Yellow)),
        Span::styled(
            tool_name.to_string(),
            Style::default().fg(Color::Yellow).bold(),
        ),
        Span::styled(
            format!(" {}", counter),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(padding),
        Span::styled(time_str, Style::default().fg(Color::DarkGray)),
    ]));

    // Content
    let content = get_message_content(msg);
    if !content.is_empty() {
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
    let selected = app.table_state.selected().map(|i| i + 1).unwrap_or(0);

    let mut footer_spans = vec![
        Span::styled(" Tab", Style::default().fg(Color::Yellow)),
        Span::raw(" projects  "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" open  "),
        Span::styled("w", Style::default().fg(Color::Yellow)),
        Span::raw(" wrapped  "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(" navigate  "),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(" quit  "),
        Span::raw("│ "),
        Span::styled(
            format!("{}/{} threads", selected, thread_count),
            Style::default().fg(Color::DarkGray),
        ),
    ];

    // Show live indicator when new data was recently detected
    if app.should_show_live_indicator() {
        footer_spans.push(Span::raw(" │ "));
        footer_spans.push(Span::styled(
            "● LIVE",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let footer = Line::from(footer_spans);
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
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(BORDER_PLAN))
                    .title(" Plans ")
                    .title_style(Style::default().fg(BORDER_PLAN).bold()),
            );
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
        Constraint::Length(20), // Slug
        Constraint::Fill(1),    // Title (flexible)
        Constraint::Length(12), // Status
        Constraint::Length(12), // Modified
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER_PLAN))
                .title(" Plans ")
                .title_style(Style::default().fg(BORDER_PLAN).bold()),
        )
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(Color::Magenta),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut app.plan_table_state);
}

/// Render plan content with markdown-aware styling.
fn render_plan_content(frame: &mut Frame, app: &mut App, area: Rect) {
    let content = match &app.selected_plan {
        Some(plan) => plan.content.as_deref().unwrap_or("(empty plan)"),
        None => "(no plan selected)",
    };

    // Parse markdown-style content for styling
    let mut lines: Vec<Line> = Vec::new();
    let mut in_code_block = false;

    for line in content.lines() {
        let styled_line = if line.starts_with("```") {
            // Toggle code block state
            in_code_block = !in_code_block;
            Line::from(Span::styled(line.to_string(), Style::default().fg(MD_CODE)))
        } else if in_code_block {
            // Code block content
            Line::from(Span::styled(line.to_string(), Style::default().fg(MD_CODE)))
        } else if line.starts_with("# ") {
            // H1 header
            Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(MD_HEADER).bold(),
            ))
        } else if line.starts_with("## ") {
            // H2 header
            Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(MD_HEADER).bold(),
            ))
        } else if line.starts_with("### ") {
            // H3 header
            Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(MD_HEADER),
            ))
        } else if line.starts_with("**") && line.ends_with("**") {
            // Bold line (like **File:** ...)
            Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(Color::Yellow),
            ))
        } else if line.starts_with("- ") || line.starts_with("* ") {
            // Bullet points
            Line::from(vec![
                Span::styled("• ", Style::default().fg(BORDER_PLAN)),
                Span::raw(line[2..].to_string()),
            ])
        } else {
            Line::raw(line.to_string())
        };
        lines.push(styled_line);
    }

    // Clamp scroll offset
    let max_scroll = lines
        .len()
        .saturating_sub(area.height.saturating_sub(2) as usize);
    if app.plan_scroll_offset > max_scroll {
        app.plan_scroll_offset = max_scroll;
    }

    let paragraph = Paragraph::new(lines.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER_PLAN))
                .title(" Content ")
                .title_style(Style::default().fg(BORDER_PLAN).bold()),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.plan_scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);

    // Render scrollbar
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state = ScrollbarState::new(lines.len()).position(app.plan_scroll_offset);

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
    let selected = app.plan_table_state.selected().map(|i| i + 1).unwrap_or(0);

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

    // Render snowflakes in background first
    render_snowflakes(frame, app, area);

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

    // Header with period and holiday flair
    let title = format!("🎄 AI Wrapped - {} 🎄", stats.period.display_name());
    render_wrapped_header(frame, &title, app.animation_frame, chunks[0]);

    // Render the current card
    render_wrapped_card(frame, stats, app.wrapped_card_index, chunks[1]);

    // Footer
    render_wrapped_footer(frame, app, chunks[2]);
}

/// Render falling snowflakes in the background.
fn render_snowflakes(frame: &mut Frame, app: &App, area: Rect) {
    // Snowflake characters with varying "weights"
    let snowflake_chars = ['❄', '❅', '❆', '✦', '·', '•', '*'];

    for (i, (x, y, speed)) in app.snowflakes.iter().enumerate() {
        // Skip if outside the render area
        if *x >= area.width || *y >= area.height {
            continue;
        }

        // Pick snowflake character based on index and speed
        let char_idx = (i + *speed as usize) % snowflake_chars.len();
        let flake = snowflake_chars[char_idx];

        // Color based on speed (faster = dimmer, gives depth effect)
        let color = match speed {
            1 => WRAPPED_WHITE,
            2 => WRAPPED_SILVER,
            _ => WRAPPED_DIM,
        };

        // Twinkle effect - some snowflakes blink
        let visible = if i % 5 == 0 {
            !app.animation_frame.is_multiple_of(4)
        } else {
            true
        };

        if visible {
            let span = Span::styled(flake.to_string(), Style::default().fg(color));
            let paragraph = Paragraph::new(span);
            let snowflake_area = Rect::new(*x, *y, 1, 1);
            frame.render_widget(paragraph, snowflake_area);
        }
    }
}

/// Render the wrapped header with animated decorations.
fn render_wrapped_header(frame: &mut Frame, title: &str, animation_frame: u64, area: Rect) {
    // Animated border characters for twinkling effect
    let twinkle_chars = ['✨', '⭐', '🌟', '💫', '✧', '✦'];
    let twinkle_idx = (animation_frame / 3) as usize % twinkle_chars.len();
    let twinkle = twinkle_chars[twinkle_idx];

    // Build the header with twinkling decorations
    let header_line = Line::from(vec![
        Span::styled(format!(" {} ", twinkle), Style::default().fg(WRAPPED_GOLD)),
        Span::styled(title, Style::default().fg(WRAPPED_CYAN).bold()),
        Span::styled(format!(" {} ", twinkle), Style::default().fg(WRAPPED_GOLD)),
    ]);

    let header = Paragraph::new(header_line)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::BOTTOM));

    frame.render_widget(header, area);
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
            format!(
                "YOUR {} AI WRAPPED",
                stats.period.display_name().to_uppercase()
            ),
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
        .title(Span::styled(
            " ★ The Numbers ★ ",
            Style::default().fg(WRAPPED_GOLD).bold(),
        ));

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
        let max_count = stats
            .tools
            .top_tools
            .iter()
            .map(|(_, c, _)| *c)
            .max()
            .unwrap_or(1);

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
                Span::styled(
                    format!("{:<10}", name),
                    Style::default().fg(WRAPPED_WHITE).bold(),
                ),
                Span::styled(
                    format!("{:>6} ", count),
                    Style::default().fg(rank_color).bold(),
                ),
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
        .title(Span::styled(
            " 🏆 Top Tools ",
            Style::default().fg(WRAPPED_GOLD).bold(),
        ));

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
        .title(Span::styled(
            " ⏰ Time Patterns ",
            Style::default().fg(WRAPPED_PURPLE).bold(),
        ));

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
    let sparkline_data: Vec<u64> = stats
        .time_patterns
        .hourly_distribution
        .iter()
        .map(|&x| x as u64)
        .collect();

    let sparkline_chunks = Layout::vertical([
        Constraint::Length(1), // Label
        Constraint::Length(2), // Sparkline
        Constraint::Length(1), // Time labels
    ])
    .split(chunks[1]);

    let label = Paragraph::new(Line::from(vec![Span::styled(
        "   Activity by hour: ",
        Style::default().fg(WRAPPED_DIM),
    )]));
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

    let time_labels = Paragraph::new(Line::from(vec![Span::styled(
        "   0h        6h        12h       18h       23h",
        Style::default().fg(WRAPPED_DIM),
    )]));
    frame.render_widget(time_labels, sparkline_chunks[2]);
}

/// Render the streaks card with gauge visualization.
fn render_wrapped_streaks_card(frame: &mut Frame, stats: &WrappedStats, area: Rect) {
    // Create block and get inner area
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(WRAPPED_CORAL))
        .title(Span::styled(
            " 🔥 Streaks ",
            Style::default().fg(WRAPPED_CORAL).bold(),
        ));

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
            format!(
                " day{}",
                if stats.streaks.current_streak_days == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            Style::default().fg(WRAPPED_WHITE),
        ),
        Span::styled(fire_emoji, Style::default()),
    ]));

    // Longest streak with celebration
    if stats.streaks.longest_streak_days > 0 {
        let streak_dates = match (
            &stats.streaks.longest_streak_start,
            &stats.streaks.longest_streak_end,
        ) {
            (Some(start), Some(end)) => {
                format!(
                    " ({} – {})",
                    start.with_timezone(&Local).format("%b %d"),
                    end.with_timezone(&Local).format("%b %d")
                )
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
                format!(
                    " day{}",
                    if stats.streaks.longest_streak_days == 1 {
                        ""
                    } else {
                        "s"
                    }
                ),
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
            let filled =
                (((project.tokens as f64 / max_tokens as f64) * bar_width as f64) as usize).max(1);
            let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);

            // Rank indicator with special treatment for #1
            let (rank_indicator, name_color, bar_color) = match i {
                0 => ("  🏆 ", WRAPPED_GOLD, WRAPPED_GOLD),
                1 => ("   2 ", WRAPPED_SILVER, WRAPPED_SILVER),
                2 => ("   3 ", WRAPPED_BRONZE, WRAPPED_BRONZE),
                _ => (
                    if i == 3 { "   4 " } else { "   5 " },
                    WRAPPED_DIM,
                    WRAPPED_DIM,
                ),
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
        .title(Span::styled(
            " 📁 Top Projects ",
            Style::default().fg(WRAPPED_LIME).bold(),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Render the trends comparison card with arrows and visual impact.
fn render_wrapped_trends_card(frame: &mut Frame, stats: &WrappedStats, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if let Some(trends) = &stats.trends {
        lines.push(Line::from(vec![Span::styled(
            "   Compared to previous period:",
            Style::default().fg(WRAPPED_DIM),
        )]));
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
        lines.push(Line::from(vec![Span::styled(
            format!("   {}", message.0),
            Style::default().fg(message.1),
        )]));
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
        .title(Span::styled(
            " 📈 vs Previous Period ",
            Style::default().fg(WRAPPED_CYAN).bold(),
        ));

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
        let tagline = Paragraph::new(Line::from(vec![Span::styled(
            format!("\"{}\"", personality.tagline()),
            Style::default().fg(WRAPPED_WHITE).italic(),
        )]))
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
        Span::styled(" cards  ", Style::default().fg(WRAPPED_DIM)),
        Span::styled("j/k", Style::default().fg(WRAPPED_GOLD)),
        Span::styled(" months  ", Style::default().fg(WRAPPED_DIM)),
        Span::styled("m", Style::default().fg(WRAPPED_GOLD)),
        Span::styled(
            format!(" {} ", period_hint),
            Style::default().fg(WRAPPED_DIM),
        ),
        Span::styled("│ ", Style::default().fg(WRAPPED_DIM)),
    ];
    footer_spans.extend(dots);

    let footer = Line::from(footer_spans);
    frame.render_widget(Paragraph::new(footer), area);
}

// ========== Project Views ==========

// ========== Dashboard Panel ==========

/// Render the dashboard panel with activity heatmap and stats.
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

/// Render the project list view.
fn render_project_list_view(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Layout: tab header, dashboard panel, table, footer
    let chunks = Layout::vertical([
        Constraint::Length(2), // Tab header
        Constraint::Length(7), // Dashboard panel (heatmap + stats)
        Constraint::Min(5),    // Table
        Constraint::Length(1), // Footer
    ])
    .split(area);

    render_tab_header(frame, ActiveTab::Projects, chunks[0]);
    render_dashboard_panel(frame, app, chunks[1]);
    render_project_table(frame, app, chunks[2]);
    render_project_list_footer(frame, app, chunks[3]);
}

/// Render the project detail view.
fn render_project_detail_view(
    frame: &mut Frame,
    app: &mut App,
    project_name: String,
    sub_tab: ProjectSubTab,
) {
    let area = frame.area();

    // Layout: header, sub-tabs, content, footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Length(2), // Sub-tab bar
        Constraint::Min(10),   // Content
        Constraint::Length(1), // Footer
    ])
    .split(area);

    render_header(frame, &format!("Project: {}", project_name), chunks[0]);
    render_project_sub_tabs(frame, sub_tab, chunks[1]);

    // Render content based on sub-tab
    match sub_tab {
        ProjectSubTab::Overview => {
            render_project_overview_content(frame, app, chunks[2]);
        }
        ProjectSubTab::Sessions => {
            render_project_sessions_content(frame, app, chunks[2]);
        }
        ProjectSubTab::Plans => {
            render_project_plans_content(frame, app, chunks[2]);
        }
        ProjectSubTab::Files => {
            render_project_files_content(frame, app, chunks[2]);
        }
    }

    render_project_detail_footer(frame, sub_tab, chunks[3]);
}

/// Render the projects table.
fn render_project_table(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.projects.is_empty() {
        let empty_msg = Paragraph::new("No projects found")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(BORDER_PROJECT))
                    .title(" Projects ")
                    .title_style(Style::default().fg(BORDER_PROJECT).bold()),
            );
        frame.render_widget(empty_msg, area);
        return;
    }

    let header_cells = ["Project", "Path", "Sessions", "Tokens", "Active"]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().fg(Color::Yellow).bold()));
    let header = Row::new(header_cells).height(1);

    let rows = app.projects.iter().map(|project| {
        // Format the path (truncate and replace home)
        let home = std::env::var("HOME").unwrap_or_default();
        let path_display = if !home.is_empty() && project.path.starts_with(&home) {
            format!("~{}", &project.path[home.len()..])
        } else {
            project.path.clone()
        };
        // Truncate path if too long
        let path_display = if path_display.len() > 30 {
            format!("...{}", &path_display[path_display.len() - 27..])
        } else {
            path_display
        };

        // Format tokens
        let tokens_display = format_tokens(project.total_tokens);

        // Format last activity
        let active_display = project
            .last_activity
            .map(format_relative_time)
            .unwrap_or_else(|| "—".to_string());

        Row::new([
            Cell::from(project.name.as_str()),
            Cell::from(path_display).style(Style::default().fg(Color::DarkGray)),
            Cell::from(project.session_count.to_string()),
            Cell::from(tokens_display).style(Style::default().fg(WRAPPED_CYAN)),
            Cell::from(active_display),
        ])
    });

    let widths = [
        Constraint::Fill(1),    // Project name (flexible)
        Constraint::Length(32), // Path
        Constraint::Length(10), // Sessions
        Constraint::Length(10), // Tokens
        Constraint::Length(12), // Active
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER_PROJECT))
                .title(" Projects ")
                .title_style(Style::default().fg(BORDER_PROJECT).bold()),
        )
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(BORDER_PROJECT),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut app.project_table_state);
}

/// Render the project overview section.
fn render_project_overview(
    frame: &mut Frame,
    stats: &aiobscura_core::analytics::ProjectStats,
    area: Rect,
) {
    let mut lines: Vec<Line> = Vec::new();

    // Row 1: Path
    let home = std::env::var("HOME").unwrap_or_default();
    let path_display = if !home.is_empty() && stats.path.starts_with(&home) {
        format!("~{}", &stats.path[home.len()..])
    } else {
        stats.path.clone()
    };
    lines.push(Line::from(vec![
        Span::styled("Path: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(path_display, Style::default().fg(Color::White)),
    ]));

    // Row 2: First Session | Last Active
    let first_session = stats
        .first_session
        .map(|ts| {
            ts.with_timezone(&chrono::Local)
                .format("%b %d, %Y")
                .to_string()
        })
        .unwrap_or_else(|| "—".to_string());
    let last_active = stats
        .last_activity
        .map(format_relative_time)
        .unwrap_or_else(|| "—".to_string());

    lines.push(Line::from(vec![
        Span::styled("First Session: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(first_session, Style::default().fg(Color::White)),
        Span::raw("    "),
        Span::styled("Last Active: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(last_active, Style::default().fg(Color::White)),
    ]));

    // Row 3: Total Time | Sessions
    lines.push(Line::from(vec![
        Span::styled("Total Time: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(stats.formatted_duration(), Style::default().fg(Color::Cyan)),
        Span::raw("    "),
        Span::styled("Sessions: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(
            stats.session_count.to_string(),
            Style::default().fg(Color::White),
        ),
    ]));

    // Row 4: Tokens | Agents | Plans
    let tokens_display = format!(
        "{} in / {} out",
        format_tokens(stats.tokens_in),
        format_tokens(stats.tokens_out)
    );
    lines.push(Line::from(vec![
        Span::styled("Tokens: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(tokens_display, Style::default().fg(WRAPPED_CYAN)),
        Span::raw("    "),
        Span::styled("Agents: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(
            stats.agents_spawned.to_string(),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("    "),
        Span::styled("Plans: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(
            stats.plans_created.to_string(),
            Style::default().fg(Color::Magenta),
        ),
    ]));

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER_PROJECT))
            .title(" Overview ")
            .title_style(Style::default().fg(BORDER_PROJECT).bold()),
    );
    frame.render_widget(paragraph, area);
}

/// Render the project activity section with sparkline.
fn render_project_activity(
    frame: &mut Frame,
    stats: &aiobscura_core::analytics::ProjectStats,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_PROJECT))
        .title(" Activity ")
        .title_style(Style::default().fg(BORDER_PROJECT).bold());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split inner area
    let chunks = Layout::vertical([
        Constraint::Length(3), // Hourly sparkline
        Constraint::Length(2), // Daily breakdown
        Constraint::Min(1),    // Peak hour info
    ])
    .split(inner);

    // Hourly sparkline
    let sparkline_data: Vec<u64> = stats
        .hourly_distribution
        .iter()
        .map(|&x| x as u64)
        .collect();
    let sparkline = Sparkline::default()
        .data(&sparkline_data)
        .style(Style::default().fg(WRAPPED_CYAN))
        .bar_set(symbols::bar::NINE_LEVELS);

    let sparkline_label = Paragraph::new(Line::from(vec![Span::styled(
        "By Hour: ",
        Style::default().fg(LABEL_COLOR),
    )]));
    let label_area = Rect {
        height: 1,
        ..chunks[0]
    };
    let sparkline_area = Rect {
        y: chunks[0].y + 1,
        height: 2,
        x: chunks[0].x + 9,
        width: chunks[0].width.saturating_sub(10),
    };
    frame.render_widget(sparkline_label, label_area);
    frame.render_widget(sparkline, sparkline_area);

    // Daily breakdown (simple bar representation)
    let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let max_daily = stats.daily_distribution.iter().max().copied().unwrap_or(1) as f64;
    let mut day_spans: Vec<Span> = vec![Span::styled("By Day: ", Style::default().fg(LABEL_COLOR))];

    for (i, &count) in stats.daily_distribution.iter().enumerate() {
        let intensity = (count as f64 / max_daily * 4.0) as usize;
        let bar_char = match intensity {
            0 => "░",
            1 => "▒",
            2 => "▓",
            _ => "█",
        };
        day_spans.push(Span::styled(days[i], Style::default().fg(Color::DarkGray)));
        day_spans.push(Span::styled(
            format!("{} ", bar_char.repeat(2)),
            Style::default().fg(BORDER_PROJECT),
        ));
    }
    let daily_line = Paragraph::new(Line::from(day_spans));
    frame.render_widget(daily_line, chunks[1]);

    // Peak hour info
    let peak_hour = stats.peak_hour();
    let peak_hour_str = format!("{}:00", peak_hour);
    let peak_line = Paragraph::new(Line::from(vec![
        Span::styled("Peak: ", Style::default().fg(LABEL_COLOR)),
        Span::styled(peak_hour_str, Style::default().fg(WRAPPED_GOLD).bold()),
    ]));
    frame.render_widget(peak_line, chunks[2]);
}

/// Render the project tools and files section.
fn render_project_tools_files(
    frame: &mut Frame,
    stats: &aiobscura_core::analytics::ProjectStats,
    area: Rect,
) {
    // Split into tools and files
    let chunks =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    // Top Tools
    let mut tool_lines: Vec<Line> = Vec::new();
    let max_tool_count = stats
        .tool_stats
        .breakdown
        .iter()
        .map(|(_, c)| *c)
        .max()
        .unwrap_or(1);

    for (name, count) in stats.tool_stats.breakdown.iter().take(4) {
        let bar_width = 10;
        let filled = (((*count as f64 / max_tool_count as f64) * bar_width as f64) as usize).max(1);
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);

        tool_lines.push(Line::from(vec![
            Span::styled(format!("{:<8}", name), Style::default().fg(Color::White)),
            Span::styled(bar, Style::default().fg(WRAPPED_GOLD)),
            Span::styled(
                format!(" {:>5}", count),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let tools_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(WRAPPED_GOLD))
        .title(" Top Tools ")
        .title_style(Style::default().fg(WRAPPED_GOLD).bold());

    let tools_para = Paragraph::new(tool_lines).block(tools_block);
    frame.render_widget(tools_para, chunks[0]);

    // Top Files
    let mut file_lines: Vec<Line> = Vec::new();
    let max_file_count = stats
        .file_stats
        .breakdown
        .iter()
        .map(|(_, c)| *c)
        .max()
        .unwrap_or(1);

    for (path, count) in stats.file_stats.breakdown.iter().take(4) {
        let basename = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);

        let bar_width = 8;
        let filled = (((*count as f64 / max_file_count as f64) * bar_width as f64) as usize).max(1);
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);

        // Truncate basename if needed (handle UTF-8 safely)
        let name_display = if basename.len() > 20 {
            let mut end = 17;
            while !basename.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}...", &basename[..end])
        } else {
            basename.to_string()
        };

        file_lines.push(Line::from(vec![
            Span::styled(
                format!("{:<20}", name_display),
                Style::default().fg(Color::White),
            ),
            Span::styled(bar, Style::default().fg(BORDER_PROJECT)),
            Span::styled(
                format!(" {:>3}", count),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let files_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_PROJECT))
        .title(" Top Files ")
        .title_style(Style::default().fg(BORDER_PROJECT).bold());

    let files_para = Paragraph::new(file_lines).block(files_block);
    frame.render_widget(files_para, chunks[1]);
}

/// Render the footer for project list view.
fn render_project_list_footer(frame: &mut Frame, app: &App, area: Rect) {
    let project_count = app.projects.len();
    let selected = app
        .project_table_state
        .selected()
        .map(|i| i + 1)
        .unwrap_or(0);

    let mut footer_spans = vec![
        Span::styled(" Tab", Style::default().fg(Color::Yellow)),
        Span::raw(" threads  "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" details  "),
        Span::styled("w", Style::default().fg(Color::Yellow)),
        Span::raw(" wrapped  "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(" navigate  "),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(" quit  "),
        Span::raw("│ "),
        Span::styled(
            format!("{}/{} projects", selected, project_count),
            Style::default().fg(Color::DarkGray),
        ),
    ];

    // Show live indicator when new data was recently detected
    if app.should_show_live_indicator() {
        footer_spans.push(Span::raw(" │ "));
        footer_spans.push(Span::styled(
            "● LIVE",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let footer = Line::from(footer_spans);
    frame.render_widget(Paragraph::new(footer), area);
}

/// Render the footer for project detail view.
fn render_project_detail_footer(frame: &mut Frame, sub_tab: ProjectSubTab, area: Rect) {
    let mut spans = vec![
        Span::styled(" Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" back  "),
    ];

    // Add context-specific hints
    match sub_tab {
        ProjectSubTab::Overview => {
            // No extra hints for overview
        }
        ProjectSubTab::Sessions | ProjectSubTab::Plans => {
            spans.push(Span::styled("Enter", Style::default().fg(Color::Yellow)));
            spans.push(Span::raw(" open  "));
            spans.push(Span::styled("j/k", Style::default().fg(Color::Yellow)));
            spans.push(Span::raw(" nav  "));
        }
        ProjectSubTab::Files => {
            spans.push(Span::styled("j/k", Style::default().fg(Color::Yellow)));
            spans.push(Span::raw(" nav  "));
        }
    }

    // Tab navigation hints
    spans.push(Span::styled("Tab", Style::default().fg(Color::Yellow)));
    spans.push(Span::raw("/"));
    spans.push(Span::styled("1-4", Style::default().fg(Color::Yellow)));
    spans.push(Span::raw(" tabs  "));
    spans.push(Span::styled("q", Style::default().fg(Color::Yellow)));
    spans.push(Span::raw(" quit"));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Render the project sub-tab bar.
fn render_project_sub_tabs(frame: &mut Frame, active: ProjectSubTab, area: Rect) {
    let make_tab = |label: &str, key: &str, is_active: bool| -> Vec<Span<'static>> {
        let style = if is_active {
            Style::default()
                .fg(BORDER_PROJECT)
                .bold()
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        vec![
            Span::styled(format!(" {}", label), style),
            Span::styled(format!("({})", key), Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
        ]
    };

    let mut spans = Vec::new();
    spans.push(Span::raw(" "));
    spans.extend(make_tab("Overview", "1", active == ProjectSubTab::Overview));
    spans.extend(make_tab("Sessions", "2", active == ProjectSubTab::Sessions));
    spans.extend(make_tab("Plans", "3", active == ProjectSubTab::Plans));
    spans.extend(make_tab("Files", "4", active == ProjectSubTab::Files));

    let tabs = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(tabs, area);
}

/// Render the project overview content (stats, activity, tools).
fn render_project_overview_content(frame: &mut Frame, app: &App, area: Rect) {
    if let Some(stats) = &app.project_stats {
        // Split into overview section and activity/tools section
        let chunks = Layout::vertical([
            Constraint::Length(6), // Overview
            Constraint::Min(5),    // Activity & Tools
        ])
        .split(area);

        render_project_overview(frame, stats, chunks[0]);

        // Split the lower section into activity and tools/files
        let middle_chunks =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[1]);

        render_project_activity(frame, stats, middle_chunks[0]);
        render_project_tools_files(frame, stats, middle_chunks[1]);
    } else {
        let placeholder = Paragraph::new("Loading project stats...")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(placeholder, area);
    }
}

/// Render the project sessions content (table of sessions).
fn render_project_sessions_content(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.project_sessions.is_empty() {
        let empty_msg = Paragraph::new("No sessions found for this project")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(BORDER_PROJECT))
                    .title(" Sessions ")
                    .title_style(Style::default().fg(BORDER_PROJECT).bold()),
            );
        frame.render_widget(empty_msg, area);
        return;
    }

    let header_cells = [
        "Session ID",
        "Last Updated",
        "Duration",
        "Threads",
        "Msgs",
        "Model",
    ]
    .into_iter()
    .map(|h| Cell::from(h).style(Style::default().fg(Color::Yellow).bold()));
    let header = Row::new(header_cells).height(1);

    let rows = app.project_sessions.iter().map(|session| {
        let msg_style = if session.message_count > 500 {
            Style::default().fg(Color::Yellow)
        } else if session.message_count > 100 {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let thread_style = if session.thread_count > 5 {
            Style::default().fg(Color::Yellow)
        } else if session.thread_count > 1 {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let model_display = session
            .model_name
            .as_ref()
            .map(|m| truncate_model_name(m))
            .unwrap_or_else(|| "—".to_string());

        Row::new([
            Cell::from(session.short_id()),
            Cell::from(session.relative_time()),
            Cell::from(session.formatted_duration()),
            Cell::from(session.thread_count.to_string()).style(thread_style),
            Cell::from(session.message_count.to_string()).style(msg_style),
            Cell::from(model_display).style(Style::default().fg(Color::DarkGray)),
        ])
    });

    let widths = [
        Constraint::Length(10), // Session ID
        Constraint::Length(12), // Last Updated
        Constraint::Length(10), // Duration
        Constraint::Length(8),  // Threads
        Constraint::Length(6),  // Msgs
        Constraint::Min(10),    // Model
    ];

    let session_count = app.project_sessions.len();
    let selected = app
        .project_sessions_table_state
        .selected()
        .map(|i| i + 1)
        .unwrap_or(0);

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER_PROJECT))
                .title(format!(" Sessions ({}/{}) ", selected, session_count))
                .title_style(Style::default().fg(BORDER_PROJECT).bold()),
        )
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(BORDER_PROJECT),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut app.project_sessions_table_state);
}

/// Truncate model name for display (e.g., "claude-3-5-sonnet-20241022" -> "sonnet-20241022")
fn truncate_model_name(name: &str) -> String {
    // Try to extract just the model variant and date
    if let Some(pos) = name.rfind("sonnet") {
        return name[pos..].to_string();
    }
    if let Some(pos) = name.rfind("opus") {
        return name[pos..].to_string();
    }
    if let Some(pos) = name.rfind("haiku") {
        return name[pos..].to_string();
    }
    // Fall back to last 15 chars if name is too long
    if name.len() > 15 {
        format!("...{}", &name[name.len() - 12..])
    } else {
        name.to_string()
    }
}

/// Render the project plans content (table of plans).
fn render_project_plans_content(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.project_plans.is_empty() {
        let empty_msg = Paragraph::new("No plans found for this project")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(BORDER_PLAN))
                    .title(" Plans ")
                    .title_style(Style::default().fg(BORDER_PLAN).bold()),
            );
        frame.render_widget(empty_msg, area);
        return;
    }

    let header_cells = ["Slug", "Title", "Status", "Modified"]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().fg(Color::Yellow).bold()));
    let header = Row::new(header_cells).height(1);

    let rows = app.project_plans.iter().map(|plan| {
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
        Constraint::Length(20), // Slug
        Constraint::Fill(1),    // Title (flexible)
        Constraint::Length(12), // Status
        Constraint::Length(12), // Modified
    ];

    let plan_count = app.project_plans.len();
    let selected = app
        .project_plans_table_state
        .selected()
        .map(|i| i + 1)
        .unwrap_or(0);

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER_PLAN))
                .title(format!(" Plans ({}/{}) ", selected, plan_count))
                .title_style(Style::default().fg(BORDER_PLAN).bold()),
        )
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(Color::Magenta),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut app.project_plans_table_state);
}

/// Render the project files content (table of files).
fn render_project_files_content(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.project_files.is_empty() {
        let empty_msg = Paragraph::new("No files modified in this project")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(BORDER_PROJECT))
                    .title(" Files ")
                    .title_style(Style::default().fg(BORDER_PROJECT).bold()),
            );
        frame.render_widget(empty_msg, area);
        return;
    }

    let header_cells = ["File Path", "Edits"]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().fg(Color::Yellow).bold()));
    let header = Row::new(header_cells).height(1);

    let rows = app.project_files.iter().map(|(path, count)| {
        // Format path: replace home dir and show relative
        let home = std::env::var("HOME").unwrap_or_default();
        let path_display = if !home.is_empty() && path.starts_with(&home) {
            format!("~{}", &path[home.len()..])
        } else {
            path.clone()
        };

        Row::new([
            Cell::from(path_display),
            Cell::from(count.to_string()).style(Style::default().fg(WRAPPED_CYAN)),
        ])
    });

    let widths = [
        Constraint::Fill(1),   // File path (flexible)
        Constraint::Length(8), // Edits
    ];

    let file_count = app.project_files.len();
    let selected = app
        .project_files_table_state
        .selected()
        .map(|i| i + 1)
        .unwrap_or(0);

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER_PROJECT))
                .title(format!(" Files ({}/{}) ", selected, file_count))
                .title_style(Style::default().fg(BORDER_PROJECT).bold()),
        )
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(BORDER_PROJECT),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, area, &mut app.project_files_table_state);
}

// ========== Live View ==========

/// Live indicator color (pulsing green)
const LIVE_INDICATOR: Color = Color::Rgb(50, 255, 50);
/// Live view border color
const BORDER_LIVE: Color = Color::Rgb(50, 200, 50);

/// Render the live activity view showing recent messages across all sessions.
fn render_live_view(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Calculate active sessions panel height (show up to 4 sessions, min 2)
    let active_session_count = app.active_sessions.len();
    let sessions_height = (active_session_count.clamp(1, 4) + 2) as u16; // +2 for border

    // Calculate quick projects height (show up to 5 projects)
    let projects_count = app.projects.len().min(5);
    let projects_height = (projects_count.max(2) + 2) as u16; // +2 for border

    // Use the larger of the two for the middle panel
    let middle_panel_height = sessions_height.max(projects_height);

    // Layout: tab header, dashboard summary, middle panels, message stream, footer
    let chunks = Layout::vertical([
        Constraint::Length(2),                   // Tab header
        Constraint::Length(6),                   // Dashboard summary (stats + heatmap)
        Constraint::Length(middle_panel_height), // Quick Projects | Active Sessions
        Constraint::Min(5),                      // Message stream
        Constraint::Length(1),                   // Footer
    ])
    .split(area);

    // === Tab Header ===
    render_tab_header(frame, ActiveTab::Live, chunks[0]);

    // === Dashboard Summary ===
    render_live_dashboard_summary(frame, app, chunks[1]);

    // === Middle Panel: Quick Projects | Active Sessions ===
    let middle_chunks = Layout::horizontal([
        Constraint::Percentage(40), // Quick Projects
        Constraint::Percentage(60), // Active Sessions
    ])
    .split(chunks[2]);

    render_quick_projects_panel(frame, app, middle_chunks[0]);
    render_active_sessions_panel(frame, app, middle_chunks[1]);

    // === Message Stream ===
    render_live_message_stream(frame, app, chunks[3]);

    // === Footer ===
    render_live_footer(frame, app, chunks[4]);
}

/// Render the dashboard summary panel with at-a-glance stats and activity heatmap.
fn render_live_dashboard_summary(frame: &mut Frame, app: &App, area: Rect) {
    // Split into: Stats (left) | Activity Heatmap (right)
    let chunks = Layout::horizontal([
        Constraint::Percentage(50), // At a glance stats
        Constraint::Percentage(50), // Activity heatmap
    ])
    .split(area);

    render_at_a_glance_panel(frame, app, chunks[0]);
    render_live_activity_panel(frame, app, chunks[1]);
}

/// Render the "At a Glance" stats panel.
fn render_at_a_glance_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Dashboard ")
        .title_style(Style::default().fg(LIVE_INDICATOR).bold())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_LIVE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build stats lines
    let mut lines: Vec<Line> = Vec::new();
    let active_count = app.active_sessions.len();

    // Row 1: 24H stats (like a weather map - longer time window first)
    lines.push(Line::from(vec![
        Span::styled("24h    ", Style::default().fg(WRAPPED_DIM)),
        Span::styled(
            format!("{}", app.live_stats_24h.total_messages),
            Style::default().fg(WRAPPED_CYAN).bold(),
        ),
        Span::styled(" msgs  ", Style::default().fg(WRAPPED_DIM)),
        Span::styled(
            format_tokens_short(app.live_stats_24h.total_tokens),
            Style::default().fg(WRAPPED_GOLD).bold(),
        ),
        Span::styled(" tokens", Style::default().fg(WRAPPED_DIM)),
    ]));

    // Row 2: 30m stats (recent activity window)
    lines.push(Line::from(vec![
        Span::styled("30m    ", Style::default().fg(WRAPPED_DIM)),
        Span::styled(
            format!("{}", app.live_stats.total_messages),
            Style::default().fg(WRAPPED_CYAN).bold(),
        ),
        Span::styled(" msgs  ", Style::default().fg(WRAPPED_DIM)),
        Span::styled(
            format_tokens_short(app.live_stats.total_tokens),
            Style::default().fg(WRAPPED_GOLD).bold(),
        ),
        Span::styled(" tokens  ", Style::default().fg(WRAPPED_DIM)),
        Span::styled(
            format!("{}", active_count),
            Style::default().fg(LIVE_INDICATOR).bold(),
        ),
        Span::styled(" active", Style::default().fg(WRAPPED_DIM)),
    ]));

    // Row 3: Streak and peak hour
    if let Some(stats) = &app.dashboard_stats {
        lines.push(Line::from(vec![
            Span::styled("Streak ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("{} days", stats.current_streak),
                Style::default().fg(WRAPPED_CORAL).bold(),
            ),
            Span::styled("  Peak ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                stats.format_peak_hour(),
                Style::default().fg(WRAPPED_PURPLE).bold(),
            ),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render the activity heatmap panel for the live dashboard.
fn render_live_activity_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Activity ")
        .title_style(Style::default().fg(LIVE_INDICATOR).bold())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_LIVE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(stats) = &app.dashboard_stats {
        // Build heatmap line (28 chars for 28 days)
        let heatmap_spans = render_heatmap_spans(&stats.daily_activity);

        // Day labels (4 weeks) - shorter version
        let day_labels = "M T W T F S S  M T W T F S S  M T W T F S S  M T W T F S S";

        // Streak summary line
        let streak_line = vec![
            Span::styled(
                format!("{}d streak", stats.current_streak),
                Style::default().fg(WRAPPED_LIME).bold(),
            ),
            Span::styled("  │  ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format!("Best: {}d", stats.longest_streak),
                Style::default().fg(WRAPPED_GOLD),
            ),
            Span::styled("  │  ", Style::default().fg(WRAPPED_DIM)),
            Span::styled(
                format_tokens_short(stats.total_tokens),
                Style::default().fg(WRAPPED_CYAN),
            ),
            Span::styled(" total", Style::default().fg(WRAPPED_DIM)),
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
        let placeholder =
            Paragraph::new("Loading activity...").style(Style::default().fg(WRAPPED_DIM).italic());
        frame.render_widget(placeholder, inner);
    }
}

/// Render the quick projects panel with numbered shortcuts.
fn render_quick_projects_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Recent Projects [1-5] ")
        .title_style(Style::default().fg(LIVE_INDICATOR).bold())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_LIVE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    if app.projects.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No projects yet",
            Style::default().fg(WRAPPED_DIM).italic(),
        )));
    } else {
        // Show up to 5 projects with number keys
        for (i, project) in app.projects.iter().take(5).enumerate() {
            let key_num = format!("[{}]", i + 1);
            let name = truncate_string(&project.name, 14);
            // Use "sess" instead of "s" to avoid confusion with seconds
            let sessions_str = if project.session_count == 1 {
                "1 sess".to_string()
            } else {
                format!("{} sess", project.session_count)
            };
            let time_str = project
                .last_activity
                .map(format_relative_time)
                .unwrap_or_else(|| "—".to_string());

            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {}", key_num),
                    Style::default().fg(WRAPPED_CYAN).bold(),
                ),
                Span::styled(format!(" {:<14}", name), Style::default().fg(Color::White)),
                Span::styled(
                    format!("{:>7}", sessions_str),
                    Style::default().fg(WRAPPED_DIM),
                ),
                Span::styled(
                    format!(" {:>7}", time_str),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render the active sessions panel showing threads with recent activity.
fn render_active_sessions_panel(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if app.active_sessions.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  No active sessions",
            Style::default().fg(Color::DarkGray).italic(),
        )]));
    } else {
        // Group sessions by parent (main threads vs agent threads)
        // Parent threads have parent_thread_id = None
        let main_threads: Vec<_> = app
            .active_sessions
            .iter()
            .filter(|s| s.parent_thread_id.is_none())
            .collect();

        for main_session in main_threads {
            // Find child agent threads for this main thread
            let child_agents: Vec<_> = app
                .active_sessions
                .iter()
                .filter(|s| s.parent_thread_id.as_ref() == Some(&main_session.thread_id))
                .collect();

            // Render main thread line
            lines.push(format_active_session_line(main_session, false));

            // Render child agents indented
            for agent_session in child_agents {
                lines.push(format_active_session_line(agent_session, true));
            }
        }

        // Also show any orphan agents (parent not in active list)
        let orphan_agents: Vec<_> = app
            .active_sessions
            .iter()
            .filter(|s| {
                if let Some(ref parent_id) = s.parent_thread_id {
                    // Check if parent is NOT in active sessions
                    !app.active_sessions
                        .iter()
                        .any(|p| p.thread_id == *parent_id)
                } else {
                    false
                }
            })
            .collect();

        for agent_session in orphan_agents {
            lines.push(format_active_session_line(agent_session, true));
        }
    }

    let content = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER_LIVE))
            .title(" Active Sessions ")
            .title_style(Style::default().fg(BORDER_LIVE).bold()),
    );

    frame.render_widget(content, area);
}

/// Format a single active session line.
/// `is_child` indicates if this is a child agent (should be indented).
fn format_active_session_line(session: &ActiveSession, is_child: bool) -> Line<'static> {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(session.last_activity);
    let is_idle = duration.num_minutes() >= 5;

    // Activity indicator: ▶ active, ⏸ idle (>5 min)
    let indicator = if is_idle { "⏸" } else { "▶" };
    let indicator_style = if is_idle {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(LIVE_INDICATOR)
    };

    // Indent for child agents
    let prefix = if is_child { "  └─ " } else { " " };

    // Project/thread name
    let thread_label = match session.thread_type {
        ThreadType::Main => format!("{} (main)", session.project_name),
        ThreadType::Agent | ThreadType::Background => {
            // Show truncated thread_id for agents/background threads
            if session.thread_id.len() > 12 {
                // Find valid char boundary (defensive, IDs should be ASCII)
                let mut end = 12;
                while !session.thread_id.is_char_boundary(end) && end > 0 {
                    end -= 1;
                }
                format!("{}...", &session.thread_id[..end])
            } else {
                session.thread_id.clone()
            }
        }
    };

    // Relative time
    let time_str = format_relative_time(session.last_activity);

    // Assistant badge
    let assistant_str = match session.assistant {
        aiobscura_core::Assistant::ClaudeCode => "Claude",
        aiobscura_core::Assistant::Codex => "Codex",
        aiobscura_core::Assistant::Aider => "Aider",
        aiobscura_core::Assistant::Cursor => "Cursor",
    };

    // Message count
    let msg_count_str = format!("+{} msgs", session.message_count);

    // Build the line with proper spacing
    // Format: ▶ project-name (main)    2m ago   Claude    +23 msgs
    let name_style = if is_idle {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };

    Line::from(vec![
        Span::styled(prefix, Style::default().fg(Color::DarkGray)),
        Span::styled(indicator, indicator_style),
        Span::styled(" ", Style::default()),
        Span::styled(format!("{:<24}", thread_label), name_style),
        Span::styled(
            format!("{:>8}", time_str),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled("   ", Style::default()),
        Span::styled(
            format!("{:<8}", assistant_str),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            format!("{:>10}", msg_count_str),
            Style::default().fg(WRAPPED_DIM),
        ),
    ])
}

/// Render the live message stream.
fn render_live_message_stream(frame: &mut Frame, app: &App, area: Rect) {
    // Messages are stored newest-first from DB, displayed newest at top
    let visible_height = area.height.saturating_sub(2) as usize; // Account for borders

    // Calculate which messages to show
    // scroll_offset=0 means show newest messages at top
    // scrolling down (increasing offset) shows older messages
    let total_messages = app.live_messages.len();
    let scroll_offset = app
        .live_scroll_offset
        .min(total_messages.saturating_sub(visible_height));

    // Build lines for display (newest at top)
    let mut lines: Vec<Line> = Vec::new();

    // Iterate in order (newest to oldest for display)
    for msg in app
        .live_messages
        .iter()
        .skip(scroll_offset)
        .take(visible_height)
    {
        lines.push(format_live_message(msg));
    }

    let content = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER_LIVE))
            .title(" Message Stream ")
            .title_style(Style::default().fg(BORDER_LIVE).bold()),
    );

    frame.render_widget(content, area);

    // Scrollbar
    if total_messages > visible_height {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);
        let mut scrollbar_state =
            ScrollbarState::new(total_messages.saturating_sub(visible_height))
                .position(scroll_offset);

        frame.render_stateful_widget(
            scrollbar,
            area.inner(ratatui::layout::Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

/// Format a single message for the live stream.
fn format_live_message(msg: &MessageWithContext) -> Line<'static> {
    // Format: HH:MM:SS [CC] [project/thread] role "preview..."
    let time_str = msg.ts.with_timezone(&Local).format("%H:%M:%S").to_string();

    // Assistant badge with distinct colors
    let (assistant_badge, assistant_color) = match msg.assistant {
        Assistant::ClaudeCode => ("CC", Color::Cyan),
        Assistant::Codex => ("CX", Color::Yellow),
        Assistant::Aider => ("AI", Color::Magenta),
        Assistant::Cursor => ("CU", Color::White),
    };

    let context_str = format!("[{}/{}]", msg.project_name, msg.thread_name);

    // Role styling
    let (role_str, role_style) = match msg.author_role {
        AuthorRole::Human => ("human", Style::default().fg(Color::Cyan)),
        AuthorRole::Caller => ("caller", Style::default().fg(Color::Cyan)),
        AuthorRole::Assistant => ("assistant", Style::default().fg(Color::Green)),
        AuthorRole::Tool => ("tool", Style::default().fg(Color::Yellow)),
        AuthorRole::Agent => ("agent", Style::default().fg(BADGE_AGENT)),
        AuthorRole::System => ("system", Style::default().fg(Color::DarkGray)),
    };

    // Preview text
    let preview = if msg.message_type == MessageType::ToolCall {
        if let Some(tool_name) = &msg.tool_name {
            format!("{}: {}", tool_name, truncate_str(&msg.preview, 40))
        } else {
            truncate_str(&msg.preview, 50).to_string()
        }
    } else {
        truncate_str(&msg.preview, 50).to_string()
    };

    Line::from(vec![
        Span::styled(time_str, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled(
            format!("[{}]", assistant_badge),
            Style::default().fg(assistant_color).bold(),
        ),
        Span::raw(" "),
        Span::styled(context_str, Style::default().fg(WRAPPED_DIM)),
        Span::raw(" "),
        Span::styled(format!("{:10}", role_str), role_style),
        Span::raw(" "),
        Span::styled(
            format!("\"{}\"", preview),
            Style::default().fg(Color::White),
        ),
    ])
}

/// Truncate a string to a maximum length.
/// Handles multi-byte UTF-8 characters safely.
/// Note: Does not add "..." - caller should handle that if needed.
fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        // Find a valid char boundary at or before max_len
        let mut end = max_len;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        &s[..end]
    }
}

/// Render the live view footer with key hints.
fn render_live_footer(frame: &mut Frame, _app: &App, area: Rect) {
    let key_style = Style::default().fg(WRAPPED_CYAN).bold();
    let label_style = Style::default().fg(Color::DarkGray);
    let separator = Span::styled("  │  ", Style::default().fg(Color::DarkGray));

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled("[1-5]", key_style),
        Span::styled(" Project  ", label_style),
        separator.clone(),
        Span::styled("[Tab]", key_style),
        Span::styled(" Projects view  ", label_style),
        separator.clone(),
        Span::styled("[j/k]", key_style),
        Span::styled(" Scroll  ", label_style),
        separator.clone(),
        Span::styled("[Space]", key_style),
        Span::styled(" Auto-scroll  ", label_style),
        separator,
        Span::styled("[q]", key_style),
        Span::styled(" Quit", label_style),
    ]));

    frame.render_widget(footer, area);
}
