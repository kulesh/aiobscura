//! Core domain types for aiobscura
//!
//! These types represent the canonical data model (Layer 1) that normalizes
//! activity from all supported AI coding assistants.
//!
//! ## Terminology
//!
//! | Term | Definition |
//! |------|------------|
//! | **Project** | A codebase/directory that multiple Sessions and Assistants can work on |
//! | **Assistant** | A coding assistant product (Claude Code, Codex, Aider, Cursor) |
//! | **BackingModel** | The LLM powering an assistant (opus-4.5, gpt-5, sonnet-4) |
//! | **Session** | A period of activity by an Assistant on a Project |
//! | **Thread** | A conversation flow within a Session; main thread is implicit, agents spawn sub-threads |
//! | **Agent** | A subprocess spawned by an Assistant to do work; never interacts directly with Human |
//! | **Human** | Always a real person (see note below) |
//! | **User** | Ambiguous term we avoid in our types (see note below) |
//! | **Tool** | An executable capability (Bash, Read, Edit, etc.) |
//! | **Plan** | A plan file associated with a Session (tracked separately) |
//!
//! ### Human vs User
//!
//! "User" is ambiguous because it depends on perspective:
//! - From an **Agent's** view: its "user" is the Assistant that spawned it
//! - From an **Assistant's** view: its "user" is the Human
//!
//! To avoid confusion, aiobscura types use precise terms:
//! - [`AuthorRole::Human`] - Always a real person
//! - [`AuthorRole::Assistant`] - The coding assistant product
//! - [`AuthorRole::Agent`] - A subprocess spawned by an Assistant
//!
//! We never use "User" as a type name. When parsing logs that contain "user" roles,
//! we map them to the appropriate [`AuthorRole`] based on context.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ============================================
// Project
// ============================================

/// A codebase that assistants work on.
///
/// Multiple Assistants (Claude Code, Codex, etc.) can work on the same Project
/// simultaneously or at different times. This enables cross-assistant analytics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Unique identifier (generated or derived from path)
    pub id: String,
    /// Canonical path to project root
    pub path: PathBuf,
    /// Human-friendly name (optional)
    pub name: Option<String>,
    /// When this project was first seen
    pub created_at: DateTime<Utc>,
    /// Most recent activity timestamp
    pub last_activity_at: Option<DateTime<Utc>>,
    /// Extensible metadata
    pub metadata: serde_json::Value,
}

// ============================================
// Source Files
// ============================================

/// Type of source file being parsed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    /// Append-only log (Claude Code, Codex)
    Jsonl,
    /// Rewritten each time
    Json,
    /// Plan files
    Markdown,
    /// Cursor database
    Sqlite,
}

impl FileType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileType::Jsonl => "jsonl",
            FileType::Json => "json",
            FileType::Markdown => "markdown",
            FileType::Sqlite => "sqlite",
        }
    }
}

impl std::str::FromStr for FileType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "jsonl" => Ok(FileType::Jsonl),
            "json" => Ok(FileType::Json),
            "markdown" => Ok(FileType::Markdown),
            "sqlite" => Ok(FileType::Sqlite),
            _ => Err(format!("unknown file type: {}", s)),
        }
    }
}

/// Checkpoint strategy depends on file type
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Checkpoint {
    /// For append-only files (JSONL): track byte offset
    ByteOffset { offset: u64 },

    /// For rewritable files (JSON, Markdown): track content hash
    ContentHash { hash: String },

    /// For databases (SQLite): track max rowid or timestamp
    DatabaseCursor {
        table: String,
        cursor_column: String,
        cursor_value: String,
    },

    /// Not yet parsed
    #[default]
    None,
}

/// A source file from Layer 0 (raw log file)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    /// Path to the source file (primary key)
    pub path: PathBuf,
    /// Type of file
    pub file_type: FileType,
    /// Which assistant this file is from
    pub assistant: Assistant,
    /// When the file was created
    pub created_at: DateTime<Utc>,
    /// When the file was last modified
    pub modified_at: DateTime<Utc>,
    /// File size in bytes
    pub size_bytes: u64,
    /// When this file was last parsed
    pub last_parsed_at: Option<DateTime<Utc>>,
    /// Checkpoint for incremental parsing (type-specific)
    pub checkpoint: Checkpoint,
}

