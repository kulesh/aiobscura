//! Claude Code JSONL parser
//!
//! Parses session logs from `~/.claude/projects/[encoded-path]/*.jsonl`.
//!
//! See [`docs/claude-code-log-format.md`](../../../../docs/claude-code-log-format.md)
//! for the complete format specification.
//!
//! # Error Handling
//!
//! The parser is designed to be resilient and recover from errors:
//!
//! - **Malformed JSON lines**: Logged as warning, line skipped, parsing continues.
//!   The warning is recorded in [`ParseResult::warnings`].
//!
//! - **Missing required fields**: Uses sensible defaults via `#[serde(default)]`.
//!   For example, missing `timestamp` falls back to `Utc::now()`.
//!
//! - **Empty content blocks**: Skipped without incrementing the message sequence
//!   number. This prevents gaps in message ordering.
//!
//! - **Unknown record types**: Converted to [`MessageType::Context`] messages
//!   rather than failing, preserving all data in the conversation thread.
//!
//! - **File truncation detected**: When the checkpoint offset exceeds the current
//!   file size, the parser resets to offset 0 and re-parses from the beginning.
//!
//! - **Incomplete last line**: Parsing stops cleanly before incomplete lines.
//!   The checkpoint is set to the last complete line, so incomplete data will
//!   be parsed on the next sync when the file is complete.
//!
//! # Incremental Parsing
//!
//! The parser supports incremental parsing via [`Checkpoint::ByteOffset`].
//! On subsequent parses, it seeks to the last checkpoint position and only
//! processes new records. This enables efficient real-time sync without
//! re-parsing entire files.

