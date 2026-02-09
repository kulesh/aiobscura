//! aiobscura - AI Agent Activity Monitor
//!
//! Terminal UI for observing, querying, and analyzing AI coding agent activity.

mod app;
mod message_format;
mod process_lock;
mod thread_row;
mod ui;

use std::io;

use aiobscura_core::ingest::IngestCoordinator;
use aiobscura_core::{Config, Database};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::App;
use crate::process_lock::{acquire_ui_guards, UiRunMode};

fn main() -> Result<()> {
    // Load configuration
    let config = Config::load().context("failed to load configuration")?;

    // Initialize logging (to file, not stdout since we have a TUI)
    let _log_guard =
        aiobscura_core::logging::init(&config.logging).context("failed to initialize logging")?;

    tracing::info!("aiobscura TUI starting up");

    // Resolve database path and then acquire process-level locks scoped to it.
    let db_path = Config::database_path();
    let process_guards = acquire_ui_guards(&db_path).context("failed to acquire process lock")?;
    if process_guards.mode == UiRunMode::ReadOnly {
        println!("aiobscura-sync is running; aiobscura will run in read-only mode.");
        tracing::info!("Running in read-only mode because sync lock is held");
    }

    // Open database
    tracing::info!(path = %db_path.display(), "Opening database");

    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    // Create a dedicated sync coordinator only when this process owns ingest.
    let sync_coordinator = if process_guards.mode == UiRunMode::OwnsIngest {
        let sync_db = Database::open(&db_path).context("failed to open sync database")?;
        sync_db
            .migrate()
            .context("failed to run sync database migrations")?;
        let coordinator = IngestCoordinator::new(sync_db);

        // Prime the database once at startup so Live view starts from current logs.
        if let Ok(result) = coordinator.sync_all() {
            tracing::info!(
                files_processed = result.files_processed,
                messages_inserted = result.messages_inserted,
                "Startup live sync complete"
            );
        }

        Some(coordinator)
    } else {
        None
    };

    // Create app and start in Live view (default tab)
    let mut app = App::new(db);
    app.start_live_view()
        .context("failed to load live messages")?;

    // Setup terminal
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    // Run the main loop
    let result = run_app(&mut terminal, &mut app, sync_coordinator.as_ref());

    // Restore terminal
    disable_raw_mode().context("failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;

    tracing::info!("aiobscura TUI shutting down");

    result
}

/// Run the main application loop.
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    sync_coordinator: Option<&IngestCoordinator>,
) -> Result<()> {
    // Poll counter for DB change detection (every 10 ticks = ~1 second)
    let mut poll_counter = 0u32;

    loop {
        // Every 10 ticks (~1 second), check for DB updates
        poll_counter += 1;
        if poll_counter >= 10 {
            poll_counter = 0;

            // In Live view, ingest fresh log data so the dashboard updates
            // even when aiobscura-sync is not running in parallel.
            if app.is_live_view() {
                if let Some(sync_coordinator) = sync_coordinator {
                    if let Err(e) = sync_coordinator.sync_all() {
                        tracing::warn!(error = %e, "Live sync iteration failed");
                    }
                }
            }

            // Only check and refresh if in a list view
            if app.is_list_view() {
                if let Ok(true) = app.check_for_updates() {
                    let _ = app.refresh_current_view();
                }
            }
        }

        // Update animations
        let size = terminal.size()?;
        app.tick_animation(size.width, size.height);

        // Render
        terminal.draw(|frame| ui::render(frame, app))?;

        // Handle events
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }

        // Check if we should quit
        if app.should_quit {
            break;
        }
    }

    Ok(())
}