// ============================================
// Assistant Types
// ============================================

/// Supported AI coding assistants (products, not agents)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Assistant {
    ClaudeCode,
    Codex,
    Aider,
    Cursor,
}

impl Assistant {
    /// Returns the display name for this assistant
    pub fn display_name(&self) -> &'static str {
        match self {
            Assistant::ClaudeCode => "Claude Code",
            Assistant::Codex => "Codex",
            Assistant::Aider => "Aider",
            Assistant::Cursor => "Cursor",
        }
    }

    /// Returns the identifier used in database storage
    pub fn as_str(&self) -> &'static str {
        match self {
            Assistant::ClaudeCode => "claude_code",
            Assistant::Codex => "codex",
            Assistant::Aider => "aider",
            Assistant::Cursor => "cursor",
        }
    }

    /// Returns the default path where this assistant stores logs
    pub fn default_log_path(&self) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        Some(match self {
            Assistant::ClaudeCode => home.join(".claude"),
            Assistant::Codex => home.join(".codex"),
            Assistant::Aider => home.join(".aider"),
            Assistant::Cursor => home.join(".cursor"),
        })
    }
}

impl std::fmt::Display for Assistant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Assistant {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude_code" | "ClaudeCode" => Ok(Assistant::ClaudeCode),
            "codex" | "Codex" => Ok(Assistant::Codex),
            "aider" | "Aider" => Ok(Assistant::Aider),
            "cursor" | "Cursor" => Ok(Assistant::Cursor),
            _ => Err(format!("unknown assistant: {}", s)),
        }
    }
}

// ============================================
// Backing Model
// ============================================

/// The LLM powering an assistant.
///
/// BackingModel is a first-class entity stored in its own table.
/// This allows future enrichment with external data (capabilities, pricing, benchmarks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackingModel {
    /// Unique identifier, e.g., "anthropic:claude-opus-4-5"
    pub id: String,
    /// Provider: "anthropic", "openai", "ollama", "google"
    pub provider: String,
    /// Provider's model ID: "claude-opus-4-5-20251101", "gpt-5", etc.
    pub model_id: String,
    /// Human-friendly name: "Claude Opus 4.5"
    pub display_name: Option<String>,
    /// When this model was first seen
    pub first_seen_at: DateTime<Utc>,
    /// Extensible metadata: context window, pricing, etc.
    pub metadata: serde_json::Value,
}

impl BackingModel {
    /// Create a canonical ID from provider and model_id
    pub fn canonical_id(provider: &str, model_id: &str) -> String {
        format!("{}:{}", provider, model_id)
    }

    /// Create a new BackingModel with canonical ID
    pub fn new(provider: String, model_id: String) -> Self {
        let id = Self::canonical_id(&provider, &model_id);
        Self {
            id,
            provider,
            model_id,
            display_name: None,
            first_seen_at: Utc::now(),
            metadata: serde_json::json!({}),
        }
    }

    /// Create a BackingModel from an ID string like "anthropic:claude-sonnet-4-20250514"
    pub fn from_id(id: &str) -> Self {
        let (provider, model_id) = id.split_once(':').unwrap_or(("unknown", id));
        Self {
            id: id.to_string(),
            provider: provider.to_string(),
            model_id: model_id.to_string(),
            display_name: None,
            first_seen_at: Utc::now(),
            metadata: serde_json::json!({}),
        }
    }
}

// ============================================
// Sessions
// ============================================

/// Current status of a session based on activity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Activity within last 5 minutes
    Active,
    /// 5-60 minutes since last activity
    Inactive,
    /// More than 60 minutes since last activity
    Stale,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Active => "active",
            SessionStatus::Inactive => "inactive",
            SessionStatus::Stale => "stale",
        }
    }

    /// Compute status from last activity time
    pub fn from_last_activity(last_activity: Option<DateTime<Utc>>) -> Self {
        let Some(last) = last_activity else {
            return SessionStatus::Stale;
        };

        let elapsed = Utc::now().signed_duration_since(last);
        let minutes = elapsed.num_minutes();

        if minutes < 5 {
            SessionStatus::Active
        } else if minutes < 60 {
            SessionStatus::Inactive
        } else {
            SessionStatus::Stale
        }
    }
}

