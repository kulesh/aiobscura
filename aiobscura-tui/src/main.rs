//! aiobscura - AI Agent Activity Monitor
//!
//! Terminal UI for observing, querying, and analyzing AI coding agent activity.

mod app;
mod thread_row;
mod ui;

use std::io;

use aiobscura_core::{Config, Database};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::App;

fn main() -> Result<()> {
    // Load configuration
    let config = Config::load().context("failed to load configuration")?;

    // Initialize logging (to file, not stdout since we have a TUI)
    let _log_guard =
        aiobscura_core::logging::init(&config.logging).context("failed to initialize logging")?;

    tracing::info!("aiobscura TUI starting up");

    // Open database
    let db_path = Config::database_path();
    tracing::info!(path = %db_path.display(), "Opening database");

    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    // Create app and load data
    let mut app = App::new(db);
    app.load_threads().context("failed to load threads")?;

    // Setup terminal
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    // Run the main loop
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode().context("failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;

    tracing::info!("aiobscura TUI shutting down");

    result
}

/// Run the main application loop.
fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
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
