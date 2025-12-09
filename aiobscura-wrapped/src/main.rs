//! aiobscura-wrapped - AI Agent Year in Review CLI
//!
//! Generate Spotify Wrapped-style summaries of your AI assistant usage.

use aiobscura_core::analytics::{
    generate_wrapped, TimePatterns, TrendComparison, WrappedConfig, WrappedPeriod, WrappedStats,
};
use aiobscura_core::{Config, Database};
use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "aiobscura-wrapped")]
#[command(about = "AI Agent Wrapped - Your Year in Review")]
#[command(version)]
struct Args {
    /// Year to generate wrapped for (default: current year)
    #[arg(long)]
    year: Option<i32>,

    /// Month to generate wrapped for (format: YYYY-MM)
    #[arg(long)]
    month: Option<String>,

    /// Disable fun mode (no personality, no witty descriptions)
    #[arg(long)]
    serious: bool,

    /// Export format (md = markdown, json = JSON)
    #[arg(long)]
    export: Option<String>,

    /// Disable trend comparison with previous period
    #[arg(long)]
    no_trends: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Load configuration and database
    let config = Config::load().context("failed to load configuration")?;
    let _log_guard = aiobscura_core::logging::init(&config.logging).ok();

    let db_path = Config::database_path();
    let db = Database::open(&db_path).context("failed to open database")?;

    // Determine the period
    let period = if let Some(month_str) = &args.month {
        // Parse YYYY-MM format
        let parts: Vec<&str> = month_str.split('-').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid month format. Use YYYY-MM (e.g., 2024-12)");
        }
        let year: i32 = parts[0].parse().context("Invalid year")?;
        let month: u32 = parts[1].parse().context("Invalid month")?;
        if !(1..=12).contains(&month) {
            anyhow::bail!("Month must be between 1 and 12");
        }
        WrappedPeriod::Month(year, month)
    } else if let Some(year) = args.year {
        WrappedPeriod::Year(year)
    } else {
        WrappedPeriod::current_year()
    };

    // Configure wrapped generation
    let wrapped_config = WrappedConfig {
        fun_mode: !args.serious,
        include_trends: !args.no_trends,
        ..Default::default()
    };

    // Generate the stats
    let stats = generate_wrapped(&db, period, &wrapped_config)
        .context("failed to generate wrapped stats")?;

    // Output based on export format
    match args.export.as_deref() {
        Some("json") => print_json(&stats)?,
        Some("md") => print_markdown(&stats, !args.serious)?,
        Some(other) => anyhow::bail!("Unknown export format: {}. Use 'md' or 'json'", other),
        None => print_terminal(&stats, !args.serious),
    }

    Ok(())
}

