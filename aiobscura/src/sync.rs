//! aiobscura-sync - CLI tool to sync AI assistant logs to the database
//!
//! This tool discovers installed AI assistants, finds their log files,
//! and populates the aiobscura database with parsed data.
//!
//! Uses XDG Base Directory specification for file locations:
//! - Database: $XDG_DATA_HOME/aiobscura/data.db (~/.local/share/aiobscura/data.db)
//! - Logs: $XDG_STATE_HOME/aiobscura/aiobscura.log (~/.local/state/aiobscura/aiobscura.log)
//! - Config: $XDG_CONFIG_HOME/aiobscura/config.toml (~/.config/aiobscura/config.toml)

mod process_lock;

use aiobscura_core::collector::SyncPublisher;
use aiobscura_core::ingest::IngestCoordinator;
use aiobscura_core::{Config, Database};
use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use indicatif::{ProgressBar, ProgressStyle};
use process_lock::acquire_sync_guard;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "aiobscura-sync")]
#[command(about = "Sync AI assistant logs to the database")]
#[command(version)]
struct Args {
    /// Verbose output (-v per-file, -vv per-message)
    #[arg(short, long, action = ArgAction::Count)]
    verbose: u8,

    /// Dry run - discover files but don't sync
    #[arg(long)]
    dry_run: bool,

    /// Watch mode - continuously sync instead of one-shot
    #[arg(short, long)]
    watch: bool,

    /// Poll interval in milliseconds (only with --watch)
    #[arg(long, default_value = "1000")]
    poll: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Ensure XDG environment variables are set before using core library
    Config::ensure_xdg_env();

    // Load configuration
    let config = Config::load().context("failed to load configuration")?;

    // Initialize logging
    let _log_guard =
        aiobscura_core::logging::init(&config.logging).context("failed to initialize logging")?;

    tracing::info!("aiobscura-sync starting");

    // Resolve database path and enforce process-level exclusivity for it.
    let db_path = Config::database_path();
    let _sync_guard = acquire_sync_guard(&db_path).context("failed to acquire process lock")?;

    // Open database at XDG-compliant path
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

    // Initialize Catsyphon publisher if configured
    let mut publisher =
        SyncPublisher::new(&config.collector).context("failed to create publisher")?;

    if publisher.is_some() {
        println!("Catsyphon collector: enabled");
        tracing::info!(
            server_url = %config.collector.server_url.as_deref().unwrap_or(""),
            "Catsyphon collector enabled"
        );
    }

    // Open a second database connection for publishing queries
    // (coordinator owns the first connection)
    let publish_db = if publisher.is_some() {
        Some(Database::open(&db_path).context("failed to open publish database")?)
    } else {
        None
    };

    let result = if args.watch {
        // Watch mode - continuous polling
        run_watch_mode(&coordinator, &args, publish_db.as_ref(), &mut publisher)
    } else {
        // One-shot sync
        run_single_sync(&coordinator, &args, publish_db.as_ref(), &mut publisher)
    };

