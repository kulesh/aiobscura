//! Ingestion layer for parsing assistant log files
//!
//! This module orchestrates the parsing of raw log files (Layer 0) into
//! canonical database records (Layer 1).
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
//! │  Source Files   │ ──► │ IngestCoordinator│ ──► │    Database     │
//! │ (~/.claude/...) │     │                  │     │ (sessions, etc) │
//! └─────────────────┘     └──────────────────┘     └─────────────────┘
//!                               │
//!                               ▼
//!                    ┌──────────────────────┐
//!                    │  AssistantParser     │
//!                    │  ├─ ClaudeCodeParser │
//!                    │  ├─ CodexParser      │
//!                    │  └─ ...              │
//!                    └──────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aiobscura_core::{Database, Config};
//! use aiobscura_core::ingest::IngestCoordinator;
//!
//! let db = Database::open(&Config::database_path())?;
//! let coordinator = IngestCoordinator::new(db);
//!
//! // Sync all discovered files
//! let result = coordinator.sync_all()?;
//! println!("Synced {} messages from {} files", result.messages_inserted, result.files_processed);
//! ```

mod parser;
pub mod parsers;

pub use parser::{AssistantParser, ParseContext, ParseResult, SourcePattern};

use crate::db::Database;
use crate::error::Result;
use crate::types::{Checkpoint, SourceFile};
use chrono::Utc;
use std::path::{Path, PathBuf};

/// Result of a full sync operation across all assistants.
#[derive(Debug, Default)]
pub struct SyncResult {
    /// Number of files processed
    pub files_processed: usize,
    /// Number of files skipped (no changes)
    pub files_skipped: usize,
    /// Number of new sessions created
    pub sessions_created: usize,
    /// Number of existing sessions updated
    pub sessions_updated: usize,
    /// Number of messages inserted
    pub messages_inserted: usize,
    /// Number of threads created
    pub threads_created: usize,
    /// Errors encountered (file path → error message)
    pub errors: Vec<(PathBuf, String)>,
    /// Warnings from parsing
    pub warnings: Vec<String>,
}

/// Result of syncing a single file.
#[derive(Debug)]
pub struct FileSyncResult {
    /// Path to the synced file
    pub path: PathBuf,
    /// Number of new messages parsed
    pub new_messages: usize,
    /// Session ID (if session was created/updated)
    pub session_id: Option<String>,
    /// Updated checkpoint for next sync
    pub new_checkpoint: Checkpoint,
    /// Whether this was a new session
    pub is_new_session: bool,
    /// Warnings from parsing
    pub warnings: Vec<String>,
}

/// Coordinates ingestion across all registered parsers.
///
/// The coordinator is responsible for:
/// - Discovering source files using parser patterns
/// - Loading checkpoints from the database
/// - Calling parsers to extract data
/// - Storing results via the repository layer
pub struct IngestCoordinator {
    db: Database,
    parsers: Vec<Box<dyn AssistantParser>>,
}

impl IngestCoordinator {
    /// Create a new coordinator with the default parsers.
    pub fn new(db: Database) -> Self {
        Self {
            db,
            parsers: parsers::create_all_parsers(),
        }
    }

    /// Create a coordinator with custom parsers.
    pub fn with_parsers(db: Database, parsers: Vec<Box<dyn AssistantParser>>) -> Self {
        Self { db, parsers }
    }

    /// Register an additional parser.
    pub fn register_parser(&mut self, parser: Box<dyn AssistantParser>) {
        self.parsers.push(parser);
    }

    /// Get the list of installed assistants.
    pub fn installed_assistants(&self) -> Vec<&dyn AssistantParser> {
        self.parsers
            .iter()
            .filter(|p| p.is_installed())
            .map(|p| p.as_ref())
            .collect()
    }

    /// Discover all source files for all registered assistants.
    pub fn discover_files(&self) -> Result<Vec<SourceFile>> {
        let mut all_files = Vec::new();

        for parser in &self.parsers {
            if !parser.is_installed() {
                tracing::debug!(
                    assistant = %parser.assistant().display_name(),
                    "Assistant not installed, skipping"
                );
                continue;
            }

            match parser.discover_files() {
                Ok(files) => {
                    tracing::info!(
                        assistant = %parser.assistant().display_name(),
                        count = files.len(),
                        "Discovered source files"
                    );
                    all_files.extend(files);
                }
                Err(e) => {
                    tracing::warn!(
                        assistant = %parser.assistant().display_name(),
                        error = %e,
                        "Failed to discover files"
                    );
                }
            }
        }

        Ok(all_files)
    }

