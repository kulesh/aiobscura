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

use aiobscura_core::collector::StatefulSyncPublisher;
use aiobscura_core::ingest::{IngestCoordinator, SyncResult};
use aiobscura_core::{Config, Database, SessionFilter};
use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use indicatif::{ProgressBar, ProgressStyle};
use process_lock::acquire_sync_guard;
use std::collections::{HashMap, HashSet};
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
    let mut publisher = if config.collector.is_ready() {
        let publish_db = Database::open(&db_path).context("failed to open publish database")?;
        StatefulSyncPublisher::new(&config.collector, publish_db)
            .context("failed to create publisher")?
    } else {
        None
    };

    if publisher.is_some() {
        println!("Catsyphon collector: enabled");
        tracing::info!(
            server_url = %config.collector.server_url.as_deref().unwrap_or(""),
            "Catsyphon collector enabled"
        );
    }

    if let Some(ref mut pub_instance) = publisher {
        let resumed = pub_instance
            .resume_incomplete(config.collector.batch_size)
            .context("failed to resume incomplete collector publishes")?;
        if resumed > 0 {
            tracing::info!(events = resumed, "Resumed incomplete collector publishes");
        }

        let completed = pub_instance
            .complete_stale_sessions()
            .context("failed to complete stale collector sessions")?;
        if completed > 0 {
            tracing::info!(sessions = completed, "Completed stale collector sessions");
        }
    }

    let result = if args.watch {
        // Watch mode - continuous polling
        run_watch_mode(&coordinator, &config, &args, &mut publisher)
    } else {
        // One-shot sync
        run_single_sync(&coordinator, &config, &args, &mut publisher)
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
    config: &Config,
    args: &Args,
    publisher: &mut Option<StatefulSyncPublisher>,
) -> Result<()> {
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

    publish_sync_sessions(config, publisher, &result);

    run_analytics_triggers(coordinator, config, &result, true)?;

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
    config: &Config,
    args: &Args,
    publisher: &mut Option<StatefulSyncPublisher>,
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
    let mut since_last_inactivity_trigger = Duration::from_secs(60);

    while running.load(Ordering::SeqCst) {
        iteration += 1;

        // Run sync (checkpoints ensure incremental parsing)
        let result = coordinator
            .sync_all_with_progress(|_current, _total, _path| {
                // Silent progress in watch mode
            })
            .context("sync failed")?;

        publish_sync_sessions(config, publisher, &result);

        // Only print if there were changes
        if result.messages_inserted > 0 {
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

        let run_inactivity = since_last_inactivity_trigger >= Duration::from_secs(60);
        run_analytics_triggers(coordinator, config, &result, run_inactivity)?;
        since_last_inactivity_trigger = if run_inactivity {
            Duration::ZERO
        } else {
            since_last_inactivity_trigger.saturating_add(poll_duration)
        };

        // Sleep until next poll
        thread::sleep(poll_duration);
    }

    println!("Watch mode stopped.");
    tracing::info!("aiobscura-sync watch mode stopped");

    Ok(())
}

/// Run automatic analytics triggers for updated or inactive sessions.
fn run_analytics_triggers(
    coordinator: &IngestCoordinator,
    config: &Config,
    sync_result: &SyncResult,
    include_inactivity: bool,
) -> Result<()> {
    let tool_call_threshold = usize::try_from(config.analytics.tool_call_threshold)
        .unwrap_or(usize::MAX)
        .max(1);
    let mut triggered_sessions = collect_tool_call_sessions(sync_result, tool_call_threshold);

    if include_inactivity {
        let cutoff = chrono::Utc::now()
            - chrono::Duration::minutes(i64::from(config.analytics.inactivity_minutes));
        for session in coordinator.db().list_sessions(&SessionFilter::default())? {
            if session
                .last_activity_at
                .map(|last| last <= cutoff)
                .unwrap_or(false)
            {
                triggered_sessions.insert(session.id);
            }
        }
    }

    if triggered_sessions.is_empty() {
        return Ok(());
    }

    let engine = aiobscura_core::analytics::create_default_engine_with_config(&config.analytics);

    let mut attempted = 0usize;
    let mut failures = 0usize;
    let mut llm_failures = 0usize;
    let mut llm_inserted = 0usize;

    let session_ids: Vec<String> = triggered_sessions.into_iter().collect();

    for session_id in &session_ids {
        attempted += 1;

        if let Err(e) = engine.ensure_session_analytics(session_id, coordinator.db()) {
            failures += 1;
            tracing::warn!(
                session_id = session_id,
                error = %e,
                "Failed event/inactivity trigger for core.edit_churn"
            );
        }

        if let Err(e) = engine.ensure_first_order_metrics(session_id, coordinator.db()) {
            failures += 1;
            tracing::warn!(
                session_id = session_id,
                error = %e,
                "Failed event/inactivity trigger for core.first_order"
            );
        }
    }

    if let Some(llm) = &config.llm {
        match aiobscura_core::assessment::create_assessment_client(llm) {
            Ok(client) => {
                for session_id in &session_ids {
                    let session = match coordinator.db().get_session(session_id)? {
                        Some(session) => session,
                        None => continue,
                    };
                    let messages = coordinator.db().get_session_messages(session_id, 100_000)?;

                    match aiobscura_core::assessment::assess_and_store_session_with_client(
                        coordinator.db(),
                        &session,
                        &messages,
                        llm,
                        client.as_ref(),
                    ) {
                        Ok(Some(_)) => {
                            llm_inserted += 1;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            llm_failures += 1;
                            tracing::warn!(
                                session_id = session_id,
                                error = %e,
                                "Failed LLM assessment trigger"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                llm_failures += 1;
                tracing::warn!(
                    error = %e,
                    "Failed to initialize LLM assessment client; skipping triggered assessments"
                );
            }
        }
    }

    tracing::debug!(
        sessions_considered = attempted,
        trigger_failures = failures,
        llm_failures,
        llm_assessments_inserted = llm_inserted,
        include_inactivity,
        tool_call_threshold,
        "Processed analytics triggers"
    );

    Ok(())
}

/// Find session IDs that crossed the per-sync tool-call threshold.
fn collect_tool_call_sessions(sync_result: &SyncResult, threshold: usize) -> HashSet<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for file_result in &sync_result.file_results {
        if file_result.new_tool_calls == 0 {
            continue;
        }
        if let Some(session_id) = &file_result.session_id {
            *counts.entry(session_id.clone()).or_insert(0) += file_result.new_tool_calls;
        }
    }

    counts
        .into_iter()
        .filter_map(|(session_id, count)| (count >= threshold).then_some(session_id))
        .collect()
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

/// Publish all updated sessions and run recovery/completion routines.
fn publish_sync_sessions(
    config: &Config,
    publisher: &mut Option<StatefulSyncPublisher>,
    result: &SyncResult,
) {
    let Some(ref mut pub_instance) = publisher else {
        return;
    };

    let mut touched_sessions = HashSet::new();
    for file_result in &result.file_results {
        if file_result.new_messages == 0 {
            continue;
        }
        if let Some(session_id) = &file_result.session_id {
            touched_sessions.insert(session_id.clone());
        }
    }

    for session_id in touched_sessions {
        loop {
            match pub_instance.publish_session(&session_id, config.collector.batch_size) {
                Ok(0) => break,
                Ok(sent) => {
                    tracing::debug!(
                        session_id = %session_id,
                        events = sent,
                        "Published session events"
                    );
                    if sent < config.collector.batch_size {
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        session_id = %session_id,
                        "Failed to publish session events"
                    );
                    break;
                }
            }
        }
    }

    match pub_instance.resume_incomplete(config.collector.batch_size) {
        Ok(resumed) if resumed > 0 => {
            tracing::info!(events = resumed, "Resumed incomplete collector publishes");
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = %e, "Failed to resume incomplete collector publishes");
        }
    }

    match pub_instance.complete_stale_sessions() {
        Ok(completed) if completed > 0 => {
            tracing::info!(sessions = completed, "Completed stale collector sessions");
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = %e, "Failed to complete stale collector sessions");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiobscura_core::ingest::{FileSyncResult, MessageSummary, SkipReason};
    use std::path::PathBuf;

    fn file_result(
        session_id: Option<&str>,
        new_messages: usize,
        new_tool_calls: usize,
    ) -> FileSyncResult {
        FileSyncResult {
            path: PathBuf::from("/tmp/test.jsonl"),
            new_messages,
            new_tool_calls,
            session_id: session_id.map(ToString::to_string),
            new_checkpoint: aiobscura_core::Checkpoint::ByteOffset { offset: 0 },
            is_new_session: false,
            warnings: vec![],
            skip_reason: Some(SkipReason::NoNewContent),
            message_summaries: vec![MessageSummary {
                role: "assistant".to_string(),
                preview: "ok".to_string(),
            }],
        }
    }

    #[test]
    fn collect_tool_call_sessions_aggregates_by_session() {
        let sync_result = SyncResult {
            file_results: vec![
                file_result(Some("sess-a"), 6, 3),
                file_result(Some("sess-a"), 4, 2),
                file_result(Some("sess-b"), 7, 1),
                file_result(Some("sess-c"), 8, 5),
            ],
            ..Default::default()
        };

        let sessions = collect_tool_call_sessions(&sync_result, 5);

        assert!(sessions.contains("sess-a"));
        assert!(sessions.contains("sess-c"));
        assert!(!sessions.contains("sess-b"));
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn collect_tool_call_sessions_ignores_unlinked_or_empty_results() {
        let sync_result = SyncResult {
            file_results: vec![
                file_result(None, 10, 10),
                file_result(Some("sess-a"), 0, 0),
                file_result(Some("sess-b"), 2, 2),
            ],
            ..Default::default()
        };

        let sessions = collect_tool_call_sessions(&sync_result, 3);

        assert!(sessions.is_empty());
    }

    #[test]
    fn collect_tool_call_sessions_ignores_non_tool_messages() {
        let sync_result = SyncResult {
            file_results: vec![file_result(Some("sess-a"), 20, 0)],
            ..Default::default()
        };

        let sessions = collect_tool_call_sessions(&sync_result, 1);

        assert!(sessions.is_empty());
    }
}
