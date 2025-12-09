//! Database layer for aiobscura
//!
//! This module provides the storage layer using SQLite with:
//! - Schema migrations
//! - Repository pattern for queries
//! - Checkpoint tracking for incremental ingestion

pub mod repo;
pub mod schema;

pub use repo::{Database, FileStats, SessionFilter, TokenUsage, ToolStats};
