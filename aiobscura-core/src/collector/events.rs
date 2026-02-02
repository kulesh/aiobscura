//! Event transformation for Catsyphon API
//!
//! Converts aiobscura's `Message` type to Catsyphon's `CollectorEvent` format.
//!
//! ## Timestamp Semantics
//!
//! Events flow through a three-stage pipeline:
//!
//! ```text
//! Source (Claude Code logs) → Collector (aiobscura) → Catsyphon Server
//!         emitted_at                observed_at         server_received_at
//! ```
//!
//! - `emitted_at`: When the event was originally produced by the AI assistant
//! - `observed_at`: When aiobscura parsed this event from the log file
//! - `server_received_at`: Set by Catsyphon server (not our concern)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::types::{AuthorRole, Message, MessageType};

/// Catsyphon event envelope
///
/// This struct matches the schema expected by Catsyphon's `/collectors/events` API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorEvent {
    /// Event type (message, tool_call, tool_result, error, etc.)
    #[serde(rename = "type")]
    pub event_type: String,

    /// When the event was originally produced (from source log)
    pub emitted_at: DateTime<Utc>,

    /// When aiobscura parsed this event
    pub observed_at: DateTime<Utc>,

    /// Content-based hash for deduplication (32-char hex)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_hash: Option<String>,

    /// Type-specific event payload
    pub data: serde_json::Value,
}

impl CollectorEvent {
    /// Create a CollectorEvent from an aiobscura Message
    pub fn from_message(msg: &Message) -> Self {
        let event_type = map_message_type(&msg.message_type);
        let data = build_event_data(msg);
        let event_hash = compute_event_hash(&event_type, &msg.emitted_at, &data);

        CollectorEvent {
            event_type,
            emitted_at: msg.emitted_at,
            observed_at: msg.observed_at,
            event_hash: Some(event_hash),
            data,
        }
    }
}

/// Map aiobscura MessageType to Catsyphon event type string
fn map_message_type(mt: &MessageType) -> String {
    match mt {
        MessageType::Prompt | MessageType::Response => "message",
        MessageType::ToolCall => "tool_call",
        MessageType::ToolResult => "tool_result",
        MessageType::Plan => "message",
        MessageType::Summary => "message",
        MessageType::Context => "message",
        MessageType::Error => "error",
    }
    .to_string()
}

/// Map aiobscura AuthorRole to Catsyphon author_role string
fn map_author_role(role: &AuthorRole) -> String {
    match role {
        AuthorRole::Human => "human",
        AuthorRole::Caller => "caller",
        AuthorRole::Assistant => "assistant",
        AuthorRole::Agent => "agent",
        AuthorRole::Tool => "tool",
        AuthorRole::System => "system",
    }
    .to_string()
}

/// Map aiobscura MessageType to Catsyphon message_type string (for data payload)
fn map_message_type_for_data(mt: &MessageType) -> String {
    match mt {
        MessageType::Prompt => "prompt",
        MessageType::Response => "response",
        MessageType::ToolCall => "tool_call",
        MessageType::ToolResult => "tool_result",
        MessageType::Plan => "plan",
        MessageType::Summary => "summary",
        MessageType::Context => "context",
        MessageType::Error => "error",
    }
    .to_string()
}

/// Build the type-specific data payload for a Catsyphon event
fn build_event_data(msg: &Message) -> serde_json::Value {
    match msg.message_type {
        MessageType::ToolCall => build_tool_call_data(msg),
        MessageType::ToolResult => build_tool_result_data(msg),
        MessageType::Error => build_error_data(msg),
        _ => build_message_data(msg),
    }
}

/// Build data payload for message events (prompt, response, plan, summary, context)
fn build_message_data(msg: &Message) -> serde_json::Value {
    let mut data = serde_json::json!({
        "author_role": map_author_role(&msg.author_role),
        "message_type": map_message_type_for_data(&msg.message_type),
    });

    if let Some(content) = &msg.content {
        data["content"] = serde_json::Value::String(content.clone());
    }

    // Add token usage if available
    if let Some(tokens_in) = msg.tokens_in {
        data["token_usage"] = serde_json::json!({
            "input_tokens": tokens_in,
            "output_tokens": msg.tokens_out.unwrap_or(0),
        });
    }

    // Preserve raw_data for lossless transmission
    if !msg.raw_data.is_null() {
        data["raw_data"] = msg.raw_data.clone();
    }

    data
}

/// Build data payload for tool_call events
fn build_tool_call_data(msg: &Message) -> serde_json::Value {
    let mut data = serde_json::json!({
        "tool_name": msg.tool_name.clone().unwrap_or_else(|| "unknown".to_string()),
    });

    // Add tool_use_id from metadata if available
    if let Some(tool_use_id) = msg.metadata.get("tool_use_id") {
        data["tool_use_id"] = tool_use_id.clone();
    }

    // Add parameters (tool_input)
    if let Some(input) = &msg.tool_input {
        data["parameters"] = input.clone();
    }

    data
}

