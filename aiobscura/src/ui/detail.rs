use super::*;

pub(super) fn render_detail_view(frame: &mut Frame, app: &mut App, thread_name: String) {
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
pub(super) fn render_session_detail_view(frame: &mut Frame, app: &mut App, session_name: String) {
    let area = frame.area();

    // Layout: header, analytics, messages, footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Length(6), // Analytics panel
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
    let error = app
        .session_analytics_error
        .as_ref()
        .or(app.session_first_order_error.as_ref());
    if app.session_analytics.is_none() && app.session_first_order_metrics.is_none() {
        if let Some(error) = error {
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
        let (thread_type, thread_color, badge, subtype_suffix) = if let Some(thread) =
            app.session_threads.iter().find(|t| t.id == thread_id)
        {
            let suffix = thread
                .agent_subtype()
                .map(|subtype| format!(" ({})", subtype))
                .unwrap_or_default();
            match thread.thread_type {
                ThreadType::Main => (ThreadType::Main, BADGE_MAIN, "●", String::new()),
                ThreadType::Agent => (ThreadType::Agent, BADGE_AGENT, "◎", suffix),
                ThreadType::Background => (ThreadType::Background, BADGE_BG, "◇", String::new()),
            }
        } else {
            (ThreadType::Main, Color::DarkGray, "●", String::new())
        };

        // For non-main threads, calculate and show duration
        let duration_str = if !matches!(thread_type, ThreadType::Main) {
            if let (Some(first), Some(last)) = (messages.first(), messages.last()) {
                format!(
                    " ({})",
                    format_group_duration(first.emitted_at, last.emitted_at)
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Build thread header line
        lines.push(Line::from(vec![
            Span::styled("─── ", Style::default().fg(thread_color)),
            Span::styled(format!("{} ", badge), Style::default().fg(thread_color)),
            Span::styled(
                format!("{}{}", thread_label, subtype_suffix),
                Style::default().fg(thread_color).bold(),
            ),
            Span::styled(duration_str, Style::default().fg(thread_color)),
            Span::styled(
                " ───────────────────────────────────",
                Style::default().fg(thread_color),
            ),
        ]));

        // Add messages for this thread
        for msg in messages {
            let (role_prefix, role_style) = session_role_prefix(msg);

            // Get content preview (shortened to leave room for timestamp)
            let content_preview = detail_preview(msg, 60);

            // Format timestamp (HH:MM in local time)
            let time_str = format_message_time(msg.emitted_at);

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
pub(super) fn render_header(frame: &mut Frame, title: &str, area: Rect) {
    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).bold())
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, area);
}

/// Which tab is currently active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ActiveTab {
    Live,
    Projects,
    Threads,
}

/// Render the tab bar header with Live, Projects, and Threads tabs.
pub(super) fn render_tab_header(frame: &mut Frame, active: ActiveTab, area: Rect) {
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
    let mut lines = Vec::new();

    if let Some(analytics) = &app.session_analytics {
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
    }

    if let Some(metrics) = &app.session_first_order_metrics {
        let mut line3_spans: Vec<Span> = Vec::new();
        line3_spans.push(Span::styled("Tokens: ", Style::default().fg(LABEL_COLOR)));
        line3_spans.push(Span::styled(
            format_tokens(metrics.tokens_total),
            Style::default().fg(Color::White),
        ));
        line3_spans.push(Span::styled(
            format!(
                " ({} in / {} out)",
                format_tokens(metrics.tokens_in),
                format_tokens(metrics.tokens_out)
            ),
            Style::default().fg(Color::DarkGray),
        ));
        line3_spans.push(Span::raw("  "));
        line3_spans.push(Span::styled("Duration: ", Style::default().fg(LABEL_COLOR)));
        line3_spans.push(Span::styled(
            format_duration_ms(metrics.duration_ms),
            Style::default().fg(Color::Cyan),
        ));
        lines.push(Line::from(line3_spans));

        let mut line4_spans: Vec<Span> = Vec::new();
        line4_spans.push(Span::styled("Tools: ", Style::default().fg(LABEL_COLOR)));
        line4_spans.push(Span::styled(
            format!("{}", metrics.tool_call_count),
            Style::default().fg(Color::White),
        ));
        line4_spans.push(Span::raw("  "));
        line4_spans.push(Span::styled("Errors: ", Style::default().fg(LABEL_COLOR)));
        let error_color = if metrics.error_count > 0 {
            Color::Red
        } else {
            Color::Green
        };
        line4_spans.push(Span::styled(
            format!("{}", metrics.error_count),
            Style::default().fg(error_color),
        ));
        line4_spans.push(Span::raw("  "));
        line4_spans.push(Span::styled("Success: ", Style::default().fg(LABEL_COLOR)));
        if metrics.tool_call_count == 0 {
            line4_spans.push(Span::styled("n/a", Style::default().fg(Color::DarkGray)));
        } else {
            let success_pct = (metrics.tool_success_rate * 100.0).round() as i64;
            let success_color = if success_pct >= 90 {
                Color::Green
            } else if success_pct >= 60 {
                Color::Yellow
            } else {
                Color::Red
            };
            line4_spans.push(Span::styled(
                format!("{}%", success_pct),
                Style::default().fg(success_color),
            ));
        }
        lines.push(Line::from(line4_spans));
    }

    if lines.is_empty() {
        lines.push(Line::from("No data"));
    }

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
pub(super) fn truncate_string(s: &str, max_len: usize) -> String {
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

/// Format duration in milliseconds for display.
fn format_duration_ms(ms: i64) -> String {
    if ms < 1000 {
        "<1s".to_string()
    } else {
        format_duration(ms / 1000)
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
pub(super) fn render_table(frame: &mut Frame, app: &mut App, area: Rect) {
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
            ThreadType::Main => ("●", "main".to_string(), BADGE_MAIN),
            ThreadType::Agent => {
                let label = thread
                    .agent_subtype
                    .as_deref()
                    .map(|subtype| format!("agent {}", subtype))
                    .unwrap_or_else(|| "agent".to_string());
                ("◎", label, BADGE_AGENT)
            }
            ThreadType::Background => ("◇", "bg".to_string(), BADGE_BG),
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
    let time_str = format_message_time(msg.emitted_at);

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
    let content = detail_content(msg);
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
    let time_str = format_message_time(msg.emitted_at);

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
    let content = detail_content(msg);
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

/// Render the footer for list view.
pub(super) fn render_list_footer(frame: &mut Frame, app: &App, area: Rect) {
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
pub(super) fn render_plan_list_view(frame: &mut Frame, app: &mut App, session_name: String) {
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
pub(super) fn render_plan_detail_view(frame: &mut Frame, app: &mut App, plan_title: String) {
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
pub(super) fn format_plan_status(status: &PlanStatus) -> String {
    match status {
        PlanStatus::Active => "Active".to_string(),
        PlanStatus::Completed => "Completed".to_string(),
        PlanStatus::Abandoned => "Abandoned".to_string(),
        PlanStatus::Unknown => "Unknown".to_string(),
    }
}
