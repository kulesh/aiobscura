use super::*;

pub(super) fn render_project_list_view(frame: &mut Frame, app: &mut App) {
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
pub(super) fn render_project_detail_view(
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
