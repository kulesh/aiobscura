//! aiobscura-collector - CLI tool for managing Catsyphon collector integration
//!
//! This tool provides commands for:
//! - Checking collector status and configuration
//! - Resuming incomplete publishes after crashes
//! - Manually flushing pending events
//!
//! Uses XDG Base Directory specification for file locations:
//! - Database: $XDG_DATA_HOME/aiobscura/data.db (~/.local/share/aiobscura/data.db)
//! - Config: $XDG_CONFIG_HOME/aiobscura/config.toml (~/.config/aiobscura/config.toml)

use aiobscura_core::collector::StatefulSyncPublisher;
use aiobscura_core::{Config, Database};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "aiobscura-collector")]
#[command(about = "Manage Catsyphon collector integration")]
#[command(version)]
struct Args {
    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show collector configuration and status
    Status,

    /// Resume publishing for sessions with unpublished messages
    Resume {
        /// Batch size for publishing (default: from config)
        #[arg(short, long)]
        batch_size: Option<usize>,
    },

    /// Flush any pending events in memory
    Flush,

    /// Show publish state for all active sessions
    Sessions {
        /// Show all sessions (including completed)
        #[arg(short, long)]
        all: bool,
    },
}

/// Returns $HOME or panics
fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .expect("HOME environment variable not set")
}

/// Returns the XDG-compliant database path
fn database_path() -> PathBuf {
    let data_home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".local/share"));
    data_home.join("aiobscura/data.db")
}