    // Flush any pending events on shutdown
    if let Some(ref mut pub_instance) = publisher {
        if pub_instance.has_pending() {
            println!(
                "Flushing {} pending events...",
                pub_instance.pending_count()
            );
            match pub_instance.flush_all() {
                Ok(sent) => {
                    if sent > 0 {
                        println!("Sent {} events to Catsyphon", sent);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to flush events on shutdown");
                }
            }
        }

        // Print collector stats
        let stats = pub_instance.stats();
        if stats.api_calls > 0 {
            tracing::info!(
                events_sent = stats.events_sent,
                events_rejected = stats.events_rejected,
                api_calls = stats.api_calls,
                api_failures = stats.api_failures,
                "Catsyphon collector stats"
            );
        }
    }

    result
}

/// Run a single sync operation with progress bar
fn run_single_sync(
    coordinator: &IngestCoordinator,
    args: &Args,
    publish_db: Option<&Database>,
    publisher: &mut Option<SyncPublisher>,
) -> Result<()> {
    // Track time before sync for publishing
    let sync_start = chrono::Utc::now();

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

    // Publish newly synced messages to Catsyphon
    if result.messages_inserted > 0 {
        publish_new_messages(publish_db, publisher, sync_start, result.messages_inserted);
    }

    print_sync_result(&result, args.verbose);

    tracing::info!(
        files_processed = result.files_processed,
        messages_inserted = result.messages_inserted,
        "aiobscura-sync complete"
    );

    Ok(())
}

/// Run continuous watch mode
fn run_watch_mode(
    coordinator: &IngestCoordinator,
    args: &Args,
    publish_db: Option<&Database>,
    publisher: &mut Option<SyncPublisher>,
) -> Result<()> {
    // Set up signal handler for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        eprintln!("\nShutting down...");
        r.store(false, Ordering::SeqCst);
    })
    .context("failed to set Ctrl+C handler")?;

    let poll_duration = Duration::from_millis(args.poll);

    println!(
        "Watch mode active (poll every {}ms). Press Ctrl+C to stop.",
        args.poll
    );
    println!();

    let mut iteration = 0u64;

    while running.load(Ordering::SeqCst) {
        iteration += 1;

        // Track time before sync for publishing
        let sync_start = chrono::Utc::now();

        // Run sync (checkpoints ensure incremental parsing)
        let result = coordinator
            .sync_all_with_progress(|_current, _total, _path| {
                // Silent progress in watch mode
            })
            .context("sync failed")?;

        // Only print if there were changes
        if result.messages_inserted > 0 {
            // Publish newly synced messages to Catsyphon
            publish_new_messages(publish_db, publisher, sync_start, result.messages_inserted);

            let timestamp = chrono::Local::now().format("%H:%M:%S");
            println!(
                "[{}] Synced: {} files, {} messages, {} sessions",
                timestamp,
                result.files_processed,
                result.messages_inserted,
                result.sessions_created + result.sessions_updated
            );

            // -v: Show per-file details, -vv: Show per-message details
            if args.verbose >= 1 {
                for file_result in &result.file_results {
                    if file_result.new_messages > 0 {
                        let path_str = shorten_path(&file_result.path);
                        println!("  {}: +{} messages", path_str, file_result.new_messages);

                        // -vv: Show per-message details
                        if args.verbose >= 2 {
                            for msg in &file_result.message_summaries {
                                println!("    [{}] {}", msg.role, msg.preview);
                            }
                        }
                    }
                }
            }

            if args.verbose >= 1 && !result.warnings.is_empty() {
                for warning in &result.warnings {
                    println!("  Warning: {}", warning);
                }
            }

            tracing::info!(
                iteration,
                files_processed = result.files_processed,
                messages_inserted = result.messages_inserted,
                "watch sync iteration"
            );
        }

        // Sleep until next poll
        thread::sleep(poll_duration);
    }

    println!("Watch mode stopped.");
    tracing::info!("aiobscura-sync watch mode stopped");

    Ok(())
}

/// Print sync result summary
fn print_sync_result(result: &aiobscura_core::ingest::SyncResult, verbose: u8) {
    println!("\nSync complete:");
    println!("  Files processed:  {}", result.files_processed);
    println!("  Files skipped:    {}", result.files_skipped);
    println!("  Sessions created: {}", result.sessions_created);
    println!("  Sessions updated: {}", result.sessions_updated);
    println!("  Messages inserted: {}", result.messages_inserted);
    println!("  Threads created:  {}", result.threads_created);

    // -v: Show per-file details, -vv: Show per-message details
    if verbose >= 1 {
        let files_with_changes: Vec<_> = result
            .file_results
            .iter()
            .filter(|f| f.new_messages > 0)
            .collect();

        if !files_with_changes.is_empty() {
            println!("\nFiles synced:");
            for file_result in files_with_changes {
                let path_str = shorten_path(&file_result.path);
                println!("  {}: +{} messages", path_str, file_result.new_messages);

                // -vv: Show per-message details
                if verbose >= 2 {
                    for msg in &file_result.message_summaries {
                        println!("    [{}] {}", msg.role, msg.preview);
                    }
                }
            }
        }
    }

    if verbose >= 1 && !result.warnings.is_empty() {
        println!("\nWarnings ({}):", result.warnings.len());
        for warning in &result.warnings {
            println!("  {}", warning);
        }
    }

    if !result.errors.is_empty() {
        println!("\nErrors ({}):", result.errors.len());
        for (path, err) in &result.errors {
            println!("  {}: {}", path.display(), err);
        }
    }
}

/// Shorten a path for display by abbreviating the home directory
fn shorten_path(path: &std::path::Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(suffix) = path.strip_prefix(&home) {
            return format!("~/{}", suffix.display());
        }
    }
    path.display().to_string()
}

/// Publish newly synced messages to Catsyphon
fn publish_new_messages(
    publish_db: Option<&Database>,
    publisher: &mut Option<SyncPublisher>,
    sync_start: chrono::DateTime<chrono::Utc>,
    expected_count: usize,
) {
    let Some(db) = publish_db else { return };
    let Some(ref mut pub_instance) = publisher else {
        return;
    };

    // Query for messages observed after sync started
    // Add a small buffer to avoid missing messages due to timing
    let query_after = sync_start - chrono::Duration::seconds(1);

    match db.get_messages_since(query_after, expected_count + 100) {
        Ok(messages) => {
            if messages.is_empty() {
                return;
            }

            tracing::debug!(
                count = messages.len(),
                expected = expected_count,
                "Publishing new messages to Catsyphon"
            );

            match pub_instance.queue(&messages) {
                Ok(sent) => {
                    if sent > 0 {
                        tracing::debug!(sent, "Sent events to Catsyphon");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to queue messages for publishing");
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to query messages for publishing");
        }
    }
}
