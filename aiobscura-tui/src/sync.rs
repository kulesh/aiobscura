//! aiobscura-sync - CLI tool to sync AI assistant logs to the database
//!
//! This tool discovers installed AI assistants, finds their log files,
//! and populates the aiobscura database with parsed data.
//!
//! Uses XDG Base Directory specification for file locations:
//! - Database: $XDG_DATA_HOME/aiobscura/data.db (~/.local/share/aiobscura/data.db)
//! - Logs: $XDG_STATE_HOME/aiobscura/aiobscura.log (~/.local/state/aiobscura/aiobscura.log)
//! - Config: $XDG_CONFIG_HOME/aiobscura/config.toml (~/.config/aiobscura/config.toml)

use aiobscura_core::ingest::IngestCoordinator;
use aiobscura_core::{Config, Database};
use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "aiobscura-sync")]
#[command(about = "Sync AI assistant logs to the database")]
#[command(version)]
struct Args {
    /// Verbose output (show warnings during sync)
    #[arg(short, long)]
    verbose: bool,

    /// Dry run - discover files but don't sync
    #[arg(long)]
    dry_run: bool,
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

    // Set XDG_DATA_HOME if not set
    if std::env::var("XDG_DATA_HOME").is_err() {
        std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
    }

    // Set XDG_STATE_HOME if not set
    if std::env::var("XDG_STATE_HOME").is_err() {
        std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
    }

    // Set XDG_CONFIG_HOME if not set
    if std::env::var("XDG_CONFIG_HOME").is_err() {
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Ensure XDG environment variables are set before using core library
    ensure_xdg_env();

    // Load configuration
    let config = Config::load().context("failed to load configuration")?;

    // Initialize logging
    let _log_guard =
        aiobscura_core::logging::init(&config.logging).context("failed to initialize logging")?;

    tracing::info!("aiobscura-sync starting");

    // Open database at XDG-compliant path
    let db_path = database_path();
    tracing::info!(path = %db_path.display(), "Opening database");

    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    println!("Database: {}", db_path.display());

    // Create coordinator and discover installed assistants
    let coordinator = IngestCoordinator::new(db);
    let installed = coordinator.installed_assistants();

    println!("Discovered {} installed assistant(s):", installed.len());
    for parser in &installed {
        match parser.discover_files() {
            Ok(files) => {
                println!(
                    "  - {}: {} file(s) at {}",
                    parser.assistant().display_name(),
                    files.len(),
                    parser
                        .root_path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                );
            }
            Err(e) => {
                println!(
                    "  - {}: error discovering files: {}",
                    parser.assistant().display_name(),
                    e
                );
            }
        }
    }

    if args.dry_run {
        println!("\nDry run - no sync performed");
        tracing::info!("Dry run complete");
        return Ok(());
    }

    // Run sync with progress bar
    println!();
    let pb = ProgressBar::new(0);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let result = coordinator
        .sync_all_with_progress(|current, total, path| {
            if current == 0 {
                pb.set_length(total as u64);
            }
            pb.set_position(current as u64);
            pb.set_message(
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("...")
                    .to_string(),
            );
        })
        .context("sync failed")?;

    pb.finish_and_clear();

    // Print stats
    println!("\nSync complete:");
    println!("  Files processed:  {}", result.files_processed);
    println!("  Files skipped:    {}", result.files_skipped);
    println!("  Sessions created: {}", result.sessions_created);
    println!("  Sessions updated: {}", result.sessions_updated);
    println!("  Messages inserted: {}", result.messages_inserted);
    println!("  Threads created:  {}", result.threads_created);

    // Show warnings if verbose
    if args.verbose && !result.warnings.is_empty() {
        println!("\nWarnings ({}):", result.warnings.len());
        for warning in &result.warnings {
            println!("  {}", warning);
        }
    }

    // Show errors
    if !result.errors.is_empty() {
        println!("\nErrors ({}):", result.errors.len());
        for (path, err) in &result.errors {
            println!("  {}: {}", path.display(), err);
        }
    }

    tracing::info!(
        files_processed = result.files_processed,
        messages_inserted = result.messages_inserted,
        "aiobscura-sync complete"
    );

    Ok(())
}