fn print_terminal(stats: &WrappedStats, fun_mode: bool) {
    let title = if fun_mode {
        format!("🎉 YOUR {} AI WRAPPED 🎉", stats.period.display_name())
    } else {
        format!("AI Usage Summary: {}", stats.period.display_name())
    };

    // Header
    println!();
    println!("╭{}╮", "─".repeat(60));
    println!("│{:^60}│", title);
    println!("╰{}╯", "─".repeat(60));
    println!();

    // Check if there's any data
    if stats.totals.sessions == 0 {
        println!("  No activity found for this period.");
        println!();
        return;
    }

    // The Numbers
    if fun_mode {
        println!("📊 THE NUMBERS");
    } else {
        println!("SUMMARY");
    }
    println!(
        "   Sessions: {:<12} Total Time: {}",
        stats.totals.sessions,
        stats.totals.duration_display()
    );
    println!(
        "   Tokens:   {:<12} Projects: {}",
        stats.totals.tokens_display(),
        stats.totals.unique_projects
    );
    println!(
        "   Tools:    {:<12} Plans: {}",
        stats.totals.tool_calls, stats.totals.plans
    );
    println!(
        "   Agents:   {:<12} Files: {}",
        stats.totals.agents_spawned, stats.totals.files_modified
    );
    println!();

    // Top Tools
    if !stats.tools.top_tools.is_empty() {
        if fun_mode {
            println!("🏆 TOP TOOLS");
        } else {
            println!("TOP TOOLS");
        }
        for (i, (name, count, desc)) in stats.tools.top_tools.iter().enumerate() {
            let rank = match i {
                0 if fun_mode => "🥇".to_string(),
                1 if fun_mode => "🥈".to_string(),
                2 if fun_mode => "🥉".to_string(),
                _ => format!("{}.", i + 1),
            };
            if let Some(description) = desc {
                println!("   {} {:<10} {:>6}  \"{}\"", rank, name, count, description);
            } else {
                println!("   {} {:<10} {:>6}", rank, name, count);
            }
        }
        println!();
    }

    // Time Patterns
    if fun_mode {
        println!("⏰ TIME PATTERNS");
    } else {
        println!("TIME PATTERNS");
    }
    println!(
        "   Peak hour:    {}",
        TimePatterns::hour_display(stats.time_patterns.peak_hour)
    );
    println!(
        "   Busiest day:  {}",
        TimePatterns::day_name(stats.time_patterns.busiest_day)
    );
    if let Some(marathon) = &stats.time_patterns.marathon_session {
        let project = marathon
            .project_name
            .as_deref()
            .unwrap_or("unknown project");
        println!(
            "   Marathon:     {} - {} on {}",
            marathon.date_display(),
            marathon.duration_display(),
            project
        );
    }
    println!();

    // Streaks
    if fun_mode {
        println!("🔥 STREAKS");
    } else {
        println!("STREAKS");
    }
    println!(
        "   Current:  {} day{}",
        stats.streaks.current_streak_days,
        if stats.streaks.current_streak_days == 1 {
            ""
        } else {
            "s"
        }
    );
    if stats.streaks.longest_streak_days > 0 {
        let streak_dates = match (
            &stats.streaks.longest_streak_start,
            &stats.streaks.longest_streak_end,
        ) {
            (Some(start), Some(end)) => {
                format!(
                    " ({} - {})",
                    start.with_timezone(&Local).format("%b %d"),
                    end.with_timezone(&Local).format("%b %d")
                )
            }
            _ => String::new(),
        };
        println!(
            "   Longest:  {} day{}{}",
            stats.streaks.longest_streak_days,
            if stats.streaks.longest_streak_days == 1 {
                ""
            } else {
                "s"
            },
            streak_dates
        );
    }
    println!(
        "   Active:   {} of {} days ({:.0}%)",
        stats.streaks.active_days,
        stats.streaks.total_days,
        stats.streaks.activity_percentage()
    );
    println!();

    // Trends
    if let Some(trends) = &stats.trends {
        if fun_mode {
            println!("📈 VS PREVIOUS PERIOD");
        } else {
            println!("VS PREVIOUS PERIOD");
        }
        println!(
            "   Sessions: {}  │  Tokens: {}  │  Tools: {}",
            TrendComparison::format_delta(trends.sessions_delta_pct),
            TrendComparison::format_delta(trends.tokens_delta_pct),
            TrendComparison::format_delta(trends.tools_delta_pct),
        );
        println!();
    }

    // Projects
    if !stats.projects.is_empty() {
        if fun_mode {
            println!("📁 TOP PROJECTS");
        } else {
            println!("TOP PROJECTS");
        }
        for (i, project) in stats.projects.iter().take(3).enumerate() {
            let rank = if fun_mode && i == 0 { "🏆 " } else { "   " };
            println!(
                "{}{} - {} sessions, {} tokens",
                rank,
                project.name,
                project.sessions,
                format_tokens(project.tokens)
            );
        }
        println!();
    }

    // Personality (fun mode only)
    if let Some(personality) = &stats.personality {
        println!("🎭 YOUR PERSONALITY: {}", personality.name());
        println!("   \"{}\"", personality.tagline());
        println!();
    }
}

