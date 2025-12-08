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
    /// Reason the file was skipped (if skipped)
    pub skip_reason: Option<SkipReason>,
}

/// Reason a file was skipped during sync.
#[derive(Debug, Clone)]
pub enum SkipReason {
    /// File was already fully parsed (checkpoint matches or exceeds file size)
    AlreadyParsed { checkpoint_offset: u64, file_size: u64 },
    /// File is empty
    EmptyFile,
    /// No new content since last parse
    NoNewContent,
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
    ///
    /// ## Spawn Linkage
    ///
    /// Agent spawn info is persisted to the `agent_spawns` table during parsing.
    /// This enables correct thread linkage even in incremental mode where main
    /// sessions may have been fully parsed in previous syncs.
    pub fn sync_all(&self) -> Result<SyncResult> {
        self.sync_all_with_progress(|_, _, _| {})
    }

    /// Sync all discovered files with progress callback.
    ///
    /// The callback receives `(current_file_index, total_files, file_path)` before
    /// each file is processed. This allows callers to display progress indicators.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// coordinator.sync_all_with_progress(|current, total, path| {
    ///     println!("Processing {}/{}: {}", current + 1, total, path.display());
    /// })?;
    /// ```
    pub fn sync_all_with_progress<F>(&self, mut on_progress: F) -> Result<SyncResult>
    where
        F: FnMut(usize, usize, &Path),
    {
        let files = self.discover_files()?;
        let total = files.len();
        let mut result = SyncResult::default();

        // Process all files - order doesn't matter since spawn info is DB-backed
        for (i, file) in files.iter().enumerate() {
            on_progress(i, total, &file.path);

            match self.sync_file_internal(&file.path) {
                Ok(file_result) => {
                    Self::update_result(&mut result, &file_result);
                }
                Err(e) => {
                    result.errors.push((file.path.clone(), e.to_string()));
                }
            }
        }

        Ok(result)
    }

