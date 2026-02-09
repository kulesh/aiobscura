use super::*;

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
