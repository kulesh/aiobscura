use super::*;

pub(super) fn render_live_view(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Calculate active sessions panel height (show up to 4 sessions, min 2)
    let active_session_count = app.active_sessions.len();
    let sessions_height = (active_session_count.clamp(1, 4) + 2) as u16; // +2 for border

    // Calculate quick projects height (show up to 5 projects)
    let projects_count = app.projects.len().min(5);
    let projects_height = (projects_count.max(2) + 2) as u16; // +2 for border

    // Environment panel needs 4 lines (db + 2 agents + border)
    let env_height: u16 = 5;

    // Use the larger of all three for the middle panel
    let middle_panel_height = sessions_height.max(projects_height).max(env_height);

    // Layout: tab header, dashboard summary, middle panels, message stream, footer
    let chunks = Layout::vertical([
        Constraint::Length(2),                   // Tab header
        Constraint::Length(6),                   // Dashboard summary (stats + heatmap)
        Constraint::Length(middle_panel_height), // Projects | Environment | Sessions
        Constraint::Min(5),                      // Message stream
        Constraint::Length(1),                   // Footer
    ])
    .split(area);

    // === Tab Header ===
    render_tab_header(frame, ActiveTab::Live, chunks[0]);

    // === Dashboard Summary ===
    render_live_dashboard_summary(frame, app, chunks[1]);

    // === Middle Panel: Recent Projects | Environment | Active Sessions ===
    let middle_chunks = Layout::horizontal([
        Constraint::Percentage(35), // Recent Projects
        Constraint::Percentage(30), // Environment Health
        Constraint::Percentage(35), // Active Sessions
    ])
    .split(chunks[2]);

    render_quick_projects_panel(frame, app, middle_chunks[0]);
    render_environment_health_panel(frame, &app.environment_health, middle_chunks[1]);
    render_active_sessions_panel(frame, app, middle_chunks[2]);

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

/// Render the environment health panel showing agent status and database info.
fn render_environment_health_panel(frame: &mut Frame, health: &EnvironmentHealth, area: Rect) {
    let block = Block::default()
        .title(" Environment ")
        .title_style(Style::default().fg(LIVE_INDICATOR).bold())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_LIVE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // Database info line - show size, sessions, and messages
    let db_size = format_bytes(health.database_size_bytes);
    lines.push(Line::from(vec![
        Span::styled(" DB ", Style::default().fg(WRAPPED_DIM)),
        Span::styled(db_size, Style::default().fg(WRAPPED_CYAN).bold()),
        Span::styled("  ", Style::default()),
        Span::styled(
            format!(
                "{} sess  {} msgs",
                health.total_sessions, health.total_messages
            ),
            Style::default().fg(WRAPPED_DIM),
        ),
    ]));

    // Show each assistant with status
    // Agents we know about (even if no data yet)
    let known_assistants = [
        (Assistant::ClaudeCode, "Claude"),
        (Assistant::Codex, "Codex"),
    ];

    for (assistant, name) in known_assistants {
        // Find stats for this assistant
        let stats = health.assistants.iter().find(|a| a.assistant == assistant);

        let (status_icon, status_color, detail) = match stats {
            Some(s) if s.file_count > 0 => {
                let size_str = format_bytes(s.total_size_bytes as u64);
                let sync_info = s
                    .last_synced
                    .map(format_relative_time)
                    .unwrap_or_else(|| "—".to_string());
                ("✓", WRAPPED_LIME, format!("{}  {}", size_str, sync_info))
            }
            _ => ("○", WRAPPED_DIM, "no logs".to_string()),
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", status_icon),
                Style::default().fg(status_color),
            ),
            Span::styled(format!("{:<7}", name), Style::default().fg(Color::White)),
            Span::styled(detail, Style::default().fg(WRAPPED_DIM)),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Format bytes as human-readable size (e.g., "42 MB").
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
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
    let time_str = msg
        .emitted_at
        .with_timezone(&Local)
        .format("%H:%M:%S")
        .to_string();

    // Assistant badge with distinct colors
    let (assistant_badge, assistant_color) = match msg.assistant {
        Assistant::ClaudeCode => ("CC", Color::Cyan),
        Assistant::Codex => ("CX", Color::Yellow),
        Assistant::Aider => ("AI", Color::Magenta),
        Assistant::Cursor => ("CU", Color::White),
    };

    let context_str = format!("[{}/{}]", msg.project_name, msg.thread_name);

    // Role styling
    let (role_str, role_style) = live_role_label(msg.author_role);

    // Preview text
    let preview = live_preview(msg);

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
