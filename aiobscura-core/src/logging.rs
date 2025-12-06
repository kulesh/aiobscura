//! Logging infrastructure for aiobscura
//!
//! Logs are written to `~/.local/state/aiobscura/aiobscura.log` following XDG standards.

use crate::config::{Config, LoggingConfig};
use std::path::PathBuf;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

/// Initialize the logging system
///
/// Sets up tracing with:
/// - File output to XDG state directory
/// - Log rotation
/// - Configurable log level via config or RUST_LOG env var
pub fn init(config: &LoggingConfig) -> crate::error::Result<LoggingGuard> {
    let log_dir = Config::state_dir();

    // Create log directory if it doesn't exist
    std::fs::create_dir_all(&log_dir)?;

    // Create file appender with daily rotation
    let file_appender = RollingFileAppender::new(Rotation::DAILY, &log_dir, "aiobscura.log");

    // Non-blocking writer for better performance
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Build the filter from config or env var
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(&config.level)
    });

    // File layer - structured logging with timestamps
    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true);

    // Initialize the subscriber
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .init();

    tracing::info!(
        log_dir = %log_dir.display(),
        level = %config.level,
        "Logging initialized"
    );

    Ok(LoggingGuard { _guard: guard })
}

/// Initialize logging for tests (logs to stdout)
pub fn init_test() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_test_writer()
        .with_span_events(FmtSpan::CLOSE)
        .try_init();
}

/// Guard that keeps the logging system alive
///
/// When dropped, flushes any pending log writes.
pub struct LoggingGuard {
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Returns the log file path
pub fn log_file_path() -> PathBuf {
    Config::log_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_file_path() {
        let path = log_file_path();
        assert!(path.ends_with("aiobscura.log"));
    }
}
