//! OpenAI Codex CLI JSONL parser
//!
//! Parses session logs from `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`.
//!
//! See [`docs/codex-log-format.md`](../../../../docs/codex-log-format.md)
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
//! - **Unknown event types**: Converted to [`MessageType::Context`] messages
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
    Assistant, AuthorRole, Checkpoint, ContentType, FileType, Message, MessageType, Project,
    Session, SessionStatus, Thread, ThreadType,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Parser for OpenAI Codex CLI JSONL logs.
pub struct CodexParser {
    root: Option<PathBuf>,
}

impl CodexParser {
    /// Create a new parser with the default root path (~/.codex).
    pub fn new() -> Self {
        Self {
            root: dirs::home_dir().map(|h| h.join(".codex")),
        }
    }

    /// Create a parser with a custom root path (for testing).
    pub fn with_root(root: PathBuf) -> Self {
        Self { root: Some(root) }
    }
}

impl Default for CodexParser {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================
// Helper functions
// ============================================

/// Detect system-injected context patterns in "user" role messages.
///
/// These are messages sent as "user" role but are actually CLI/system context,
/// not actual human input. They should be labeled as "caller" (the system calling
/// the model) rather than "human".
fn is_system_injected_context(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with("<environment_context>")
        || trimmed.starts_with("<user_shell_command>")
        || trimmed.starts_with("<INSTRUCTIONS>")
        || trimmed.starts_with("<user_instructions>")
        || trimmed.starts_with("<system")
        || trimmed.starts_with("# AGENTS.md instructions for")
}

// ============================================
// Raw JSONL record types (serde deserialization)
// ============================================

/// Top-level event container for Codex JSONL records.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawEvent {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    event_type: Option<String>,
    payload: serde_json::Value,
}

/// Session metadata payload (first record in file).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct SessionMetaPayload {
    id: Option<String>,
    cwd: Option<String>,
    originator: Option<String>,
    cli_version: Option<String>,
    instructions: Option<String>,
    source: Option<String>,
    model_provider: Option<String>,
    git: Option<GitInfo>,
}

#[derive(Debug, Deserialize, serde::Serialize, Default, Clone)]
#[serde(default)]
struct GitInfo {
    commit_hash: Option<String>,
    branch: Option<String>,
    repository_url: Option<String>,
}

/// Event message payload subtypes.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct EventMsgPayload {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    message: Option<String>,
    text: Option<String>,
    images: Option<Vec<serde_json::Value>>,
    info: Option<TokenInfo>,
    rate_limits: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TokenInfo {
    total_token_usage: Option<TokenUsage>,
    last_token_usage: Option<TokenUsage>,
    model_context_window: Option<i64>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
struct TokenUsage {
    input_tokens: Option<i32>,
    cached_input_tokens: Option<i32>,
    output_tokens: Option<i32>,
    reasoning_output_tokens: Option<i32>,
    total_tokens: Option<i32>,
}

/// Response item payload subtypes.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ResponseItemPayload {
    #[serde(rename = "type")]
    item_type: Option<String>,
    role: Option<String>,
    content: Option<Vec<ContentBlock>>,
    name: Option<String>,
    arguments: Option<String>,
    call_id: Option<String>,
    output: Option<String>,
    ghost_commit: Option<serde_json::Value>,
    summary: Option<Vec<serde_json::Value>>,
    encrypted_content: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "input_text")]
    InputText { text: String },
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Unknown,
}

/// Turn context payload.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TurnContextPayload {
    cwd: Option<String>,
    approval_policy: Option<String>,
    sandbox_policy: Option<serde_json::Value>,
    model: Option<String>,
    summary: Option<String>,
}

impl AssistantParser for CodexParser {
    fn assistant(&self) -> Assistant {
        Assistant::Codex
    }

    fn root_path(&self) -> Option<PathBuf> {
        self.root.clone()
    }

    fn source_patterns(&self) -> Vec<SourcePattern> {
        vec![SourcePattern {
            pattern: "sessions/*/*/*/rollout-*.jsonl".to_string(),
            file_type: FileType::Jsonl,
            description: "Codex CLI session logs".to_string(),
        }]
    }