    /// Update result counters from a file sync result.
    fn update_result(result: &mut SyncResult, file_result: &FileSyncResult) {
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

            // Log why the file was skipped
            let reason = match &file_result.skip_reason {
                Some(SkipReason::AlreadyParsed {
                    checkpoint_offset,
                    file_size,
                }) => format!(
                    "already parsed (checkpoint {} >= file size {})",
                    checkpoint_offset, file_size
                ),
                Some(SkipReason::EmptyFile) => "empty file".to_string(),
                Some(SkipReason::NoNewContent) => "no new content".to_string(),
                None => "unknown".to_string(),
            };
            tracing::debug!(
                path = %file_result.path.display(),
                reason = %reason,
                "File skipped"
            );
        }
        result.warnings.extend(file_result.warnings.clone());
    }

    /// Sync a single file.
    ///
    /// Loads the checkpoint from the database, parses new content,
    /// and stores the results.
    pub fn sync_file(&self, path: &Path) -> Result<FileSyncResult> {
        self.sync_file_internal(path)
    }

    /// Internal sync implementation.
    ///
    /// Handles spawn info persistence and lookup:
    /// - Main sessions: persist spawn map to DB after parsing
    /// - Agent files: look up spawn info from DB to link threads
    fn sync_file_internal(&self, path: &Path) -> Result<FileSyncResult> {
        // Find the parser for this file
        let parser = self
            .parser_for_file(path)
            .ok_or_else(|| crate::error::Error::Parse {
                agent: "unknown".to_string(),
                message: format!("No parser found for file: {}", path.display()),
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
            // Determine why the file was skipped
            let skip_reason = if file_size == 0 {
                Some(SkipReason::EmptyFile)
            } else if let Checkpoint::ByteOffset { offset } = &checkpoint {
                if *offset >= file_size {
                    Some(SkipReason::AlreadyParsed {
                        checkpoint_offset: *offset,
                        file_size,
                    })
                } else {
                    Some(SkipReason::NoNewContent)
                }
            } else {
                Some(SkipReason::NoNewContent)
            };

            return Ok(FileSyncResult {
                path: path.to_path_buf(),
                new_messages: 0,
                session_id: None,
                new_checkpoint: parse_result.new_checkpoint,
                is_new_session: false,
                warnings: parse_result.warnings,
                skip_reason,
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
            created_at: existing
                .as_ref()
                .map(|s| s.created_at)
                .unwrap_or_else(Utc::now),
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

        // Store backing model (before session, since session references it)
        if let Some(ref session) = parse_result.session {
            if let Some(ref model_id) = session.backing_model_id {
                let backing_model = crate::types::BackingModel::from_id(model_id);
                self.db.upsert_backing_model(&backing_model)?;
            }
        }

        // Store session
        let session_id = parse_result.session.as_ref().map(|s| s.id.clone());
        let is_new_session = existing.is_none() && parse_result.session.is_some();

        if let Some(session) = &parse_result.session {
            self.db.upsert_session(session)?;
        }

        // Persist spawn info for main sessions (enables incremental agent linking)
        if !is_agent_file(path) {
            if let Some(ref sid) = session_id {
                for (agent_id, spawning_seq) in &parse_result.agent_spawn_map {
                    self.db.upsert_agent_spawn(agent_id, sid, *spawning_seq)?;
                }
            }
        }

        // Store threads
        for thread in &parse_result.threads {
            // Check if thread already exists
            let existing_threads = self.db.get_session_threads(&thread.session_id)?;
            if !existing_threads.iter().any(|t| t.id == thread.id) {
                self.db.insert_thread(thread)?;
            }
        }

        // For agent files, look up spawn info from DB and link threads
        if is_agent_file(path) {
            if let Some(agent_id) = extract_agent_id(path) {
                if let Some(spawn_info) = self.db.get_agent_spawn(&agent_id)? {
                    for thread in &parse_result.threads {
                        self.update_thread_spawn_info(
                            &thread.id,
                            spawn_info.spawning_message_seq,
                            &spawn_info.session_id,
                        )?;
                    }
                }
            }
        }

        // Store messages
        let new_messages = parse_result.messages.len();
        if !parse_result.messages.is_empty() {
            self.db.insert_messages(&parse_result.messages)?;
        }

        // Store plans and link to session
        if let Some(ref sid) = session_id {
            for plan in &parse_result.plans {
                // Upsert plan version (deduplicates by content hash)
                self.db.upsert_plan_version(plan)?;

                // Link session to plan
                let first_used_at = parse_result
                    .session
                    .as_ref()
                    .map(|s| s.started_at)
                    .unwrap_or_else(Utc::now);
                self.db.link_session_plan(sid, &plan.id, first_used_at)?;
            }
        }

        Ok(FileSyncResult {
            path: path.to_path_buf(),
            new_messages,
            session_id,
            new_checkpoint: parse_result.new_checkpoint,
            is_new_session,
            warnings: parse_result.warnings,
            skip_reason: None,
        })
    }

    /// Update thread spawn info after parsing an agent file.
    ///
    /// Sets `spawned_by_message_id` and `parent_thread_id` for the given thread.
    fn update_thread_spawn_info(
        &self,
        thread_id: &str,
        spawning_seq: i64,
        session_id: &str,
    ) -> Result<()> {
        let main_thread_id = format!("{}-main", session_id);
        self.db
            .update_thread_spawn_info(thread_id, spawning_seq, &main_thread_id)
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

/// Check if a file is an agent file (agent-*.jsonl pattern).
fn is_agent_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.starts_with("agent-"))
        .unwrap_or(false)
}

/// Extract agent ID from an agent file path.
///
/// Given `agent-a4767a09.jsonl`, returns `Some("a4767a09")`.
fn extract_agent_id(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;
    if !file_name.starts_with("agent-") {
        return None;
    }

    // Strip "agent-" prefix and ".jsonl" suffix
    let stem = path.file_stem()?.to_str()?;
    stem.strip_prefix("agent-").map(|s| s.to_string())
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

    #[test]
    fn test_is_agent_file() {
        assert!(is_agent_file(Path::new("agent-a4767a09.jsonl")));
        assert!(is_agent_file(Path::new("/path/to/agent-10b3de07.jsonl")));
        assert!(!is_agent_file(Path::new(
            "b4749c81-937a-4bd4-b62c-9d78905f0975.jsonl"
        )));
        assert!(!is_agent_file(Path::new("session.jsonl")));
    }

    #[test]
    fn test_extract_agent_id() {
        assert_eq!(
            extract_agent_id(Path::new("agent-a4767a09.jsonl")),
            Some("a4767a09".to_string())
        );
        assert_eq!(
            extract_agent_id(Path::new("/path/to/agent-10b3de07.jsonl")),
            Some("10b3de07".to_string())
        );
        assert_eq!(
            extract_agent_id(Path::new("b4749c81-937a-4bd4-b62c-9d78905f0975.jsonl")),
            None
        );
        assert_eq!(
            extract_agent_id(Path::new("agent-.jsonl")),
            Some("".to_string())
        );
    }
}
