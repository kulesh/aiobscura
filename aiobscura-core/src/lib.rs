//! # aiobscura-core
//!
//! Core library for aiobscura - an AI agent activity monitor.
//!
//! This library provides:
//! - Domain types for sessions, events, and plans
//! - Database storage layer with SQLite
//! - Configuration management
//! - Logging infrastructure
//!
//! ## Architecture
//!
//! Data flows through three layers:
//! - **Layer 0 (Raw):** Source files on disk (immutable)
//! - **Layer 1 (Canonical):** Normalized SQLite tables with full `raw_data` preservation
//! - **Layer 2 (Derived):** Computed metrics and assessments (regenerable)
//!
//! ## Example
//!
//! ```rust,no_run
//! use aiobscura_core::{Config, Database};
//!
//! // Load configuration
//! let config = Config::load().expect("failed to load config");
//!
//! // Open database
//! let db = Database::open(&Config::database_path()).expect("failed to open database");
//! db.migrate().expect("failed to run migrations");
//! ```

// Re-export commonly used items at the crate root
pub use config::Config;
pub use db::{Database, SessionFilter};
pub use error::{Error, Result};
pub use ingest::{IngestCoordinator, SyncResult};
pub use types::*;

// Public modules
pub mod analytics;
pub mod config;
pub mod db;
pub mod error;
pub mod ingest;
pub mod logging;
pub mod types;