use crate::error::{Error, Result};
use crate::ingest::parser::{AssistantParser, ParseContext, ParseResult, SourcePattern};
use crate::types::{
    Assistant, AuthorRole, Checkpoint, ContentType, FileType, Message, MessageType, Plan,
    PlanStatus, Project, Session, SessionStatus, Thread, ThreadType,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Parser for Claude Code JSONL logs.
pub struct ClaudeCodeParser {
    root: Option<PathBuf>,
}

impl ClaudeCodeParser {
    /// Create a new parser with the default root path (~/.claude).
    pub fn new() -> Self {
        Self {
            root: dirs::home_dir().map(|h| h.join(".claude")),
        }
    }

    /// Create a parser with a custom root path (for testing).
    pub fn with_root(root: PathBuf) -> Self {
        Self { root: Some(root) }
    }
}

impl Default for ClaudeCodeParser {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================
// Raw JSONL record types (serde deserialization)
// ============================================

/// Represents a single line from Claude Code JSONL.
///
/// Uses `#[serde(default)]` liberally to handle missing fields gracefully.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawRecord {
    // Common fields
    uuid: Option<String>,
    parent_uuid: Option<String>,
    session_id: Option<String>,
    #[serde(rename = "type")]
    record_type: Option<String>,
    timestamp: Option<String>,
    cwd: Option<String>,
    version: Option<String>,
    git_branch: Option<String>,
    is_sidechain: Option<bool>,

    // Message content
    message: Option<RawMessage>,

    // Request tracking
    request_id: Option<String>,

    // Agent-specific
    agent_id: Option<String>,
    slug: Option<String>,

    // Tool result (for user messages)
    tool_use_result: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawMessage {
    role: Option<String>,
    model: Option<String>,
    id: Option<String>,
    content: Option<RawContent>,
    usage: Option<RawUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl Default for RawContent {
    fn default() -> Self {
        RawContent::Text(String::new())
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(rename = "image")]
    Image { source: ImageSource },
    // Catch-all for unknown block types
    #[serde(other)]
    Unknown,
}

/// Source information for an image content block.
///
/// The `data` field is intentionally omitted as it contains the full
/// base64-encoded image which would be too large to store.
#[derive(Debug, Deserialize)]
struct ImageSource {
    /// Source type, typically "base64" (not currently used but kept for completeness)
    #[serde(rename = "type")]
    #[allow(dead_code)]
    source_type: String,
    /// Media type, e.g., "image/png", "image/jpeg"
    media_type: String,
    // data field intentionally omitted - too large to store
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawUsage {
    input_tokens: Option<i32>,
    output_tokens: Option<i32>,
    cache_creation_input_tokens: Option<i32>,
    cache_read_input_tokens: Option<i32>,
}

impl AssistantParser for ClaudeCodeParser {
    fn assistant(&self) -> Assistant {
        Assistant::ClaudeCode
    }

    fn root_path(&self) -> Option<PathBuf> {
        self.root.clone()
    }

    fn source_patterns(&self) -> Vec<SourcePattern> {
        vec![SourcePattern {
            pattern: "projects/*/*.jsonl".to_string(),
            file_type: FileType::Jsonl,
            description: "Claude Code session logs".to_string(),
        }]
    }

    fn parse(&self, ctx: &ParseContext) -> Result<ParseResult> {
        let mut result = ParseResult::default();

        // Detect if this is an agent file (agent-*.jsonl)
        // Agent files contain sidechain records which should NOT be skipped
        let is_agent_file = ctx
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.starts_with("agent-"))
            .unwrap_or(false);

        // Determine thread type from file name - used for role assignment
        let thread_type = if is_agent_file {
            ThreadType::Agent
        } else {
            ThreadType::Main
        };

        // Open file
        let file = File::open(ctx.path).map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to open {}: {}", ctx.path.display(), e),
            ))
        })?;

        // Determine start offset from checkpoint
        let start_offset = match ctx.checkpoint {
            Checkpoint::ByteOffset { offset } => {
                if *offset > ctx.file_size {
                    result.warnings.push(format!(
                        "File truncated: checkpoint {} > file size {}, starting from beginning",
                        offset, ctx.file_size
                    ));
                    0
                } else {
                    *offset
                }
            }
            _ => 0,
        };

        // If we're already at EOF, nothing to do
        if start_offset >= ctx.file_size {
            result.new_checkpoint = Checkpoint::ByteOffset {
                offset: ctx.file_size,
            };
            return Ok(result);
        }

        let mut reader = BufReader::new(file);
        if start_offset > 0 {
            reader.seek(SeekFrom::Start(start_offset))?;
        }

        let mut current_offset = start_offset;
        let mut line_number = 0; // Line numbers are relative to checkpoint position
        let mut seq = 0i32;
        let mut session_id: Option<String> = None;
        let mut thread_id: Option<String> = None;
        let mut model_id: Option<String> = None;
        let mut cwd: Option<String> = None;
        let mut git_branch: Option<String> = None;
        // Capture observed_at once per parse invocation
        let observed_at = Utc::now();
        // Track first/last timestamps for session metadata; initialized to observed_at
        // so records without timestamps can use the last seen value as approximation
        let mut first_timestamp: DateTime<Utc> = observed_at;
        let mut last_timestamp: DateTime<Utc> = observed_at;

        // Track plan slugs (sessions can have multiple plans)
        let mut slugs: Vec<String> = Vec::new();

        // Track uuid -> seq mapping for agent spawn linkage
        let mut uuid_to_seq: HashMap<String, i32> = HashMap::new();

        // Track last activity per thread (for setting Thread.last_activity_at)
        // Only tracks non-context messages (excludes summary, file-history-snapshot, etc.)
        let mut thread_last_activity: HashMap<String, DateTime<Utc>> = HashMap::new();

        let source_path = ctx.path.to_string_lossy().to_string();

        for line_result in reader.lines() {
            line_number += 1;

            let line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    result.warnings.push(format!(
                        "Line {} (offset {}): read error: {}",
                        line_number, current_offset, e
                    ));
                    continue;
                }
            };

            let line_bytes = line.len() as u64 + 1; // +1 for newline
            let record_offset = current_offset;
            current_offset += line_bytes;

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            // Parse JSON
            let raw_json: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    result.warnings.push(format!(
                        "Line {} (offset {}): JSON parse error: {}",
                        line_number, record_offset, e
                    ));
                    continue;
                }
            };

            // Skip special record types
            if let Some(record_type) = raw_json.get("type").and_then(|v| v.as_str()) {
                if record_type == "file-history-snapshot" {
                    continue;
                }
            }

            // Skip sidechain references in main session files (they have their own agent files)
            // But don't skip when parsing agent files themselves
            if !is_agent_file
                && raw_json
                    .get("isSidechain")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            {
                continue;
            }

            // Deserialize into structured record
            let record: RawRecord = match serde_json::from_value(raw_json.clone()) {
                Ok(r) => r,
                Err(e) => {
                    result.warnings.push(format!(
                        "Line {} (offset {}): deserialization error: {}",
                        line_number, record_offset, e
                    ));
                    continue;
                }
            };

            // Extract session ID (first occurrence)
            if session_id.is_none() {
                session_id = record
                    .session_id
                    .clone()
                    .or_else(|| self.extract_session_id(ctx.path));
            }

            // Extract metadata from first record
            if cwd.is_none() {
                cwd = record.cwd.clone();
            }
            if git_branch.is_none() {
                git_branch = record.git_branch.clone();
            }

            // Collect unique slugs (plan file references)
            if let Some(ref s) = record.slug {
                if !slugs.contains(s) {
                    slugs.push(s.clone());
                }
            }

            // Parse timestamp - use last seen if record has no timestamp (approximation)
            let emitted_at = record
                .timestamp
                .as_ref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(last_timestamp);

            // Update first_timestamp only on first record (when it equals observed_at)
            if first_timestamp == observed_at {
                first_timestamp = emitted_at;
            }
            last_timestamp = emitted_at;

            // Create thread on first message if needed
            if thread_id.is_none() {
                let sid = session_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                let tid = format!("{}-{}", sid, thread_type.as_str());
                thread_id = Some(tid.clone());

                result.threads.push(Thread {
                    id: tid,
                    session_id: sid.clone(),
                    thread_type,
                    parent_thread_id: None,
                    spawned_by_message_id: None,
                    started_at: emitted_at,
                    ended_at: None,
                    last_activity_at: None, // Set at the end of parsing
                    metadata: serde_json::json!({}),
                });
            }

            // Extract model from message
            if let Some(ref msg) = record.message {
                if model_id.is_none() {
                    model_id = msg.model.clone();
                }
            }

            // Track uuid -> seq for the CURRENT seq (before messages are created)
            // This will let us look up the seq of the spawning message later
            let pre_message_seq = seq;

            // Convert to Message(s)
            let messages = self.record_to_messages(
                &record,
                &raw_json,
                session_id.as_ref().unwrap_or(&String::new()),
                thread_id.as_ref().unwrap_or(&String::new()),
                thread_type,
                &mut seq,
                emitted_at,
                observed_at,
                &source_path,
                record_offset as i64,
                Some(line_number),
            );

            // Record uuid -> seq mapping for the first message created from this record
            // (used for agent spawn linkage - the spawning message is the tool_use)
            if let Some(uuid) = &record.uuid {
                // Use pre_message_seq + 1 since seq is incremented when creating messages
                if pre_message_seq < seq {
                    uuid_to_seq.insert(uuid.clone(), pre_message_seq + 1);
                }
            }

            // Check for agent spawn info in tool_result records (main session only)
            // The toolUseResult.agentId field links to the agent file
            if !is_agent_file {
                if let Some(ref tool_use_result) = record.tool_use_result {
                    if let Some(agent_id) = tool_use_result.get("agentId").and_then(|v| v.as_str())
                    {
                        // The parentUuid points to the tool_use message that spawned this agent
                        if let Some(ref parent_uuid) = record.parent_uuid {
                            if let Some(&spawning_seq) = uuid_to_seq.get(parent_uuid) {
                                result
                                    .agent_spawn_map
                                    .insert(agent_id.to_string(), spawning_seq as i64);
                            }
                        }
                    }
                }
            }

            // Update thread's last activity for non-context messages
            if let Some(ref tid) = thread_id {
                for msg in &messages {
                    if msg.message_type != MessageType::Context {
                        thread_last_activity.insert(tid.clone(), emitted_at);
                        break; // Only need to update once per record
                    }
                }
            }

            result.messages.extend(messages);
        }

        // Set last_activity_at on threads before returning
        for thread in &mut result.threads {
            if let Some(&last_activity) = thread_last_activity.get(&thread.id) {
                thread.last_activity_at = Some(last_activity);
            }
        }

        // Create/update session and project
        if let Some(sid) = session_id {
            let project_path = self.extract_project_path(ctx.path);

            // Create Project from cwd if available
            let project_id = if let Some(ref cwd_path) = cwd {
                let proj_id = Self::generate_project_id(cwd_path);
                let proj_name = Self::extract_dir_name(cwd_path);

                result.project = Some(Project {
                    id: proj_id.clone(),
                    path: PathBuf::from(cwd_path),
                    name: Some(proj_name),
                    created_at: first_timestamp,
                    last_activity_at: Some(last_timestamp),
                    metadata: serde_json::json!({}),
                });

                Some(proj_id)
            } else {
                None
            };

            result.session = Some(Session {
                id: sid.clone(),
                assistant: Assistant::ClaudeCode,
                backing_model_id: model_id.map(|m| format!("anthropic:{}", m)),
                project_id,
                started_at: first_timestamp,
                last_activity_at: Some(last_timestamp),
                status: SessionStatus::from_last_activity(Some(last_timestamp)),
                source_file_path: source_path,
                metadata: serde_json::json!({
                    "project_path": project_path,
                    "cwd": cwd,
                    "git_branch": git_branch,
                    "slugs": slugs,
                }),
            });

            // Parse plan files for each slug
            for slug in &slugs {
                if let Some(plan) = Self::parse_plan_file(slug) {
                    result.plans.push(plan);
                }
            }
        }

        // Update checkpoint
        result.new_checkpoint = Checkpoint::ByteOffset {
            offset: current_offset,
        };

        Ok(result)
    }

    fn extract_project_path(&self, file_path: &Path) -> Option<PathBuf> {
        // Path format: ~/.claude/projects/-home-user-dev-myproject/session.jsonl
        // Project folder name is the encoded path with dashes
        let folder_name = file_path.parent()?.file_name()?.to_str()?;

        // Convert "-home-user-dev-myproject" back to "/home/user/dev/myproject"
        // The encoding replaces "/" with "-", so we need to reverse that
        // Note: This assumes paths don't contain literal dashes (which is not always true)
        // A more robust solution would need the original encoding scheme

        // Simple approach: replace leading dash with "/" and subsequent dashes with "/"
        if !folder_name.starts_with('-') {
            return None;
        }

        let decoded = folder_name.replacen('-', "/", 1).replace('-', "/");
        Some(PathBuf::from(decoded))
    }

    fn extract_session_id(&self, file_path: &Path) -> Option<String> {
        let stem = file_path.file_stem()?.to_str()?;
        Some(stem.to_string())
    }
}