impl std::str::FromStr for SessionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(SessionStatus::Active),
            "inactive" => Ok(SessionStatus::Inactive),
            "stale" => Ok(SessionStatus::Stale),
            _ => Err(format!("unknown session status: {}", s)),
        }
    }
}

/// A session represents a period of activity with an AI coding assistant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique identifier for this session
    pub id: String,
    /// Which assistant this session is from
    pub assistant: Assistant,
    /// FK to backing_models table (if known)
    pub backing_model_id: Option<String>,
    /// FK to projects table
    pub project_id: Option<String>,
    /// When the session started
    pub started_at: DateTime<Utc>,
    /// Most recent activity timestamp
    pub last_activity_at: Option<DateTime<Utc>>,
    /// Current status (computed from last_activity_at)
    pub status: SessionStatus,

    // Lineage - reference to source file
    /// FK to source_files table (path is PK)
    pub source_file_path: String,

    /// Parsed assistant-specific fields (cwd, git_branch, etc.)
    pub metadata: serde_json::Value,
}

impl Session {
    /// Update status based on current time
    pub fn refresh_status(&mut self) {
        self.status = SessionStatus::from_last_activity(self.last_activity_at);
    }
}

// ============================================
// Threads
// ============================================

/// Type of thread within a session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadType {
    /// Implicit main conversation thread
    Main,
    /// Spawned by Task tool (explore, plan, etc.)
    Agent,
    /// Background operations (summarization, backup)
    Background,
}

impl ThreadType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThreadType::Main => "main",
            ThreadType::Agent => "agent",
            ThreadType::Background => "background",
        }
    }
}

impl std::str::FromStr for ThreadType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "main" => Ok(ThreadType::Main),
            "agent" => Ok(ThreadType::Agent),
            "background" => Ok(ThreadType::Background),
            _ => Err(format!("unknown thread type: {}", s)),
        }
    }
}

/// A Thread represents a conversation flow within a Session.
///
/// - The main conversation is an implicit "main" thread
/// - Agents spawn sub-threads (e.g., Task agents, background summarizers)
/// - Threads have provenance: which thread spawned them
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    /// Unique identifier
    pub id: String,
    /// Session this thread belongs to
    pub session_id: String,
    /// Type of thread
    pub thread_type: ThreadType,
    /// Parent thread ID (null for main thread)
    pub parent_thread_id: Option<String>,
    /// Message that triggered this thread
    pub spawned_by_message_id: Option<i64>,
    /// When the thread started
    pub started_at: DateTime<Utc>,
    /// When the thread ended (if known)
    pub ended_at: Option<DateTime<Utc>>,
    /// Last message timestamp for this thread (excluding context messages)
    pub last_activity_at: Option<DateTime<Utc>>,
    /// Extensible metadata
    pub metadata: serde_json::Value,
}

impl Thread {
    /// Returns the agent subtype if recorded in metadata.
    pub fn agent_subtype(&self) -> Option<&str> {
        self.metadata.get("agent_subtype").and_then(|v| v.as_str())
    }
}

// ============================================
// Authors
// ============================================

/// Role of the message author
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorRole {
    /// Real person interacting in main conversation
    Human,
    /// CLI or parent assistant invoking a session/agent
    Caller,
    /// The coding assistant (Claude Code, Codex, etc.)
    Assistant,
    /// Subprocess spawned by assistant (Task agent, etc.)
    /// Note: Currently unused by parsers; reserved for unified timeline views
    Agent,
    /// Tool execution (Bash, Read, Edit)
    Tool,
    /// System-level events (snapshots, internal context)
    System,
}

impl AuthorRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthorRole::Human => "human",
            AuthorRole::Caller => "caller",
            AuthorRole::Assistant => "assistant",
            AuthorRole::Agent => "agent",
            AuthorRole::Tool => "tool",
            AuthorRole::System => "system",
        }
    }
}