/// Build data payload for tool_result events
fn build_tool_result_data(msg: &Message) -> serde_json::Value {
    let mut data = serde_json::json!({});

    // Add tool_use_id from metadata if available
    if let Some(tool_use_id) = msg.metadata.get("tool_use_id") {
        data["tool_use_id"] = tool_use_id.clone();
    }

    // Determine success from metadata or assume true if we have a result
    let success = msg
        .metadata
        .get("is_error")
        .and_then(|v| v.as_bool())
        .map(|is_err| !is_err)
        .unwrap_or(true);
    data["success"] = serde_json::Value::Bool(success);

    // Add result content
    if let Some(result) = &msg.tool_result {
        data["result"] = serde_json::Value::String(result.clone());
    }

    // Add error message if this was an error
    if !success {
        if let Some(content) = &msg.content {
            data["error_message"] = serde_json::Value::String(content.clone());
        }
    }

    data
}

/// Build data payload for error events
fn build_error_data(msg: &Message) -> serde_json::Value {
    let mut data = serde_json::json!({
        "error_type": "unknown",
    });

    if let Some(content) = &msg.content {
        data["message"] = serde_json::Value::String(content.clone());
    }

    // Try to extract error_type from metadata
    if let Some(error_type) = msg.metadata.get("error_type") {
        data["error_type"] = error_type.clone();
    }

    data
}

/// Compute a content-based hash for event deduplication
///
/// Returns a 32-character hex digest of SHA-256(event_type + emitted_at + data)
fn compute_event_hash(
    event_type: &str,
    emitted_at: &DateTime<Utc>,
    data: &serde_json::Value,
) -> String {
    let content = serde_json::to_string(data).unwrap_or_default();
    let hash_input = format!("{}:{}:{}", event_type, emitted_at.to_rfc3339(), content);

    let mut hasher = Sha256::new();
    hasher.update(hash_input.as_bytes());
    let result = hasher.finalize();

    // Take first 16 bytes (32 hex chars)
    hex::encode(&result[..16])
}

/// Batch of events to send to Catsyphon
#[derive(Debug, Clone, Serialize)]
pub struct EventBatch {
    /// Session ID these events belong to
    pub session_id: String,

    /// Events to send
    pub events: Vec<CollectorEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AuthorRole;

    fn make_test_message() -> Message {
        Message {
            id: 1,
            session_id: "test-session".to_string(),
            thread_id: "test-thread".to_string(),
            seq: 1,
            emitted_at: Utc::now(),
            observed_at: Utc::now(),
            author_role: AuthorRole::Human,
            author_name: None,
            message_type: MessageType::Prompt,
            content: Some("Hello, world!".to_string()),
            content_type: None,
            tool_name: None,
            tool_input: None,
            tool_result: None,
            tokens_in: Some(10),
            tokens_out: None,
            duration_ms: None,
            source_file_path: "/test/path".to_string(),
            source_offset: 0,
            source_line: Some(1),
            raw_data: serde_json::json!({}),
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn test_message_to_collector_event() {
        let msg = make_test_message();
        let event = CollectorEvent::from_message(&msg);

        assert_eq!(event.event_type, "message");
        assert!(event.event_hash.is_some());
        assert_eq!(event.data["author_role"], "human");
        assert_eq!(event.data["message_type"], "prompt");
        assert_eq!(event.data["content"], "Hello, world!");
    }

    #[test]
    fn test_tool_call_event() {
        let mut msg = make_test_message();
        msg.message_type = MessageType::ToolCall;
        msg.author_role = AuthorRole::Assistant;
        msg.tool_name = Some("Read".to_string());
        msg.tool_input = Some(serde_json::json!({"file_path": "/test.txt"}));
        msg.metadata = serde_json::json!({"tool_use_id": "toolu_123"});

        let event = CollectorEvent::from_message(&msg);

        assert_eq!(event.event_type, "tool_call");
        assert_eq!(event.data["tool_name"], "Read");
        assert_eq!(event.data["tool_use_id"], "toolu_123");
        assert_eq!(event.data["parameters"]["file_path"], "/test.txt");
    }

    #[test]
    fn test_tool_result_event() {
        let mut msg = make_test_message();
        msg.message_type = MessageType::ToolResult;
        msg.author_role = AuthorRole::Tool;
        msg.tool_result = Some("file contents here".to_string());
        msg.metadata = serde_json::json!({"tool_use_id": "toolu_123"});

        let event = CollectorEvent::from_message(&msg);

        assert_eq!(event.event_type, "tool_result");
        assert_eq!(event.data["tool_use_id"], "toolu_123");
        assert_eq!(event.data["success"], true);
        assert_eq!(event.data["result"], "file contents here");
    }

    #[test]
    fn test_event_hash_deterministic() {
        let msg = make_test_message();
        let event1 = CollectorEvent::from_message(&msg);
        let event2 = CollectorEvent::from_message(&msg);

        assert_eq!(event1.event_hash, event2.event_hash);
    }

    #[test]
    fn test_map_message_types() {
        assert_eq!(map_message_type(&MessageType::Prompt), "message");
        assert_eq!(map_message_type(&MessageType::Response), "message");
        assert_eq!(map_message_type(&MessageType::ToolCall), "tool_call");
        assert_eq!(map_message_type(&MessageType::ToolResult), "tool_result");
        assert_eq!(map_message_type(&MessageType::Error), "error");
    }

    #[test]
    fn test_map_author_roles() {
        assert_eq!(map_author_role(&AuthorRole::Human), "human");
        assert_eq!(map_author_role(&AuthorRole::Assistant), "assistant");
        assert_eq!(map_author_role(&AuthorRole::Tool), "tool");
        assert_eq!(map_author_role(&AuthorRole::Agent), "agent");
    }
}