/// Sets XDG environment variables to ensure the core library uses XDG paths
fn ensure_xdg_env() {
    let home = home_dir();

    if std::env::var("XDG_DATA_HOME").is_err() {
        std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
    }

    if std::env::var("XDG_STATE_HOME").is_err() {
        std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
    }

    if std::env::var("XDG_CONFIG_HOME").is_err() {
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    ensure_xdg_env();

    // Load configuration
    let config = Config::load().context("failed to load configuration")?;

    // Initialize logging if verbose
    if args.verbose {
        let _log_guard = aiobscura_core::logging::init(&config.logging)
            .context("failed to initialize logging")?;
    }

    match args.command {
        Command::Status => cmd_status(&config),
        Command::Resume { batch_size } => cmd_resume(&config, batch_size),
        Command::Flush => cmd_flush(&config),
        Command::Sessions { all } => cmd_sessions(&config, all),
    }
}

fn cmd_status(config: &Config) -> Result<()> {
    println!("Catsyphon Collector Configuration");
    println!("==================================");
    println!();

    let collector = &config.collector;

    println!("Enabled:         {}", collector.enabled);

    if !collector.enabled {
        println!();
        println!("Collector is disabled. Enable it in config.toml:");
        println!();
        println!("  [collector]");
        println!("  enabled = true");
        println!("  server_url = \"https://your-catsyphon-server.com\"");
        println!("  collector_id = \"your-collector-id\"");
        println!("  api_key = \"cs_live_xxxxxxxxxxxx\"");
        return Ok(());
    }

    println!(
        "Server URL:      {}",
        collector.server_url.as_deref().unwrap_or("<not set>")
    );
    println!(
        "Collector ID:    {}",
        collector.collector_id.as_deref().unwrap_or("<not set>")
    );
    println!(
        "API Key:         {}",
        if collector.api_key.is_some() {
            "<set>"
        } else {
            "<not set>"
        }
    );
    println!("Batch Size:      {}", collector.batch_size);
    println!("Flush Interval:  {}s", collector.flush_interval_secs);
    println!("Timeout:         {}s", collector.timeout_secs);
    println!("Max Retries:     {}", collector.max_retries);

    // Check if ready
    println!();
    if collector.is_ready() {
        println!("Status: Ready to publish");

        // Show database stats
        let db_path = database_path();
        if db_path.exists() {
            let db = Database::open(&db_path).context("failed to open database")?;
            let states = db.get_active_publish_states()?;

            println!();
            println!("Active Sessions: {}", states.len());

            let incomplete = db.get_incomplete_publish_states()?;
            if !incomplete.is_empty() {
                println!(
                    "Incomplete:      {} (run 'resume' to publish)",
                    incomplete.len()
                );
            }
        }
    } else {
        println!("Status: Not ready (missing required configuration)");
    }

    Ok(())
}

fn cmd_resume(config: &Config, batch_size_override: Option<usize>) -> Result<()> {
    if !config.collector.is_ready() {
        println!("Collector is not configured. Run 'status' for details.");
        return Ok(());
    }

    let db_path = database_path();
    if !db_path.exists() {
        println!("Database not found at {}", db_path.display());
        return Ok(());
    }

    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    let batch_size = batch_size_override.unwrap_or(config.collector.batch_size);

    let mut publisher = StatefulSyncPublisher::new(&config.collector, db)
        .context("failed to create publisher")?
        .expect("collector should be ready");

    println!("Checking for incomplete publishes...");

    let incomplete = publisher.database().get_incomplete_publish_states()?;
    if incomplete.is_empty() {
        println!("No incomplete publishes found.");
        return Ok(());
    }

    println!(
        "Found {} session(s) with unpublished messages",
        incomplete.len()
    );
    println!();

    for state in &incomplete {
        println!(
            "  Session: {} (last_seq: {})",
            &state.session_id[..8.min(state.session_id.len())],
            state.last_published_seq
        );
    }

    println!();
    println!("Resuming...");

    let sent = publisher.resume_incomplete(batch_size)?;

    println!();
    if sent > 0 {
        println!("Published {} event(s)", sent);
    } else {
        println!("No events published");
    }

    // Print stats
    let stats = publisher.stats();
    if stats.api_calls > 0 {
        println!();
        println!("Stats:");
        println!("  API Calls:  {}", stats.api_calls);
        println!("  Sent:       {}", stats.events_sent);
        println!("  Rejected:   {}", stats.events_rejected);
        println!("  Failures:   {}", stats.api_failures);
    }

    Ok(())
}

fn cmd_flush(config: &Config) -> Result<()> {
    if !config.collector.is_ready() {
        println!("Collector is not configured. Run 'status' for details.");
        return Ok(());
    }

    let db_path = database_path();
    if !db_path.exists() {
        println!("Database not found at {}", db_path.display());
        return Ok(());
    }

    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    let mut publisher = StatefulSyncPublisher::new(&config.collector, db)
        .context("failed to create publisher")?
        .expect("collector should be ready");

    if !publisher.has_pending() {
        println!("No pending events to flush.");
        return Ok(());
    }

    println!("Flushing {} pending event(s)...", publisher.pending_count());

    let sent = publisher.flush_all()?;

    if sent > 0 {
        println!("Flushed {} event(s)", sent);
    } else {
        println!("No events flushed");
    }

    Ok(())
}

fn cmd_sessions(config: &Config, show_all: bool) -> Result<()> {
    let db_path = database_path();
    if !db_path.exists() {
        println!("Database not found at {}", db_path.display());
        return Ok(());
    }

    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    let states = if show_all {
        // Get all publish states (would need a new method, for now just use active)
        db.get_active_publish_states()?
    } else {
        db.get_active_publish_states()?
    };

    if states.is_empty() {
        println!("No publish states found.");
        if !config.collector.is_ready() {
            println!();
            println!("Note: Collector is not configured. Run 'status' for details.");
        }
        return Ok(());
    }

    println!("Session Publish States");
    println!("======================");
    println!();
    println!(
        "{:<20} {:>10} {:>8} {:>20}",
        "Session ID", "Last Seq", "Status", "Last Published"
    );
    println!("{:-<62}", "");

    for state in states {
        let session_short = if state.session_id.len() > 18 {
            format!("{}...", &state.session_id[..15])
        } else {
            state.session_id.clone()
        };

        let last_published = state
            .last_published_at
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "never".to_string());

        println!(
            "{:<20} {:>10} {:>8} {:>20}",
            session_short, state.last_published_seq, state.status, last_published
        );
    }

    // Check for incomplete
    let incomplete = db.get_incomplete_publish_states()?;
    if !incomplete.is_empty() {
        println!();
        println!(
            "{} session(s) have unpublished messages. Run 'resume' to publish.",
            incomplete.len()
        );
    }

    Ok(())
}
