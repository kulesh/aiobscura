//! Error types for aiobscura-core

use thiserror::Error;

/// Main error type for the aiobscura-core library
#[derive(Error, Debug)]
pub enum Error {
    /// Database error
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Parse error for agent logs
    #[error("parse error in {agent} log: {message}")]
    Parse { agent: String, message: String },

    /// JSON parsing error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Configuration error
    #[error("configuration error: {0}")]
    Config(String),

    /// LLM error
    #[error("LLM error: {0}")]
    Llm(String),

    /// Session not found
    #[error("session not found: {0}")]
    SessionNotFound(String),

    /// Plan not found
    #[error("plan not found: {0}")]
    PlanNotFound(String),

    /// Collector/API error
    #[error("collector error: {0}")]
    Collector(String),
}

/// Result type alias for aiobscura-core
pub type Result<T> = std::result::Result<T, Error>;