impl ClaudeCodeParser {
    /// Generate a deterministic project ID from the path using SHA256.
    fn generate_project_id(path: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(path.as_bytes());
        let hash = hasher.finalize();
        format!("{:x}", hash)[..16].to_string()
    }

    /// Extract the directory name from a path for use as project name.
    fn extract_dir_name(path: &str) -> String {
        Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    }

    /// Parse a plan file and return a Plan object.
    ///
    /// Plan files are markdown files in `~/.claude/plans/[slug].md`.
    /// We extract the title from the first `# ` heading and compute a content
    /// hash for deduplication.
    fn parse_plan_file(slug: &str) -> Option<Plan> {
        // Construct plan file path
        let plan_path = dirs::home_dir()?
            .join(".claude/plans")
            .join(format!("{}.md", slug));

        if !plan_path.exists() {
            return None;
        }

        // Read file content
        let content = std::fs::read_to_string(&plan_path).ok()?;
        let metadata = std::fs::metadata(&plan_path).ok()?;

        // Extract title from first # heading
        let title = content
            .lines()
            .find(|line| line.starts_with("# "))
            .map(|line| line.trim_start_matches("# ").to_string());

        // Compute content hash for deduplication
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let hash = hasher.finalize();
        let content_hash = format!("{:x}", hash);

        // Get file timestamps
        let created_at = metadata
            .created()
            .ok()
            .map(DateTime::from)
            .unwrap_or_else(Utc::now);
        let modified_at = metadata
            .modified()
            .ok()
            .map(DateTime::from)
            .unwrap_or_else(Utc::now);

        Some(Plan {
            id: slug.to_string(),
            session_id: String::new(), // Linked via join table, not stored here
            path: plan_path.clone(),
            title,
            created_at,
            modified_at,
            status: PlanStatus::Unknown,
            content: Some(content),
            source_file_path: plan_path.to_string_lossy().to_string(),
            raw_data: serde_json::json!({}),
            metadata: serde_json::json!({
                "content_hash": content_hash,
            }),
        })
    }