impl std::str::FromStr for AuthorRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "human" => Ok(AuthorRole::Human),
            "caller" => Ok(AuthorRole::Caller),
            "assistant" => Ok(AuthorRole::Assistant),
            "agent" => Ok(AuthorRole::Agent),
            "tool" => Ok(AuthorRole::Tool),
            "system" => Ok(AuthorRole::System),
            _ => Err(format!("unknown author role: {}", s)),
        }
    }
}

/// Who authored a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    /// Role of the author
    pub role: AuthorRole,
    /// Name (for tools: "Read", "Bash"; for agents: agent_id)
    pub name: Option<String>,
}

impl Author {
    pub fn human() -> Self {
        Self {
            role: AuthorRole::Human,
            name: None,
        }
    }

    pub fn caller() -> Self {
        Self {
            role: AuthorRole::Caller,
            name: None,
        }
    }

    pub fn assistant() -> Self {
        Self {
            role: AuthorRole::Assistant,
            name: None,
        }
    }

    pub fn agent(name: impl Into<String>) -> Self {
        Self {
            role: AuthorRole::Agent,
            name: Some(name.into()),
        }
    }

    pub fn tool(name: impl Into<String>) -> Self {
        Self {
            role: AuthorRole::Tool,
            name: Some(name.into()),
        }
    }

    pub fn system() -> Self {
        Self {
            role: AuthorRole::System,
            name: None,
        }
    }
}

// ============================================
// Messages (formerly Events)
// ============================================

/// Type of message within a session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Request/instruction to assistant
    Prompt,
    /// Reply from assistant
    Response,
    /// Request to invoke a tool
    ToolCall,
    /// Result from tool execution
    ToolResult,
    /// Planning/reasoning output
    Plan,
    /// Summarization
    Summary,
    /// Context loading
    Context,
    /// Error or exception
    Error,
}

impl MessageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageType::Prompt => "prompt",
            MessageType::Response => "response",
            MessageType::ToolCall => "tool_call",
            MessageType::ToolResult => "tool_result",
            MessageType::Plan => "plan",
            MessageType::Summary => "summary",
            MessageType::Context => "context",
            MessageType::Error => "error",
        }
    }
}

impl std::str::FromStr for MessageType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "prompt" => Ok(MessageType::Prompt),
            "response" => Ok(MessageType::Response),
            "tool_call" => Ok(MessageType::ToolCall),
            "tool_result" => Ok(MessageType::ToolResult),
            "plan" => Ok(MessageType::Plan),
            "summary" => Ok(MessageType::Summary),
            "context" => Ok(MessageType::Context),
            "error" => Ok(MessageType::Error),
            _ => Err(format!("unknown message type: {}", s)),
        }
    }
}

// ============================================
// Content Types
// ============================================

/// MIME-like content type for message content.
///
/// Describes the format of message content, similar to HTTP Content-Type headers.
/// Used to distinguish text prompts from images, binary data, or unknown content types.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentType {
    /// Plain text content (default)
    #[default]
    Text,
    /// Image content with media type and optional encoding
    Image {
        /// Image format: "png", "jpeg", "gif", "webp", etc.
        media_type: String,
        /// Encoding used: "base64" for embedded images
        encoding: Option<String>,
    },
    /// Unknown or unparsed content type (fallback)
    Unknown(String),
}

impl ContentType {
    /// Create an image content type with base64 encoding
    pub fn image_base64(media_type: &str) -> Self {
        ContentType::Image {
            media_type: media_type.to_string(),
            encoding: Some("base64".to_string()),
        }
    }

    /// Check if this is text content
    pub fn is_text(&self) -> bool {
        matches!(self, ContentType::Text)
    }

    /// Check if this is image content
    pub fn is_image(&self) -> bool {
        matches!(self, ContentType::Image { .. })
    }
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentType::Text => write!(f, "text"),
            ContentType::Image {
                media_type,
                encoding,
            } => {
                if let Some(enc) = encoding {
                    write!(f, "image/{};{}", media_type, enc)
                } else {
                    write!(f, "image/{}", media_type)
                }
            }
            ContentType::Unknown(s) => write!(f, "unknown:{}", s),
        }
    }
}

