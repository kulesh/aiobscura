//! aiobscura - AI Agent Activity Monitor
//!
//! Terminal UI for observing, querying, and analyzing AI coding agent activity.

use aiobscura_core::{Config, Database};
use anyhow::{Context, Result};

fn main() -> Result<()> {
    // Load configuration
    let config = Config::load().context("failed to load configuration")?;

    // Initialize logging
    let _log_guard = aiobscura_core::logging::init(&config.logging)
        .context("failed to initialize logging")?;

    tracing::info!("aiobscura starting up");

    // Open database
    let db_path = Config::database_path();
    tracing::info!(path = %db_path.display(), "Opening database");

    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    // Show status
    let session_counts = db.count_sessions_by_status()?;
    let event_count = db.count_events()?;

    println!("aiobscura - AI Agent Activity Monitor");
    println!();
    println!("Database: {}", db_path.display());
    println!("Sessions: {:?}", session_counts);
    println!("Events: {}", event_count);
    println!();
    println!("TUI not yet implemented - see Phase 6 of the implementation plan.");
    println!("Run 'cargo nextest run' to verify the core library works.");

    tracing::info!("aiobscura shutting down");

    Ok(())
}