    /// Sync all discovered files (full sync).
    ///
    /// This discovers all source files and syncs each one, respecting
    /// checkpoints for incremental parsing.
    pub fn sync_all(&self) -> Result<SyncResult> {
        let files = self.discover_files()?;
        let mut result = SyncResult::default();

        for file in files {
            match self.sync_file(&file.path) {
                Ok(file_result) => {
                    if file_result.new_messages > 0 {
                        result.files_processed += 1;
                        result.messages_inserted += file_result.new_messages;
                        if file_result.is_new_session {
                            result.sessions_created += 1;
                        } else if file_result.session_id.is_some() {
                            result.sessions_updated += 1;
                        }
                    } else {
                        result.files_skipped += 1;
                    }
                    result.warnings.extend(file_result.warnings);
                }
                Err(e) => {
                    result.errors.push((file.path.clone(), e.to_string()));
                }
            }
        }

        Ok(result)
    }

    /// Sync a single file.
    ///
    /// Loads the checkpoint from the database, parses new content,
    /// and stores the results.
    pub fn sync_file(&self, path: &Path) -> Result<FileSyncResult> {
        // Find the parser for this file
        let parser = self.parser_for_file(path).ok_or_else(|| {
            crate::error::Error::Parse {
                agent: "unknown".to_string(),
                message: format!("No parser found for file: {}", path.display()),
            }
        })?;

        // Get existing source file record (for checkpoint)
        let existing = self.db.get_source_file(&path.to_string_lossy())?;
        let checkpoint = existing
            .as_ref()
            .map(|s| s.checkpoint.clone())
            .unwrap_or(Checkpoint::None);

        // Get file metadata
        let metadata = std::fs::metadata(path)?;
        let file_size = metadata.len();
        let modified_at = metadata
            .modified()
            .ok()
            .map(chrono::DateTime::from)
            .unwrap_or_else(Utc::now);

        // Create parse context
        let ctx = ParseContext {
            path,
            checkpoint: &checkpoint,
            file_size,
            modified_at,
        };

        // Parse the file
        let parse_result = parser.parse(&ctx)?;

        // Check if there's anything new
        if parse_result.messages.is_empty()
            && parse_result.session.is_none()
            && parse_result.threads.is_empty()
        {
            return Ok(FileSyncResult {
                path: path.to_path_buf(),
                new_messages: 0,
                session_id: None,
                new_checkpoint: parse_result.new_checkpoint,
                is_new_session: false,
                warnings: parse_result.warnings,
            });
        }

        // Store source file with updated checkpoint
        let source_file = SourceFile {
            path: path.to_path_buf(),
            file_type: existing
                .as_ref()
                .map(|s| s.file_type)
                .unwrap_or(crate::types::FileType::Jsonl),
            assistant: parser.assistant(),
            created_at: existing.as_ref().map(|s| s.created_at).unwrap_or_else(Utc::now),
            modified_at,
            size_bytes: file_size,
            last_parsed_at: Some(Utc::now()),
            checkpoint: parse_result.new_checkpoint.clone(),
        };
        self.db.upsert_source_file(&source_file)?;

        // Store project (before session, since session references it)
        if let Some(project) = &parse_result.project {
            self.db.upsert_project(project)?;
        }

        // Store session
        let session_id = parse_result.session.as_ref().map(|s| s.id.clone());
        let is_new_session = existing.is_none() && parse_result.session.is_some();

        if let Some(session) = &parse_result.session {
            self.db.upsert_session(session)?;
        }

        // Store threads
        for thread in &parse_result.threads {
            // Check if thread already exists
            let existing_threads = self.db.get_session_threads(&thread.session_id)?;
            if !existing_threads.iter().any(|t| t.id == thread.id) {
                self.db.insert_thread(thread)?;
            }
        }

        // Store messages
        let new_messages = parse_result.messages.len();
        if !parse_result.messages.is_empty() {
            self.db.insert_messages(&parse_result.messages)?;
        }

        Ok(FileSyncResult {
            path: path.to_path_buf(),
            new_messages,
            session_id,
            new_checkpoint: parse_result.new_checkpoint,
            is_new_session,
            warnings: parse_result.warnings,
        })
    }

    /// Find the parser that handles a given file.
    fn parser_for_file(&self, path: &Path) -> Option<&dyn AssistantParser> {
        for parser in &self.parsers {
            if let Some(root) = parser.root_path() {
                if path.starts_with(&root) {
                    return Some(parser.as_ref());
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_result_default() {
        let result = SyncResult::default();
        assert_eq!(result.files_processed, 0);
        assert_eq!(result.messages_inserted, 0);
        assert!(result.errors.is_empty());
    }
}