impl std::str::FromStr for ContentType {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "text" {
            Ok(ContentType::Text)
        } else if let Some(rest) = s.strip_prefix("image/") {
            // Parse "image/png" or "image/png;base64"
            if let Some((media, enc)) = rest.split_once(';') {
                Ok(ContentType::Image {
                    media_type: media.to_string(),
                    encoding: Some(enc.to_string()),
                })
            } else {
                Ok(ContentType::Image {
                    media_type: rest.to_string(),
                    encoding: None,
                })
            }
        } else if let Some(rest) = s.strip_prefix("unknown:") {
            Ok(ContentType::Unknown(rest.to_string()))
        } else {
            Ok(ContentType::Unknown(s.to_string()))
        }
    }
}

/// A message within a session (the core unit of activity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Database ID (auto-incremented)
    pub id: i64,
    /// Session this message belongs to
    pub session_id: String,
    /// Thread this message belongs to
    pub thread_id: String,
    /// Sequence number within the thread
    pub seq: i32,

    // Timestamps
    /// When the event actually happened (from log file, or approximated from surrounding events)
    pub emitted_at: DateTime<Utc>,
    /// When we parsed/ingested this event
    pub observed_at: DateTime<Utc>,

    // Who authored this message
    /// Author role
    pub author_role: AuthorRole,
    /// Author name (for tools: "Read"; for agents: agent_id)
    pub author_name: Option<String>,

    // What kind of message
    /// Type of message
    pub message_type: MessageType,

    // Content
    /// Text content (prompt, response, etc.)
    pub content: Option<String>,
    /// MIME-like type of content (text, image, unknown)
    pub content_type: Option<ContentType>,
    /// Name of tool called (for tool_call/tool_result)
    pub tool_name: Option<String>,
    /// Input to the tool (JSON)
    pub tool_input: Option<serde_json::Value>,
    /// Result from the tool
    pub tool_result: Option<String>,

    // Token usage (if available)
    /// Input tokens consumed
    pub tokens_in: Option<i32>,
    /// Output tokens generated
    pub tokens_out: Option<i32>,
    /// Duration in milliseconds
    pub duration_ms: Option<i32>,

    // Lineage - trace back to raw source
    /// FK to source_files table (path is PK)
    pub source_file_path: String,
    /// Byte offset in source file
    pub source_offset: i64,
    /// Line number in source file (if applicable)
    pub source_line: Option<i32>,

    // Lossless capture
    /// Complete original record - NEVER loses data
    pub raw_data: serde_json::Value,
    /// Parsed assistant-specific fields we recognized
    pub metadata: serde_json::Value,
}

impl Message {
    /// Get the Author for this message
    pub fn author(&self) -> Author {
        Author {
            role: self.author_role,
            name: self.author_name.clone(),
        }
    }

    /// Check if this message is part of the human-assistant conversation
    /// (excludes tool calls, agent messages, system context)
    pub fn is_conversation_message(&self) -> bool {
        matches!(self.author_role, AuthorRole::Human | AuthorRole::Assistant)
            && matches!(
                self.message_type,
                MessageType::Prompt | MessageType::Response
            )
    }

    /// Get a one-line preview of the message content suitable for display.
    ///
    /// For tool calls, shows `<tool_name> argument_preview`.
    /// For tool results, shows the first line of output.
    /// For other messages, shows the first line of content.
    pub fn preview(&self, max_len: usize) -> String {
        match self.message_type {
            MessageType::ToolCall => {
                let tool = self.tool_name.as_deref().unwrap_or("unknown");
                let arg_preview = self.tool_arg_preview();
                if arg_preview.is_empty() {
                    format!("<{}>", tool)
                } else {
                    Self::truncate(&format!("<{}> {}", tool, arg_preview), max_len)
                }
            }
            MessageType::ToolResult => self
                .tool_result
                .as_ref()
                .and_then(|r| r.lines().next())
                .map(|s| Self::truncate(s, max_len))
                .unwrap_or_else(|| "[result]".to_string()),
            _ => self
                .content
                .as_ref()
                .and_then(|c| c.lines().next())
                .map(|s| Self::truncate(s, max_len))
                .unwrap_or_else(|| {
                    // Fallback to tool_name for any other message with tool_name
                    self.tool_name
                        .as_ref()
                        .map(|t| format!("<{}>", t))
                        .unwrap_or_default()
                }),
        }
    }

