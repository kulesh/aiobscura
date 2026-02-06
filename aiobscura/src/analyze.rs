//! aiobscura-analyze - CLI tool to run analytics on AI assistant sessions
//!
//! Runs the analytics plugin framework on sessions and displays metrics.

use aiobscura_core::analytics::{create_default_engine, PluginRunStatus};
use aiobscura_core::{Config, Database, SessionFilter};
use anyhow::{Context, Result};
use clap::Parser;

#[derive(Parser)]
#[command(name = "aiobscura-analyze")]
#[command(about = "Run analytics on AI assistant sessions")]
#[command(version)]
struct Args {
    /// Session ID to analyze (partial match supported)
    /// If not provided, analyzes all sessions
    #[arg(short, long)]
    session: Option<String>,

    /// Show only specific plugin results
    #[arg(short, long)]
    plugin: Option<String>,

    /// List available plugins without running analysis
    #[arg(long)]
    list_plugins: bool,

    /// Output format: text (default) or json
    #[arg(short, long, default_value = "text")]
    format: String,

    /// Verbose output (show all metrics, not just summary)
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    Config::ensure_xdg_env();

    // Load configuration
    let config = Config::load().context("failed to load configuration")?;

    // Initialize logging
    let _log_guard =
        aiobscura_core::logging::init(&config.logging).context("failed to initialize logging")?;

    // Open database
    let db_path = Config::database_path();
    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    // Create analytics engine
    let engine = create_default_engine();

    // List plugins mode
    if args.list_plugins {
        println!("Available plugins:");
        for name in engine.plugin_names() {
            println!("  - {}", name);
        }
        return Ok(());
    }

    // Find sessions to analyze
    let sessions = if let Some(ref session_id) = args.session {
        // Try exact match first
        if let Some(session) = db.get_session(session_id)? {
            vec![session]
        } else {
            // Try partial match
            let all_sessions = db.list_sessions(&SessionFilter::default())?;
            let matches: Vec<_> = all_sessions
                .into_iter()
                .filter(|s| s.id.contains(session_id))
                .collect();

            if matches.is_empty() {
                anyhow::bail!("No session found matching '{}'", session_id);
            }
            matches
        }
    } else {
        // Analyze all sessions
        db.list_sessions(&SessionFilter::default())?
    };

    if sessions.is_empty() {
        println!("No sessions found in database.");
        println!("Run 'aiobscura-sync' first to sync AI assistant logs.");
        return Ok(());
    }

    println!("Analyzing {} session(s)...\n", sessions.len());

    let mut total_metrics = 0;
    let mut sessions_with_data = 0;

    for session in &sessions {
        // Load messages for this session
        let messages = db.get_session_messages(&session.id, 100_000)?;

        if messages.is_empty() {
            continue;
        }

        // Run plugins (or specific plugin)
        let results = if let Some(ref plugin_name) = args.plugin {
            match engine.run_plugin(plugin_name, session, &messages, &db) {
                Ok(r) => vec![r],
                Err(e) => {
                    if args.verbose {
                        eprintln!("Plugin {} error: {}", plugin_name, e);
                    }
                    vec![]
                }
            }
        } else {
            // Run each plugin individually to catch errors
            let mut results = Vec::new();
            for plugin_name in engine.plugin_names() {
                match engine.run_plugin(plugin_name, session, &messages, &db) {
                    Ok(r) => results.push(r),
                    Err(e) => {
                        if args.verbose {
                            eprintln!(
                                "Plugin {} error on session {}: {}",
                                plugin_name,
                                &session.id[..8],
                                e
                            );
                        }
                    }
                }
            }
            results
        };

        // Check if any plugin produced metrics
        let produced = results.iter().map(|r| r.metrics_produced).sum::<usize>();
        if produced == 0 {
            continue;
        }

        sessions_with_data += 1;
        total_metrics += produced;

        // Output results
        if args.format == "json" {
            print_json_results(&session.id, &results, &db, args.verbose)?;
        } else {
            print_text_results(&session.id, &results, &db, args.verbose)?;
        }
    }

    // Summary
    if args.format != "json" {
        println!("\n---");
        println!(
            "Analyzed {} session(s), {} with data, {} metrics produced",
            sessions.len(),
            sessions_with_data,
            total_metrics
        );
    }

    Ok(())
}

fn print_text_results(
    session_id: &str,
    results: &[aiobscura_core::analytics::PluginRunResult],
    db: &Database,
    verbose: bool,
) -> Result<()> {
    // Get project name for display
    let session = db.get_session(session_id)?;
    let project_name = session
        .as_ref()
        .and_then(|s| s.project_id.as_ref())
        .and_then(|pid| db.get_project(pid).ok().flatten())
        .and_then(|p| p.name)
        .unwrap_or_else(|| "(no project)".to_string());

    let short_id = &session_id[..8.min(session_id.len())];
    println!("Session: {} ({})", short_id, project_name);

    for result in results {
        let status_icon = match result.status {
            PluginRunStatus::Success => "+",
            PluginRunStatus::Error => "!",
        };

        println!(
            "  [{}] {} ({} metrics, {}ms)",
            status_icon, result.plugin_name, result.metrics_produced, result.duration_ms
        );

        if let Some(ref e) = result.error_message {
            println!("      Error: {}", e);
        }

        // Show metrics if verbose or if there's interesting data
        if verbose || should_show_metrics(&result.plugin_name) {
            let metrics = db.get_session_plugin_metrics(session_id)?;
            let plugin_metrics: Vec<_> = metrics
                .iter()
                .filter(|m| m.plugin_name == result.plugin_name)
                .collect();

            for metric in plugin_metrics {
                let value_str = format_metric_value(&metric.metric_value);
                println!("      {}: {}", metric.metric_name, value_str);
            }
        }
    }
    println!();

    Ok(())
}

fn print_json_results(
    session_id: &str,
    results: &[aiobscura_core::analytics::PluginRunResult],
    db: &Database,
    _verbose: bool,
) -> Result<()> {
    let metrics = db.get_session_plugin_metrics(session_id)?;

    let output = serde_json::json!({
        "session_id": session_id,
        "results": results.iter().map(|r| {
            serde_json::json!({
                "plugin": r.plugin_name,
                "status": r.status.as_str(),
                "metrics_produced": r.metrics_produced,
                "duration_ms": r.duration_ms,
            })
        }).collect::<Vec<_>>(),
        "metrics": metrics.iter().map(|m| {
            serde_json::json!({
                "plugin": m.plugin_name,
                "name": m.metric_name,
                "value": m.metric_value,
            })
        }).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Determine if we should show metrics for this plugin by default
fn should_show_metrics(plugin_name: &str) -> bool {
    // Always show edit_churn metrics since they're the interesting ones
    plugin_name == "core.edit_churn"
}

/// Format a metric value for display
fn format_metric_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 {
                    format!("{}", f as i64)
                } else {
                    format!("{:.2}", f)
                }
            } else {
                n.to_string()
            }
        }
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else if arr.len() <= 3 {
                format!(
                    "[{}]",
                    arr.iter()
                        .map(format_metric_value)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("[{} items]", arr.len())
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                "{}".to_string()
            } else if obj.len() <= 3 {
                format!(
                    "{{{}}}",
                    obj.iter()
                        .map(|(k, v)| format!("{}: {}", k, format_metric_value(v)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("{{{} keys}}", obj.len())
            }
        }
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
    }
}