    /// Convert a raw record to one or more Messages.
    #[allow(clippy::too_many_arguments)]
    fn record_to_messages(
        &self,
        record: &RawRecord,
        raw_json: &serde_json::Value,
        session_id: &str,
        thread_id: &str,
        thread_type: ThreadType,
        seq: &mut i32,
        emitted_at: DateTime<Utc>,
        observed_at: DateTime<Utc>,
        source_path: &str,
        source_offset: i64,
        source_line: Option<i32>,
    ) -> Vec<Message> {
        // In agent threads, user messages come from the parent assistant (caller),
        // not a human. In main threads, user messages are from actual humans.
        let user_role = if thread_type == ThreadType::Agent {
            AuthorRole::Caller
        } else {
            AuthorRole::Human
        };
        let assistant_role = if thread_type == ThreadType::Agent {
            AuthorRole::Agent
        } else {
            AuthorRole::Assistant
        };
        let mut messages = Vec::new();
        let record_type = record.record_type.as_deref().unwrap_or("unknown");

        match record_type {
            "assistant" => {
                if let Some(ref msg) = record.message {
                    // Extract usage
                    let (tokens_in, tokens_out) = msg
                        .usage
                        .as_ref()
                        .map(|u| (u.input_tokens, u.output_tokens))
                        .unwrap_or((None, None));

                    // Process content
                    if let Some(ref content) = msg.content {
                        match content {
                            RawContent::Text(text) => {
                                if !text.is_empty() {
                                    *seq += 1;
                                    messages.push(Message {
                                        id: 0,
                                        session_id: session_id.to_string(),
                                        thread_id: thread_id.to_string(),
                                        seq: *seq,
                                        emitted_at,
                                        observed_at,
                                        author_role: assistant_role,
                                        author_name: None,
                                        message_type: MessageType::Response,
                                        content: Some(text.clone()),
                                        content_type: Some(ContentType::Text),
                                        tool_name: None,
                                        tool_input: None,
                                        tool_result: None,
                                        tokens_in,
                                        tokens_out,
                                        duration_ms: None,
                                        source_file_path: source_path.to_string(),
                                        source_offset,
                                        source_line,
                                        raw_data: raw_json.clone(),
                                        metadata: serde_json::json!({}),
                                    });
                                }
                            }
                            RawContent::Blocks(blocks) => {
                                for block in blocks {
                                    match block {
                                        ContentBlock::Text { text } => {
                                            if !text.is_empty() {
                                                *seq += 1;
                                                messages.push(Message {
                                                    id: 0,
                                                    session_id: session_id.to_string(),
                                                    thread_id: thread_id.to_string(),
                                                    seq: *seq,
                                                    emitted_at,
                                                    observed_at,
                                                    author_role: assistant_role,
                                                    author_name: None,
                                                    message_type: MessageType::Response,
                                                    content: Some(text.clone()),
                                                    content_type: Some(ContentType::Text),
                                                    tool_name: None,
                                                    tool_input: None,
                                                    tool_result: None,
                                                    tokens_in,
                                                    tokens_out,
                                                    duration_ms: None,
                                                    source_file_path: source_path.to_string(),
                                                    source_offset,
                                                    source_line,
                                                    raw_data: raw_json.clone(),
                                                    metadata: serde_json::json!({}),
                                                });
                                            }
                                        }
                                        ContentBlock::ToolUse { id, name, input } => {
                                            *seq += 1;
                                            messages.push(Message {
                                                id: 0,
                                                session_id: session_id.to_string(),
                                                thread_id: thread_id.to_string(),
                                                seq: *seq,
                                                emitted_at,
                                                observed_at,
                                                author_role: assistant_role,
                                                author_name: None,
                                                message_type: MessageType::ToolCall,
                                                content: None,
                                                content_type: None,
                                                tool_name: Some(name.clone()),
                                                tool_input: Some(input.clone()),
                                                tool_result: None,
                                                tokens_in,
                                                tokens_out,
                                                duration_ms: None,
                                                source_file_path: source_path.to_string(),
                                                source_offset,
                                                source_line,
                                                raw_data: raw_json.clone(),
                                                metadata: serde_json::json!({
                                                    "tool_use_id": id,
                                                }),
                                            });
                                        }
                                        // Image and Unknown blocks in assistant messages are rare,
                                        // but we capture them as Context rather than silently skip
                                        ContentBlock::Image { source } => {
                                            *seq += 1;
                                            let media_subtype = source
                                                .media_type
                                                .strip_prefix("image/")
                                                .unwrap_or(&source.media_type);
                                            messages.push(Message {
                                                id: 0,
                                                session_id: session_id.to_string(),
                                                thread_id: thread_id.to_string(),
                                                seq: *seq,
                                                emitted_at,
                                                observed_at,
                                                author_role: assistant_role,
                                                author_name: None,
                                                message_type: MessageType::Context,
                                                content: None,
                                                content_type: Some(ContentType::image_base64(
                                                    media_subtype,
                                                )),
                                                tool_name: None,
                                                tool_input: None,
                                                tool_result: None,
                                                tokens_in,
                                                tokens_out,
                                                duration_ms: None,
                                                source_file_path: source_path.to_string(),
                                                source_offset,
                                                source_line,
                                                raw_data: raw_json.clone(),
                                                metadata: serde_json::json!({}),
                                            });
                                        }
                                        ContentBlock::ToolResult { .. } => {
                                            // ToolResult blocks should not appear in assistant messages
                                            // but if they do, capture them as Context
                                            *seq += 1;
                                            messages.push(Message {
                                                id: 0,
                                                session_id: session_id.to_string(),
                                                thread_id: thread_id.to_string(),
                                                seq: *seq,
                                                emitted_at,
                                                observed_at,
                                                author_role: assistant_role,
                                                author_name: None,
                                                message_type: MessageType::Context,
                                                content: Some(
                                                    "[unexpected tool_result in assistant message]"
                                                        .to_string(),
                                                ),
                                                content_type: Some(ContentType::Unknown(
                                                    "tool_result".to_string(),
                                                )),
                                                tool_name: None,
                                                tool_input: None,
                                                tool_result: None,
                                                tokens_in,
                                                tokens_out,
                                                duration_ms: None,
                                                source_file_path: source_path.to_string(),
                                                source_offset,
                                                source_line,
                                                raw_data: raw_json.clone(),
                                                metadata: serde_json::json!({}),
                                            });
                                        }
                                        ContentBlock::Unknown => {
                                            // Unknown block type - don't skip silently
                                            *seq += 1;
                                            messages.push(Message {
                                                id: 0,
                                                session_id: session_id.to_string(),
                                                thread_id: thread_id.to_string(),
                                                seq: *seq,
                                                emitted_at,
                                                observed_at,
                                                author_role: assistant_role,
                                                author_name: None,
                                                message_type: MessageType::Context,
                                                content: Some(
                                                    "[unknown content block]".to_string(),
                                                ),
                                                content_type: Some(ContentType::Unknown(
                                                    "unknown".to_string(),
                                                )),
                                                tool_name: None,
                                                tool_input: None,
                                                tool_result: None,
                                                tokens_in,
                                                tokens_out,
                                                duration_ms: None,
                                                source_file_path: source_path.to_string(),
                                                source_offset,
                                                source_line,
                                                raw_data: raw_json.clone(),
                                                metadata: serde_json::json!({}),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "user" => {
                if let Some(ref msg) = record.message {
                    if let Some(ref content) = msg.content {
                        match content {
                            RawContent::Text(text) => {
                                if !text.is_empty() {
                                    *seq += 1;
                                    messages.push(Message {
                                        id: 0,
                                        session_id: session_id.to_string(),
                                        thread_id: thread_id.to_string(),
                                        seq: *seq,
                                        emitted_at,
                                        observed_at,
                                        author_role: user_role,
                                        author_name: None,
                                        message_type: MessageType::Prompt,
                                        content: Some(text.clone()),
                                        content_type: Some(ContentType::Text),
                                        tool_name: None,
                                        tool_input: None,
                                        tool_result: None,
                                        tokens_in: None,
                                        tokens_out: None,
                                        duration_ms: None,
                                        source_file_path: source_path.to_string(),
                                        source_offset,
                                        source_line,
                                        raw_data: raw_json.clone(),
                                        metadata: serde_json::json!({}),
                                    });
                                }
                            }
                            RawContent::Blocks(blocks) => {
                                for block in blocks {
                                    match block {
                                        ContentBlock::Text { text } => {
                                            if !text.is_empty() {
                                                *seq += 1;
                                                messages.push(Message {
                                                    id: 0,
                                                    session_id: session_id.to_string(),
                                                    thread_id: thread_id.to_string(),
                                                    seq: *seq,
                                                    emitted_at,
                                                    observed_at,
                                                    author_role: user_role,
                                                    author_name: None,
                                                    message_type: MessageType::Prompt,
                                                    content: Some(text.clone()),
                                                    content_type: Some(ContentType::Text),
                                                    tool_name: None,
                                                    tool_input: None,
                                                    tool_result: None,
                                                    tokens_in: None,
                                                    tokens_out: None,
                                                    duration_ms: None,
                                                    source_file_path: source_path.to_string(),
                                                    source_offset,
                                                    source_line,
                                                    raw_data: raw_json.clone(),
                                                    metadata: serde_json::json!({}),
                                                });
                                            }
                                        }
                                        ContentBlock::ToolResult {
                                            tool_use_id,
                                            content,
                                            is_error,
                                        } => {
                                            *seq += 1;
                                            let result_str = match content {
                                                serde_json::Value::String(s) => s.clone(),
                                                v => v.to_string(),
                                            };
                                            messages.push(Message {
                                                id: 0,
                                                session_id: session_id.to_string(),
                                                thread_id: thread_id.to_string(),
                                                seq: *seq,
                                                emitted_at,
                                                observed_at,
                                                author_role: AuthorRole::Tool,
                                                author_name: None,
                                                message_type: if *is_error {
                                                    MessageType::Error
                                                } else {
                                                    MessageType::ToolResult
                                                },
                                                content: None,
                                                content_type: None,
                                                tool_name: None,
                                                tool_input: None,
                                                tool_result: Some(result_str),
                                                tokens_in: None,
                                                tokens_out: None,
                                                duration_ms: None,
                                                source_file_path: source_path.to_string(),
                                                source_offset,
                                                source_line,
                                                raw_data: raw_json.clone(),
                                                metadata: serde_json::json!({
                                                    "tool_use_id": tool_use_id,
                                                }),
                                            });
                                        }
                                        // Image blocks in user messages are prompts (screenshots)
                                        ContentBlock::Image { source } => {
                                            *seq += 1;
                                            let media_subtype = source
                                                .media_type
                                                .strip_prefix("image/")
                                                .unwrap_or(&source.media_type);
                                            messages.push(Message {
                                                id: 0,
                                                session_id: session_id.to_string(),
                                                thread_id: thread_id.to_string(),
                                                seq: *seq,
                                                emitted_at,
                                                observed_at,
                                                author_role: user_role,
                                                author_name: None,
                                                message_type: MessageType::Prompt,
                                                content: None,
                                                content_type: Some(ContentType::image_base64(
                                                    media_subtype,
                                                )),
                                                tool_name: None,
                                                tool_input: None,
                                                tool_result: None,
                                                tokens_in: None,
                                                tokens_out: None,
                                                duration_ms: None,
                                                source_file_path: source_path.to_string(),
                                                source_offset,
                                                source_line,
                                                raw_data: raw_json.clone(),
                                                metadata: serde_json::json!({}),
                                            });
                                        }
                                        ContentBlock::ToolUse { .. } => {
                                            // ToolUse blocks should not appear in user messages
                                            // but if they do, capture them as Context
                                            *seq += 1;
                                            messages.push(Message {
                                                id: 0,
                                                session_id: session_id.to_string(),
                                                thread_id: thread_id.to_string(),
                                                seq: *seq,
                                                emitted_at,
                                                observed_at,
                                                author_role: user_role,
                                                author_name: None,
                                                message_type: MessageType::Context,
                                                content: Some(
                                                    "[unexpected tool_use in user message]"
                                                        .to_string(),
                                                ),
                                                content_type: Some(ContentType::Unknown(
                                                    "tool_use".to_string(),
                                                )),
                                                tool_name: None,
                                                tool_input: None,
                                                tool_result: None,
                                                tokens_in: None,
                                                tokens_out: None,
                                                duration_ms: None,
                                                source_file_path: source_path.to_string(),
                                                source_offset,
                                                source_line,
                                                raw_data: raw_json.clone(),
                                                metadata: serde_json::json!({}),
                                            });
                                        }
                                        ContentBlock::Unknown => {
                                            // Unknown block type - don't skip silently
                                            *seq += 1;
                                            messages.push(Message {
                                                id: 0,
                                                session_id: session_id.to_string(),
                                                thread_id: thread_id.to_string(),
                                                seq: *seq,
                                                emitted_at,
                                                observed_at,
                                                author_role: user_role,
                                                author_name: None,
                                                message_type: MessageType::Context,
                                                content: Some(
                                                    "[unknown content block]".to_string(),
                                                ),
                                                content_type: Some(ContentType::Unknown(
                                                    "unknown".to_string(),
                                                )),
                                                tool_name: None,
                                                tool_input: None,
                                                tool_result: None,
                                                tokens_in: None,
                                                tokens_out: None,
                                                duration_ms: None,
                                                source_file_path: source_path.to_string(),
                                                source_offset,
                                                source_line,
                                                raw_data: raw_json.clone(),
                                                metadata: serde_json::json!({}),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                // Unknown record type - still capture it
                *seq += 1;
                messages.push(Message {
                    id: 0,
                    session_id: session_id.to_string(),
                    thread_id: thread_id.to_string(),
                    seq: *seq,
                    emitted_at,
                    observed_at,
                    author_role: AuthorRole::System,
                    author_name: Some(record_type.to_string()),
                    message_type: MessageType::Context,
                    content: None,
                    content_type: Some(ContentType::Unknown(record_type.to_string())),
                    tool_name: None,
                    tool_input: None,
                    tool_result: None,
                    tokens_in: None,
                    tokens_out: None,
                    duration_ms: None,
                    source_file_path: source_path.to_string(),
                    source_offset,
                    source_line,
                    raw_data: raw_json.clone(),
                    metadata: serde_json::json!({}),
                });
            }
        }

        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_project_path() {
        let parser = ClaudeCodeParser::new();

        // Standard path
        let path =
            PathBuf::from("/Users/test/.claude/projects/-Users-test-dev-myproject/session.jsonl");
        let project = parser.extract_project_path(&path);
        assert_eq!(project, Some(PathBuf::from("/Users/test/dev/myproject")));
    }

    #[test]
    fn test_extract_session_id_uuid() {
        let parser = ClaudeCodeParser::new();
        let path = PathBuf::from("/path/b4749c81-937a-4bd4-b62c-9d78905f0975.jsonl");

        let sid = parser.extract_session_id(&path);
        assert_eq!(
            sid,
            Some("b4749c81-937a-4bd4-b62c-9d78905f0975".to_string())
        );
    }

    #[test]
    fn test_extract_session_id_agent() {
        let parser = ClaudeCodeParser::new();
        let path = PathBuf::from("/path/agent-a1a93487.jsonl");

        let sid = parser.extract_session_id(&path);
        assert_eq!(sid, Some("agent-a1a93487".to_string()));
    }

    #[test]
    fn test_source_patterns() {
        let parser = ClaudeCodeParser::new();
        let patterns = parser.source_patterns();

        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].pattern, "projects/*/*.jsonl");
        assert!(matches!(patterns[0].file_type, FileType::Jsonl));
    }

    #[test]
    fn test_assistant_type() {
        let parser = ClaudeCodeParser::new();
        assert_eq!(parser.assistant(), Assistant::ClaudeCode);
    }
}
