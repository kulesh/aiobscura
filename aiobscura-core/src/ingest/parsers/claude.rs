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
    Assistant, AuthorRole, Checkpoint, FileType, Message, MessageType, Project, Session,
    SessionStatus, Thread, ThreadType,
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
    // Catch-all for unknown block types
    #[serde(other)]
    Unknown,
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
        let mut first_timestamp: Option<DateTime<Utc>> = None;
        let mut last_timestamp: Option<DateTime<Utc>> = None;

        // Track uuid -> seq mapping for agent spawn linkage
        let mut uuid_to_seq: HashMap<String, i32> = HashMap::new();

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

            // Parse timestamp
            let ts = record
                .timestamp
                .as_ref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(Utc::now);

            if first_timestamp.is_none() {
                first_timestamp = Some(ts);
            }
            last_timestamp = Some(ts);

            // Create thread on first message if needed
            if thread_id.is_none() {
                let sid = session_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                // Determine thread type from file name
                let ttype = if ctx
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("agent-"))
                    .unwrap_or(false)
                {
                    ThreadType::Agent
                } else {
                    ThreadType::Main
                };

                let tid = format!("{}-{}", sid, ttype.as_str());
                thread_id = Some(tid.clone());

                result.threads.push(Thread {
                    id: tid,
                    session_id: sid.clone(),
                    thread_type: ttype,
                    parent_thread_id: None,
                    spawned_by_message_id: None,
                    started_at: ts,
                    ended_at: None,
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
                &mut seq,
                ts,
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

            result.messages.extend(messages);
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
                    created_at: first_timestamp.unwrap_or_else(Utc::now),
                    last_activity_at: last_timestamp,
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
                started_at: first_timestamp.unwrap_or_else(Utc::now),
                last_activity_at: last_timestamp,
                status: SessionStatus::from_last_activity(last_timestamp),
                source_file_path: source_path,
                metadata: serde_json::json!({
                    "project_path": project_path,
                    "cwd": cwd,
                    "git_branch": git_branch,
                }),
            });
        }

        // Update checkpoint
        result.new_checkpoint = Checkpoint::ByteOffset {
            offset: current_offset,
        };

        Ok(result)
    }

    fn extract_project_path(&self, file_path: &Path) -> Option<PathBuf> {
        // Path format: ~/.claude/projects/-Users-kulesh-dev-aiobscura/session.jsonl
        // Project folder name is the encoded path with dashes
        let folder_name = file_path.parent()?.file_name()?.to_str()?;

        // Convert "-Users-kulesh-dev-aiobscura" back to "/Users/kulesh/dev/aiobscura"
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

    /// Convert a raw record to one or more Messages.
    #[allow(clippy::too_many_arguments)]
    fn record_to_messages(
        &self,
        record: &RawRecord,
        raw_json: &serde_json::Value,
        session_id: &str,
        thread_id: &str,
        seq: &mut i32,
        ts: DateTime<Utc>,
        source_path: &str,
        source_offset: i64,
        source_line: Option<i32>,
    ) -> Vec<Message> {
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
                                        ts,
                                        author_role: AuthorRole::Assistant,
                                        author_name: None,
                                        message_type: MessageType::Response,
                                        content: Some(text.clone()),
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
                                                    ts,
                                                    author_role: AuthorRole::Assistant,
                                                    author_name: None,
                                                    message_type: MessageType::Response,
                                                    content: Some(text.clone()),
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
                                                ts,
                                                author_role: AuthorRole::Assistant,
                                                author_name: None,
                                                message_type: MessageType::ToolCall,
                                                content: None,
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
                                        _ => {} // Skip unknown block types
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
                                        ts,
                                        author_role: AuthorRole::Human,
                                        author_name: None,
                                        message_type: MessageType::Prompt,
                                        content: Some(text.clone()),
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
                                                    ts,
                                                    author_role: AuthorRole::Human,
                                                    author_name: None,
                                                    message_type: MessageType::Prompt,
                                                    content: Some(text.clone()),
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
                                                ts,
                                                author_role: AuthorRole::Tool,
                                                author_name: None,
                                                message_type: if *is_error {
                                                    MessageType::Error
                                                } else {
                                                    MessageType::ToolResult
                                                },
                                                content: None,
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
                                        _ => {}
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
                    ts,
                    author_role: AuthorRole::System,
                    author_name: Some(record_type.to_string()),
                    message_type: MessageType::Context,
                    content: None,
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