fn print_markdown(stats: &WrappedStats, fun_mode: bool) -> Result<()> {
    let title = if fun_mode {
        format!("🎉 {} AI Wrapped 🎉", stats.period.display_name())
    } else {
        format!("AI Usage Summary: {}", stats.period.display_name())
    };

    println!("# {}", title);
    println!();

    if stats.totals.sessions == 0 {
        println!("*No activity found for this period.*");
        return Ok(());
    }

    // Summary table
    println!("## Summary");
    println!();
    println!("| Metric | Value |");
    println!("|--------|-------|");
    println!("| Sessions | {} |", stats.totals.sessions);
    println!("| Total Time | {} |", stats.totals.duration_display());
    println!("| Tokens | {} |", stats.totals.tokens_display());
    println!("| Tool Calls | {} |", stats.totals.tool_calls);
    println!("| Agents Spawned | {} |", stats.totals.agents_spawned);
    println!("| Files Modified | {} |", stats.totals.files_modified);
    println!("| Plans | {} |", stats.totals.plans);
    println!("| Projects | {} |", stats.totals.unique_projects);
    println!();

    // Top Tools
    if !stats.tools.top_tools.is_empty() {
        println!("## Top Tools");
        println!();
        for (i, (name, count, desc)) in stats.tools.top_tools.iter().enumerate() {
            let emoji = match i {
                0 => "🥇",
                1 => "🥈",
                2 => "🥉",
                _ => "  ",
            };
            if fun_mode {
                if let Some(description) = desc {
                    println!(
                        "{} **{}** - {} calls - *\"{}\"*",
                        emoji, name, count, description
                    );
                } else {
                    println!("{} **{}** - {} calls", emoji, name, count);
                }
            } else {
                println!("{}. **{}** - {} calls", i + 1, name, count);
            }
        }
        println!();
    }

    // Time Patterns
    println!("## Time Patterns");
    println!();
    println!(
        "- **Peak hour:** {}",
        TimePatterns::hour_display(stats.time_patterns.peak_hour)
    );
    println!(
        "- **Busiest day:** {}",
        TimePatterns::day_name(stats.time_patterns.busiest_day)
    );
    if let Some(marathon) = &stats.time_patterns.marathon_session {
        let project = marathon.project_name.as_deref().unwrap_or("unknown");
        println!(
            "- **Marathon session:** {} - {} on {}",
            marathon.date_display(),
            marathon.duration_display(),
            project
        );
    }
    println!();

    // Streaks
    println!("## Streaks");
    println!();
    println!(
        "- **Current streak:** {} days",
        stats.streaks.current_streak_days
    );
    println!(
        "- **Longest streak:** {} days",
        stats.streaks.longest_streak_days
    );
    println!(
        "- **Active days:** {} of {} ({:.0}%)",
        stats.streaks.active_days,
        stats.streaks.total_days,
        stats.streaks.activity_percentage()
    );
    println!();

    // Trends
    if let Some(trends) = &stats.trends {
        println!("## Trends vs Previous Period");
        println!();
        println!("| Metric | Change |");
        println!("|--------|--------|");
        println!(
            "| Sessions | {} |",
            TrendComparison::format_delta(trends.sessions_delta_pct)
        );
        println!(
            "| Tokens | {} |",
            TrendComparison::format_delta(trends.tokens_delta_pct)
        );
        println!(
            "| Tools | {} |",
            TrendComparison::format_delta(trends.tools_delta_pct)
        );
        println!(
            "| Duration | {} |",
            TrendComparison::format_delta(trends.duration_delta_pct)
        );
        println!();
    }

    // Personality
    if let Some(personality) = &stats.personality {
        println!("## Your Coding Personality");
        println!();
        println!("{} **{}**", personality.emoji(), personality.name());
        println!();
        println!("*\"{}\"*", personality.tagline());
        println!();
    }

    println!("---");
    println!("*Generated by aiobscura-wrapped*");

    Ok(())
}

fn print_json(stats: &WrappedStats) -> Result<()> {
    // Convert to a serializable format
    let json = serde_json::json!({
        "period": stats.period.display_name(),
        "totals": {
            "sessions": stats.totals.sessions,
            "duration_secs": stats.totals.total_duration_secs,
            "tokens_in": stats.totals.tokens_in,
            "tokens_out": stats.totals.tokens_out,
            "tool_calls": stats.totals.tool_calls,
            "plans": stats.totals.plans,
            "agents_spawned": stats.totals.agents_spawned,
            "files_modified": stats.totals.files_modified,
            "unique_projects": stats.totals.unique_projects,
        },
        "tools": stats.tools.top_tools.iter().map(|(name, count, _)| {
            serde_json::json!({"name": name, "count": count})
        }).collect::<Vec<_>>(),
        "time_patterns": {
            "peak_hour": stats.time_patterns.peak_hour,
            "busiest_day": stats.time_patterns.busiest_day,
            "hourly_distribution": stats.time_patterns.hourly_distribution,
            "daily_distribution": stats.time_patterns.daily_distribution,
        },
        "streaks": {
            "current": stats.streaks.current_streak_days,
            "longest": stats.streaks.longest_streak_days,
            "active_days": stats.streaks.active_days,
            "total_days": stats.streaks.total_days,
        },
        "personality": stats.personality.as_ref().map(|p| p.name()),
        "trends": stats.trends.as_ref().map(|t| serde_json::json!({
            "sessions_delta_pct": t.sessions_delta_pct,
            "tokens_delta_pct": t.tokens_delta_pct,
            "tools_delta_pct": t.tools_delta_pct,
            "duration_delta_pct": t.duration_delta_pct,
        })),
        "projects": stats.projects.iter().map(|p| serde_json::json!({
            "name": p.name,
            "sessions": p.sessions,
            "tokens": p.tokens,
            "duration_secs": p.duration_secs,
        })).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}
