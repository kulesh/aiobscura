use super::*;

pub(super) fn render_wrapped_view(frame: &mut Frame, app: &App) {
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