    /// Extract a meaningful preview from tool_input JSON.
    /// Tries common field names used by various tools (command, file_path, etc.)
    fn tool_arg_preview(&self) -> String {
        self.tool_input
            .as_ref()
            .and_then(|v| {
                v.get("command")
                    .or_else(|| v.get("file_path"))
                    .or_else(|| v.get("filePath"))
                    .or_else(|| v.get("path"))
                    .or_else(|| v.get("pattern"))
                    .or_else(|| v.get("query"))
                    .or_else(|| v.get("content"))
                    .or_else(|| v.get("description"))
            })
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    /// Truncate a string to max length with ellipsis.
    fn truncate(s: &str, max_len: usize) -> String {
        if s.len() <= max_len {
            s.to_string()
        } else {
            // Find a valid char boundary at or before max_len - 3 (for "...")
            let target = max_len.saturating_sub(3);
            let mut end = target;
            while !s.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}...", &s[..end])
        }
    }
}

// ============================================
// Plans
// ============================================

/// Status of a plan file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    /// Plan is being worked on
    Active,
    /// Plan was executed
    Completed,
    /// Plan was discarded
    Abandoned,
    /// Status not determined
    Unknown,
}

impl PlanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanStatus::Active => "active",
            PlanStatus::Completed => "completed",
            PlanStatus::Abandoned => "abandoned",
            PlanStatus::Unknown => "unknown",
        }
    }
}

impl std::str::FromStr for PlanStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(PlanStatus::Active),
            "completed" => Ok(PlanStatus::Completed),
            "abandoned" => Ok(PlanStatus::Abandoned),
            "unknown" => Ok(PlanStatus::Unknown),
            _ => Err(format!("unknown plan status: {}", s)),
        }
    }
}

/// A plan file associated with a session (tracked separately)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Unique identifier
    pub id: String,
    /// Session this plan is associated with
    pub session_id: String,
    /// Path to the plan file (e.g., ~/.claude/plans/foo.md)
    pub path: PathBuf,
    /// Extracted from plan content
    pub title: Option<String>,
    /// When the plan was created
    pub created_at: DateTime<Utc>,
    /// When the plan was last modified
    pub modified_at: DateTime<Utc>,
    /// Status of the plan
    pub status: PlanStatus,
    /// Full content of the plan file
    pub content: Option<String>,

    // Lineage
    /// FK to source_files table
    pub source_file_path: String,

    // Lossless capture
    /// Original data if from structured source
    pub raw_data: serde_json::Value,
    /// Parsed fields
    pub metadata: serde_json::Value,
}

// ============================================
// Metrics (Layer 2 - Derived)
// ============================================

/// Aggregated metrics for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    /// Session this metrics belongs to
    pub session_id: String,
    /// Schema version for recomputation
    pub metric_version: i32,
    /// When these metrics were computed
    pub computed_at: DateTime<Utc>,

    // First-order aggregations
    /// Total input tokens
    pub total_tokens_in: i32,
    /// Total output tokens
    pub total_tokens_out: i32,
    /// Total tool calls made
    pub total_tool_calls: i32,
    /// Breakdown of tool calls by tool name
    pub tool_call_breakdown: HashMap<String, i32>,
    /// Number of errors encountered
    pub error_count: i32,
    /// Total session duration in milliseconds
    pub duration_ms: i64,

    // Higher-order derived
    /// Tokens generated per minute
    pub tokens_per_minute: f64,
    /// Ratio of successful tool calls
    pub tool_success_rate: f64,
    /// Ratio of re-edits to same file regions
    pub edit_churn_ratio: f64,
}

