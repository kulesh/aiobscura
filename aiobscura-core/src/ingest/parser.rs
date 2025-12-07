//! Parser trait abstraction
//!
//! All assistant parsers implement the [`AssistantParser`] trait to provide
//! a unified interface for discovering and parsing log files.
//!
//! ## Design Principles
//!
//! 1. **Lossless capture**: Every parsed record preserves complete `raw_data`
//! 2. **Resilience**: Parse failures for individual records log warnings but continue
//! 3. **Incremental**: Checkpoints enable resuming from last parsed position
//! 4. **Extensible**: New assistants only require implementing this trait

use crate::error::Result;
use crate::types::{
    Assistant, Checkpoint, FileType, Message, Plan, Project, Session, SourceFile, Thread,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Pattern for discovering source files for an assistant.
///
/// Each assistant may have multiple patterns (e.g., session files, plan files).
#[derive(Debug, Clone)]
pub struct SourcePattern {
    /// Glob pattern relative to assistant root (e.g., "projects/*/*.jsonl")
    pub pattern: String,
    /// File type determines checkpoint strategy
    pub file_type: FileType,
    /// Human-readable description for logging
    pub description: String,
}

/// Result of parsing a source file.
///
/// Contains all entities extracted from the file, plus the updated checkpoint
/// for incremental parsing.
#[derive(Debug, Default)]
pub struct ParseResult {
    /// Project inferred from cwd (auto-created)
    pub project: Option<Project>,
    /// Session to upsert (may be new or updated)
    pub session: Option<Session>,
    /// Threads to insert (typically just main thread on first parse)
    pub threads: Vec<Thread>,
    /// Messages to insert
    pub messages: Vec<Message>,
    /// Plans discovered (for plan file parsers)
    pub plans: Vec<Plan>,
    /// Updated checkpoint for next incremental parse
    pub new_checkpoint: Checkpoint,
    /// Warnings encountered during parsing (non-fatal)
    pub warnings: Vec<String>,
    /// Map of agentId -> spawning message seq (for linking agent threads)
    ///
    /// Populated when parsing main session files that contain Task tool results.
    /// Used by IngestCoordinator to set `Thread.spawned_by_message_id` when
    /// parsing agent files.
    pub agent_spawn_map: HashMap<String, i64>,
}

/// Context passed to parser with file metadata and checkpoint info.
pub struct ParseContext<'a> {
    /// Path to the source file
    pub path: &'a Path,
    /// Current checkpoint (for incremental parsing)
    pub checkpoint: &'a Checkpoint,
    /// File size in bytes (for truncation detection)
    pub file_size: u64,
    /// Last modified time
    pub modified_at: chrono::DateTime<chrono::Utc>,
}

/// Trait implemented by all assistant parsers.
///
/// Each supported assistant (Claude Code, Codex, Aider, Cursor) has a parser
/// that implements this trait.
///
/// ## Example
///
/// ```rust,ignore
/// use aiobscura_core::ingest::{AssistantParser, ParseContext, ParseResult};
///
/// struct MyParser;
///
/// impl AssistantParser for MyParser {
///     fn assistant(&self) -> Assistant { Assistant::ClaudeCode }
///     // ... implement other methods
/// }
/// ```
pub trait AssistantParser: Send + Sync {
    /// Which assistant this parser handles
    fn assistant(&self) -> Assistant;

    /// Root directory for this assistant's data (e.g., ~/.claude)
    ///
    /// Returns `None` if the path cannot be determined (e.g., $HOME not set).
    fn root_path(&self) -> Option<PathBuf>;

    /// Check if this assistant is installed (root path exists)
    fn is_installed(&self) -> bool {
        self.root_path().map(|p| p.exists()).unwrap_or(false)
    }

    /// Patterns for discovering source files.
    ///
    /// Patterns are relative to [`Self::root_path`]. Each pattern includes
    /// the file type for checkpoint strategy selection.
    fn source_patterns(&self) -> Vec<SourcePattern>;

    /// Parse a source file, starting from the given checkpoint.
    ///
    /// ## Incremental Parsing
    ///
    /// If `ctx.checkpoint` is `ByteOffset { offset }`, parsing should resume
    /// from that byte position. If the offset exceeds file size (truncation),
    /// reset to beginning and log a warning.
    ///
    /// ## Error Handling
    ///
    /// - Individual record parse failures should be logged as warnings
    ///   and added to `ParseResult::warnings`, not returned as errors
    /// - Only fatal errors (file not found, I/O errors) should return `Err`
    fn parse(&self, ctx: &ParseContext) -> Result<ParseResult>;

    /// Extract project path from source file path.
    ///
    /// This is assistant-specific because each assistant encodes paths differently.
    /// Returns `None` if the path cannot be determined.
    fn extract_project_path(&self, file_path: &Path) -> Option<PathBuf>;

    /// Extract session ID from source file path.
    ///
    /// For most assistants, this is the file stem (e.g., UUID from `{uuid}.jsonl`).
    fn extract_session_id(&self, file_path: &Path) -> Option<String>;

    /// Discover all source files matching this parser's patterns.
    ///
    /// Default implementation uses glob patterns from [`Self::source_patterns`].
    fn discover_files(&self) -> Result<Vec<SourceFile>> {
        let root = match self.root_path() {
            Some(r) => r,
            None => return Ok(vec![]),
        };

        let mut files = Vec::new();

        for pattern in self.source_patterns() {
            let full_pattern = root.join(&pattern.pattern);
            let pattern_str = full_pattern.to_string_lossy();

            let entries = glob::glob(&pattern_str).map_err(|e| crate::error::Error::Parse {
                agent: self.assistant().to_string(),
                message: format!("Invalid glob pattern: {}", e),
            })?;

            for entry in entries.flatten() {
                let metadata = std::fs::metadata(&entry).ok();
                let now = chrono::Utc::now();
                let (size, modified, created) = metadata
                    .map(|m| {
                        (
                            m.len(),
                            m.modified().ok().map(chrono::DateTime::from).unwrap_or(now),
                            m.created().ok().map(chrono::DateTime::from).unwrap_or(now),
                        )
                    })
                    .unwrap_or((0, now, now));

                files.push(SourceFile {
                    path: entry,
                    file_type: pattern.file_type,
                    assistant: self.assistant(),
                    created_at: created,
                    modified_at: modified,
                    size_bytes: size,
                    last_parsed_at: None,
                    checkpoint: Checkpoint::None,
                });
            }
        }

        Ok(files)
    }
}