    fn parse(&self, ctx: &ParseContext) -> Result<ParseResult> {
        let mut result = ParseResult::default();

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
        let mut line_number = 0;
        let mut seq = 0i32;

        // Session state - extract from path upfront for incremental parsing
        // (session_meta event may be skipped when resuming from checkpoint)
        let mut session_id: Option<String> = self.extract_session_id(ctx.path);
        let mut thread_id: Option<String> = session_id.as_ref().map(|sid| format!("{}-main", sid));
        let mut model_id: Option<String> = None;
        let mut cwd: Option<String> = None;
        let mut git_info: Option<GitInfo> = None;
        let mut first_timestamp: Option<DateTime<Utc>> = None;
        let mut last_timestamp: Option<DateTime<Utc>> = None;
        let mut last_token_usage: Option<TokenUsage> = None;

        // Track if we've seen the first user prompt (the CLI invocation).
        // The first user message that isn't system-injected context is the
        // CLI-provided prompt, which should be labeled as "caller" not "human".
        // On incremental parse (start_offset > 0), we've already processed the
        // initial prompt, so any new user messages are follow-up human input.
        let mut seen_first_user_prompt = start_offset > 0;

        // Track call_id -> message seq for tool result linking
        let mut call_id_to_seq: HashMap<String, i32> = HashMap::new();

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

            // Deserialize into event structure
            let event: RawEvent = match serde_json::from_value(raw_json.clone()) {
                Ok(e) => e,
                Err(e) => {
                    result.warnings.push(format!(
                        "Line {} (offset {}): deserialization error: {}",
                        line_number, record_offset, e
                    ));
                    continue;
                }
            };

            // Parse timestamp
            let ts = event
                .timestamp
                .as_ref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(Utc::now);

            if first_timestamp.is_none() {
                first_timestamp = Some(ts);
            }
            last_timestamp = Some(ts);

            let event_type = event.event_type.as_deref().unwrap_or("unknown");

            match event_type {
                "session_meta" => {
                    let payload: SessionMetaPayload =
                        serde_json::from_value(event.payload.clone()).unwrap_or_default();

                    // Extract session ID - payload.id takes precedence over filename
                    if let Some(id) = payload.id.clone() {
                        session_id = Some(id.clone());
                        // Update thread_id to match new session_id
                        thread_id = Some(format!("{}-main", id));
                    }

                    // Extract metadata
                    if cwd.is_none() {
                        cwd = payload.cwd.clone();
                    }
                    if git_info.is_none() {
                        git_info = payload.git.clone();
                    }

                    // Create thread on first session_meta (only in initial parse)
                    if result.threads.is_empty() {
                        let sid = session_id
                            .clone()
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                        let tid = thread_id.clone().unwrap_or_else(|| format!("{}-main", sid));

                        result.threads.push(Thread {
                            id: tid,
                            session_id: sid,
                            thread_type: ThreadType::Main,
                            parent_thread_id: None,
                            spawned_by_message_id: None,
                            started_at: ts,
                            ended_at: None,
                            metadata: serde_json::json!({}),
                        });
                    }
                }

                "event_msg" => {
                    let payload: EventMsgPayload =
                        serde_json::from_value(event.payload.clone()).unwrap_or_default();

                    let msg_type = payload.msg_type.as_deref().unwrap_or("unknown");

                    match msg_type {
                        // Skip these - they duplicate response_item events:
                        // - user_message duplicates response_item.message (role=user)
                        // - agent_message duplicates response_item.message (role=assistant)
                        // - agent_reasoning duplicates response_item.reasoning.summary
                        "user_message" | "agent_message" | "agent_reasoning" => {
                            // Intentionally skipped - content captured via response_item
                        }

                        "token_count" => {
                            // Store latest token usage for subsequent messages
                            if let Some(ref info) = payload.info {
                                last_token_usage = info.last_token_usage.clone();
                            }
                        }

                        _ => {
                            // Unknown event_msg type - capture as context for future-proofing
                            seq += 1;
                            result.messages.push(Message {
                                id: 0,
                                session_id: session_id.clone().unwrap_or_default(),
                                thread_id: thread_id.clone().unwrap_or_default(),
                                seq,
                                ts,
                                author_role: AuthorRole::System,
                                author_name: Some(msg_type.to_string()),
                                message_type: MessageType::Context,
                                content: None,
                                content_type: Some(ContentType::Unknown(msg_type.to_string())),
                                tool_name: None,
                                tool_input: None,
                                tool_result: None,
                                tokens_in: None,
                                tokens_out: None,
                                duration_ms: None,
                                source_file_path: source_path.clone(),
                                source_offset: record_offset as i64,
                                source_line: Some(line_number),
                                raw_data: raw_json.clone(),
                                metadata: serde_json::json!({}),
                            });
                        }
                    }
                }

                "response_item" => {
                    let payload: ResponseItemPayload =
                        serde_json::from_value(event.payload.clone()).unwrap_or_default();

                    let item_type = payload.item_type.as_deref().unwrap_or("unknown");

                    match item_type {
                        "message" => {
                            let role = payload.role.as_deref().unwrap_or("unknown");

                            // Extract text from content blocks
                            if let Some(blocks) = &payload.content {
                                for block in blocks {
                                    let text = match block {
                                        ContentBlock::InputText { text } => Some(text.clone()),
                                        ContentBlock::OutputText { text } => Some(text.clone()),
                                        ContentBlock::Text { text } => Some(text.clone()),
                                        ContentBlock::Unknown => None,
                                    };

                                    if let Some(text) = text {
                                        if !text.is_empty() {
                                            // Detect system-injected context patterns (not actual human input)
                                            // These are messages sent as "user" role but are CLI/system context
                                            let is_system_context =
                                                role == "user" && is_system_injected_context(&text);

                                            // Determine author role and message type based on role and content
                                            // The first non-system user message is the CLI invocation (caller),
                                            // subsequent user messages are actual human input.
                                            let (author_role, message_type) = match role {
                                                "assistant" => {
                                                    (AuthorRole::Assistant, MessageType::Response)
                                                }
                                                "user" if is_system_context => {
                                                    // System/CLI injected context (environment, instructions)
                                                    (AuthorRole::Caller, MessageType::Context)
                                                }
                                                "user" if !seen_first_user_prompt => {
                                                    // First user prompt is the CLI invocation
                                                    seen_first_user_prompt = true;
                                                    (AuthorRole::Caller, MessageType::Prompt)
                                                }
                                                "user" => (AuthorRole::Human, MessageType::Prompt),
                                                _ => (AuthorRole::System, MessageType::Context),
                                            };

                                            seq += 1;

                                            // Apply token counts from last token_count event
                                            let (tokens_in, tokens_out) =
                                                if author_role == AuthorRole::Assistant {
                                                    last_token_usage
                                                        .as_ref()
                                                        .map(|u| (u.input_tokens, u.output_tokens))
                                                        .unwrap_or((None, None))
                                                } else {
                                                    (None, None)
                                                };

                                            result.messages.push(Message {
                                                id: 0,
                                                session_id: session_id.clone().unwrap_or_default(),
                                                thread_id: thread_id.clone().unwrap_or_default(),
                                                seq,
                                                ts,
                                                author_role,
                                                author_name: None,
                                                message_type,
                                                content: Some(text),
                                                content_type: Some(ContentType::Text),
                                                tool_name: None,
                                                tool_input: None,
                                                tool_result: None,
                                                tokens_in,
                                                tokens_out,
                                                duration_ms: None,
                                                source_file_path: source_path.clone(),
                                                source_offset: record_offset as i64,
                                                source_line: Some(line_number),
                                                raw_data: raw_json.clone(),
                                                metadata: serde_json::json!({}),
                                            });
                                        }
                                    }
                                }
                            }
                        }

                        "function_call" => {
                            seq += 1;

                            // Parse arguments JSON string
                            let tool_input = payload
                                .arguments
                                .as_ref()
                                .and_then(|s| serde_json::from_str(s).ok());

                            // Track call_id for linking to output
                            if let Some(ref call_id) = payload.call_id {
                                call_id_to_seq.insert(call_id.clone(), seq);
                            }

                            result.messages.push(Message {
                                id: 0,
                                session_id: session_id.clone().unwrap_or_default(),
                                thread_id: thread_id.clone().unwrap_or_default(),
                                seq,
                                ts,
                                author_role: AuthorRole::Assistant,
                                author_name: None,
                                message_type: MessageType::ToolCall,
                                content: None,
                                content_type: None,
                                tool_name: payload.name.clone(),
                                tool_input,
                                tool_result: None,
                                tokens_in: None,
                                tokens_out: None,
                                duration_ms: None,
                                source_file_path: source_path.clone(),
                                source_offset: record_offset as i64,
                                source_line: Some(line_number),
                                raw_data: raw_json.clone(),
                                metadata: serde_json::json!({
                                    "call_id": payload.call_id,
                                }),
                            });
                        }

                        "function_call_output" => {
                            seq += 1;

                            result.messages.push(Message {
                                id: 0,
                                session_id: session_id.clone().unwrap_or_default(),
                                thread_id: thread_id.clone().unwrap_or_default(),
                                seq,
                                ts,
                                author_role: AuthorRole::Tool,
                                author_name: None,
                                message_type: MessageType::ToolResult,
                                content: None,
                                content_type: None,
                                tool_name: None,
                                tool_input: None,
                                tool_result: payload.output.clone(),
                                tokens_in: None,
                                tokens_out: None,
                                duration_ms: None,
                                source_file_path: source_path.clone(),
                                source_offset: record_offset as i64,
                                source_line: Some(line_number),
                                raw_data: raw_json.clone(),
                                metadata: serde_json::json!({
                                    "call_id": payload.call_id,
                                }),
                            });
                        }

                        "reasoning" => {
                            // Model reasoning (may be encrypted)
                            seq += 1;

                            // Extract summary text if available
                            let summary_text = payload
                                .summary
                                .as_ref()
                                .and_then(|s| s.first())
                                .and_then(|v| v.get("text"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            // Combine summary with encrypted indicator when both are present
                            let content = match (&summary_text, &payload.encrypted_content) {
                                (Some(summary), Some(_)) => {
                                    Some(format!("{}\n[encrypted reasoning]", summary))
                                }
                                (Some(summary), None) => Some(summary.clone()),
                                (None, Some(_)) => Some("[encrypted reasoning]".to_string()),
                                (None, None) => None,
                            };

                            result.messages.push(Message {
                                id: 0,
                                session_id: session_id.clone().unwrap_or_default(),
                                thread_id: thread_id.clone().unwrap_or_default(),
                                seq,
                                ts,
                                author_role: AuthorRole::Assistant,
                                author_name: None,
                                message_type: MessageType::Context,
                                content,
                                content_type: Some(ContentType::Text),
                                tool_name: None,
                                tool_input: None,
                                tool_result: None,
                                tokens_in: None,
                                tokens_out: None,
                                duration_ms: None,
                                source_file_path: source_path.clone(),
                                source_offset: record_offset as i64,
                                source_line: Some(line_number),
                                raw_data: raw_json.clone(),
                                metadata: serde_json::json!({
                                    "reasoning": true,
                                    "encrypted": payload.encrypted_content.is_some(),
                                }),
                            });
                        }

                        "ghost_snapshot" => {
                            // Git state snapshot - capture as context with commit hash
                            seq += 1;

                            // Extract short commit hash for display
                            let commit_preview = payload
                                .ghost_commit
                                .as_ref()
                                .and_then(|gc| gc.get("id"))
                                .and_then(|v| v.as_str())
                                .map(|s| if s.len() > 8 { &s[..8] } else { s })
                                .unwrap_or("unknown");

                            result.messages.push(Message {
                                id: 0,
                                session_id: session_id.clone().unwrap_or_default(),
                                thread_id: thread_id.clone().unwrap_or_default(),
                                seq,
                                ts,
                                author_role: AuthorRole::System,
                                author_name: Some("snapshot".to_string()),
                                message_type: MessageType::Context,
                                content: Some(format!("git checkpoint: {}", commit_preview)),
                                content_type: Some(ContentType::Text),
                                tool_name: None,
                                tool_input: None,
                                tool_result: None,
                                tokens_in: None,
                                tokens_out: None,
                                duration_ms: None,
                                source_file_path: source_path.clone(),
                                source_offset: record_offset as i64,
                                source_line: Some(line_number),
                                raw_data: raw_json.clone(),
                                metadata: serde_json::json!({
                                    "git_snapshot": payload.ghost_commit,
                                }),
                            });
                        }

                        "custom_tool_call" => {
                            // Custom tool calls (e.g., apply_patch) - handle like function_call
                            seq += 1;

                            // Track call_id for linking to output
                            if let Some(ref call_id) = payload.call_id {
                                call_id_to_seq.insert(call_id.clone(), seq);
                            }

                            result.messages.push(Message {
                                id: 0,
                                session_id: session_id.clone().unwrap_or_default(),
                                thread_id: thread_id.clone().unwrap_or_default(),
                                seq,
                                ts,
                                author_role: AuthorRole::Assistant,
                                author_name: None,
                                message_type: MessageType::ToolCall,
                                content: None,
                                content_type: None,
                                tool_name: payload.name.clone(),
                                tool_input: Some(serde_json::json!({
                                    "input": event.payload.get("input"),
                                })),
                                tool_result: None,
                                tokens_in: None,
                                tokens_out: None,
                                duration_ms: None,
                                source_file_path: source_path.clone(),
                                source_offset: record_offset as i64,
                                source_line: Some(line_number),
                                raw_data: raw_json.clone(),
                                metadata: serde_json::json!({
                                    "call_id": payload.call_id,
                                    "custom_tool": true,
                                }),
                            });
                        }

                        "custom_tool_call_output" => {
                            // Custom tool output - handle like function_call_output
                            seq += 1;

                            result.messages.push(Message {
                                id: 0,
                                session_id: session_id.clone().unwrap_or_default(),
                                thread_id: thread_id.clone().unwrap_or_default(),
                                seq,
                                ts,
                                author_role: AuthorRole::Tool,
                                author_name: None,
                                message_type: MessageType::ToolResult,
                                content: None,
                                content_type: None,
                                tool_name: None,
                                tool_input: None,
                                tool_result: payload.output.clone(),
                                tokens_in: None,
                                tokens_out: None,
                                duration_ms: None,
                                source_file_path: source_path.clone(),
                                source_offset: record_offset as i64,
                                source_line: Some(line_number),
                                raw_data: raw_json.clone(),
                                metadata: serde_json::json!({
                                    "call_id": payload.call_id,
                                    "custom_tool": true,
                                }),
                            });
                        }

                        _ => {
                            // Unknown response_item type - capture as context
                            seq += 1;
                            result.messages.push(Message {
                                id: 0,
                                session_id: session_id.clone().unwrap_or_default(),
                                thread_id: thread_id.clone().unwrap_or_default(),
                                seq,
                                ts,
                                author_role: AuthorRole::System,
                                author_name: Some(item_type.to_string()),
                                message_type: MessageType::Context,
                                content: None,
                                content_type: Some(ContentType::Unknown(item_type.to_string())),
                                tool_name: None,
                                tool_input: None,
                                tool_result: None,
                                tokens_in: None,
                                tokens_out: None,
                                duration_ms: None,
                                source_file_path: source_path.clone(),
                                source_offset: record_offset as i64,
                                source_line: Some(line_number),
                                raw_data: raw_json.clone(),
                                metadata: serde_json::json!({}),
                            });
                        }
                    }
                }

                "turn_context" => {
                    let payload: TurnContextPayload =
                        serde_json::from_value(event.payload.clone()).unwrap_or_default();

                    // Extract model ID from turn_context
                    if model_id.is_none() {
                        model_id = payload.model.clone();
                    }

                    // Update cwd if changed
                    if let Some(new_cwd) = payload.cwd {
                        cwd = Some(new_cwd);
                    }
                }

                _ => {
                    // Unknown event type - capture as context
                    seq += 1;
                    result.messages.push(Message {
                        id: 0,
                        session_id: session_id.clone().unwrap_or_default(),
                        thread_id: thread_id.clone().unwrap_or_default(),
                        seq,
                        ts,
                        author_role: AuthorRole::System,
                        author_name: Some(event_type.to_string()),
                        message_type: MessageType::Context,
                        content: None,
                        content_type: Some(ContentType::Unknown(event_type.to_string())),
                        tool_name: None,
                        tool_input: None,
                        tool_result: None,
                        tokens_in: None,
                        tokens_out: None,
                        duration_ms: None,
                        source_file_path: source_path.clone(),
                        source_offset: record_offset as i64,
                        source_line: Some(line_number),
                        raw_data: raw_json.clone(),
                        metadata: serde_json::json!({}),
                    });
                }
            }
        }

        // Create session and project
        if let Some(sid) = session_id {
            // Create Project from cwd
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
                assistant: Assistant::Codex,
                backing_model_id: model_id.map(|m| format!("openai:{}", m)),
                project_id,
                started_at: first_timestamp.unwrap_or_else(Utc::now),
                last_activity_at: last_timestamp,
                status: SessionStatus::from_last_activity(last_timestamp),
                source_file_path: source_path,
                metadata: serde_json::json!({
                    "cwd": cwd,
                    "git": git_info,
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
        // Codex doesn't encode project path in the file path like Claude does.
        // Project path comes from session_meta.cwd instead.
        // Return None here - we'll get it from the session metadata.
        let _ = file_path;
        None
    }

    fn extract_session_id(&self, file_path: &Path) -> Option<String> {
        // Filename format: rollout-2025-11-24T19-33-35-019ab86e-1e83-75b0-b2d7-d335492e7026.jsonl
        // Session ID is the ULID portion after the timestamp
        let stem = file_path.file_stem()?.to_str()?;

        // Find the ULID pattern (8-4-4-4-12 hex)
        // The stem looks like: rollout-2025-11-24T19-33-35-019ab86e-1e83-75b0-b2d7-d335492e7026
        // We want: 019ab86e-1e83-75b0-b2d7-d335492e7026

        // Split on '-' and find where the ULID starts
        let parts: Vec<&str> = stem.split('-').collect();
        if parts.len() >= 5 {
            // ULID starts after the timestamp portion
            // Format: rollout-YYYY-MM-DDThh-mm-ss-ULID
            // ULID is 5 parts: XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX
            // Find the part that looks like start of ULID (8 hex chars after timestamp)
            for i in 0..parts.len().saturating_sub(4) {
                let candidate = &parts[i..];
                if candidate.len() >= 5 {
                    // Check if this looks like a ULID (first part is 8 chars)
                    if candidate[0].len() == 8
                        && candidate[0].chars().all(|c| c.is_ascii_hexdigit())
                    {
                        // Reconstruct the ULID
                        let ulid = candidate[..5].join("-");
                        return Some(ulid);
                    }
                }
            }
        }

        // Fallback: return the full stem
        Some(stem.to_string())
    }
}

impl CodexParser {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_session_id() {
        let parser = CodexParser::new();

        let path = PathBuf::from(
            "/Users/test/.codex/sessions/2025/11/24/rollout-2025-11-24T19-33-35-019ab86e-1e83-75b0-b2d7-d335492e7026.jsonl",
        );
        let session_id = parser.extract_session_id(&path);
        assert_eq!(
            session_id,
            Some("019ab86e-1e83-75b0-b2d7-d335492e7026".to_string())
        );
    }

    #[test]
    fn test_source_patterns() {
        let parser = CodexParser::new();
        let patterns = parser.source_patterns();

        assert_eq!(patterns.len(), 1);
        assert!(patterns[0].pattern.contains("rollout-*.jsonl"));
    }

    #[test]
    fn test_assistant_type() {
        let parser = CodexParser::new();
        assert_eq!(parser.assistant(), Assistant::Codex);
    }

    #[test]
    fn test_root_path() {
        let parser = CodexParser::new();
        let root = parser.root_path();
        assert!(root.is_some());
        assert!(root.unwrap().ends_with(".codex"));
    }

    #[test]
    fn test_with_root() {
        let custom_root = PathBuf::from("/custom/path");
        let parser = CodexParser::with_root(custom_root.clone());
        assert_eq!(parser.root_path(), Some(custom_root));
    }

    #[test]
    fn test_generate_project_id() {
        let id1 = CodexParser::generate_project_id("/Users/test/dev/project");
        let id2 = CodexParser::generate_project_id("/Users/test/dev/project");
        let id3 = CodexParser::generate_project_id("/Users/test/dev/other");

        // Same path should produce same ID
        assert_eq!(id1, id2);
        // Different path should produce different ID
        assert_ne!(id1, id3);
        // ID should be 16 hex characters
        assert_eq!(id1.len(), 16);
    }
}