/// LLM-generated assessment of a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assessment {
    /// Database ID
    pub id: i64,
    /// Session this assessment is for
    pub session_id: String,
    /// Name of the plugin/assessor that generated this
    pub assessor: String,
    /// LLM model used (if applicable)
    pub model: Option<String>,
    /// When this assessment was generated
    pub assessed_at: DateTime<Utc>,
    /// Structured scores (e.g., {"sycophancy": 0.3, "clarity": 0.8})
    pub scores: serde_json::Value,
    /// Raw LLM response for debugging
    pub raw_response: Option<String>,
    /// Hash of the prompt for cache invalidation
    pub prompt_hash: Option<String>,
}

/// Generic plugin metric output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetric {
    /// Database ID
    pub id: i64,
    /// Name of the plugin that generated this
    pub plugin_name: String,
    /// Type of entity this metric is for
    pub entity_type: String,
    /// ID of the entity (session_id, message_id, etc.)
    pub entity_id: Option<String>,
    /// Name of the metric
    pub metric_name: String,
    /// Value of the metric (JSON for flexibility)
    pub metric_value: serde_json::Value,
    /// When this metric was computed
    pub computed_at: DateTime<Utc>,
}

// ============================================
// Discovery
// ============================================

/// Information about a discovered assistant installation
#[derive(Debug, Clone)]
pub struct DiscoveredAssistant {
    /// Type of assistant
    pub assistant: Assistant,
    /// Root path where assistant data is stored
    pub root_path: PathBuf,
    /// Number of sessions found
    pub session_count: usize,
    /// Whether the assistant appears to be active
    pub is_active: bool,
}

// ============================================
// Backward Compatibility Type Aliases
// ============================================

// These aliases help with migration from the old naming scheme
// TODO: Remove after full migration

/// Alias for backward compatibility
#[deprecated(note = "Use Assistant instead")]
pub type AgentType = Assistant;

/// Alias for backward compatibility
#[deprecated(note = "Use Message instead")]
pub type Event = Message;

/// Alias for backward compatibility
#[deprecated(note = "Use MessageType instead")]
pub type EventType = MessageType;

/// Alias for backward compatibility
#[deprecated(note = "Use DiscoveredAssistant instead")]
pub type DiscoveredAgent = DiscoveredAssistant;

// ============================================
// Live View Types
// ============================================

/// A message with project/thread context for the live stream view.
///
/// This is a lightweight representation of a message with enough context
/// to display in a multi-session live stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageWithContext {
    /// Database ID
    pub id: i64,
    /// When the event actually happened
    pub emitted_at: DateTime<Utc>,
    /// Which assistant produced this message
    pub assistant: Assistant,
    /// Project name (or "(no project)")
    pub project_name: String,
    /// Thread name ("main" or agent short ID)
    pub thread_name: String,
    /// Author role
    pub author_role: AuthorRole,
    /// Type of message
    pub message_type: MessageType,
    /// Preview text (first ~60 chars of content)
    pub preview: String,
    /// Tool name (for tool calls)
    pub tool_name: Option<String>,
}

/// An active session/thread for the live view's Active Sessions panel.
///
/// Represents a thread that has had recent activity, with enough context
/// to display in a hierarchical session list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSession {
    /// Session ID
    pub session_id: String,
    /// Thread ID
    pub thread_id: String,
    /// Project name (or "(no project)")
    pub project_name: String,
    /// Thread type (Main or Agent)
    pub thread_type: ThreadType,
    /// Which AI assistant
    pub assistant: Assistant,
    /// Timestamp of last activity
    pub last_activity: DateTime<Utc>,
    /// Total message count for this thread
    pub message_count: i64,
    /// Parent thread ID (for agent hierarchy)
    pub parent_thread_id: Option<String>,
}

/// Aggregate statistics for the live view's stats toolbar.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LiveStats {
    /// Total number of messages in the time window
    pub total_messages: i64,
    /// Total tokens (input + output) in the time window
    pub total_tokens: i64,
    /// Total number of agent threads spawned in the time window
    pub total_agents: i64,
    /// Total number of tool calls in the time window
    pub total_tool_calls: i64,
}
