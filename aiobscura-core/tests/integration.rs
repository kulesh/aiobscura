//! Integration tests for aiobscura parser and ingestion pipeline
//!
//! These tests use fixture files in `tests/fixtures/claude-code/` to verify
//! the end-to-end parsing and database storage flow.

use aiobscura_core::db::Database;
use aiobscura_core::ingest::parsers::{ClaudeCodeParser, CodexParser};
use aiobscura_core::ingest::{AssistantParser, ParseContext};
use aiobscura_core::types::{Assistant, AuthorRole, Checkpoint, Message, MessageType};
use std::path::PathBuf;
use tempfile::TempDir;

/// Get the path to a fixture file
fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude-code")
        .join(name)
}

/// Create a parse context for a fixture file
fn parse_context(path: &PathBuf) -> ParseContext<'_> {
    let metadata = std::fs::metadata(path).unwrap();
    ParseContext {
        path,
        checkpoint: &Checkpoint::None,
        file_size: metadata.len(),
        modified_at: chrono::Utc::now(),
    }
}

// ============================================
// Basic Parsing Tests
// ============================================

#[test]
fn test_parse_minimal_session() {
    let path = fixture_path("minimal-session.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    // Should have created a session
    assert!(result.session.is_some());
    let session = result.session.as_ref().unwrap();
    assert_eq!(session.id, "test-session-001");

    // Should have created one main thread
    assert_eq!(result.threads.len(), 1);
    assert_eq!(
        result.threads[0].thread_type,
        aiobscura_core::types::ThreadType::Main
    );

    // Should have parsed 4 messages (2 user + 2 assistant)
    assert_eq!(result.messages.len(), 4);

    // Check message types
    let user_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.author_role == AuthorRole::Human)
        .collect();
    let assistant_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.author_role == AuthorRole::Assistant)
        .collect();

    assert_eq!(user_msgs.len(), 2);
    assert_eq!(assistant_msgs.len(), 2);

    // Check first user message
    assert_eq!(user_msgs[0].message_type, MessageType::Prompt);
    assert!(user_msgs[0].content.as_ref().unwrap().contains("Hello"));

    // Check first assistant message has token counts
    assert!(assistant_msgs[0].tokens_in.is_some());
    assert!(assistant_msgs[0].tokens_out.is_some());
    assert_eq!(assistant_msgs[0].tokens_in.unwrap(), 50);
    assert_eq!(assistant_msgs[0].tokens_out.unwrap(), 25);
}

#[test]
fn test_parse_with_tool_calls() {
    let path = fixture_path("with-tool-calls.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    // Count tool calls and results
    let tool_calls: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.message_type == MessageType::ToolCall)
        .collect();
    let tool_results: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.message_type == MessageType::ToolResult)
        .collect();

    // Should have 2 tool calls (Read and Bash)
    assert_eq!(tool_calls.len(), 2);

    // Should have 2 tool results
    assert_eq!(tool_results.len(), 2);

    // Check tool names
    let tool_names: Vec<_> = tool_calls
        .iter()
        .filter_map(|m| m.tool_name.as_ref())
        .collect();
    assert!(tool_names.contains(&&"Read".to_string()));
    assert!(tool_names.contains(&&"Bash".to_string()));

    // Check tool input is captured
    let read_call = tool_calls
        .iter()
        .find(|m| m.tool_name.as_ref() == Some(&"Read".to_string()))
        .unwrap();
    assert!(read_call.tool_input.is_some());
}

#[test]
fn test_parse_empty_file() {
    let path = fixture_path("empty.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    // Empty file should produce no session, threads, or messages
    assert!(result.session.is_none());
    assert!(result.threads.is_empty());
    assert!(result.messages.is_empty());
}

#[test]
fn test_parse_malformed_json_recovery() {
    let path = fixture_path("malformed-lines.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);

    let result = parser
        .parse(&ctx)
        .expect("parse should succeed despite bad lines");

    // Should have parsed valid messages, skipping malformed lines
    // The fixture has 4 valid messages and 2 invalid lines
    assert!(!result.messages.is_empty());

    // Should have warnings about malformed lines
    assert!(
        !result.warnings.is_empty(),
        "should have warnings about bad JSON"
    );

    // Session should still be created from valid messages
    assert!(result.session.is_some());
}

#[test]
fn test_parse_truncated_file() {
    let path = fixture_path("truncated.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    // Should parse complete messages, ignore truncated last line
    // The fixture has 2 complete messages and 1 truncated
    assert!(result.messages.len() >= 2);

    // Session should be created
    assert!(result.session.is_some());
}

// ============================================
// Agent Spawn Linkage Tests
// ============================================

#[test]
fn test_agent_spawn_map_extraction() {
    let path = fixture_path("with-agent-spawn.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    // Should have extracted agent spawn mapping
    assert!(
        !result.agent_spawn_map.is_empty(),
        "should have agent spawn map"
    );

    // Should map agent ID to spawning message seq
    assert!(result.agent_spawn_map.contains_key("a1234567"));
}

#[test]
fn test_agent_file_parsing() {
    let path = fixture_path("agent-a1234567.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    // Should have created session and thread
    assert!(result.session.is_some());
    assert!(!result.threads.is_empty());

    // Thread should be agent type
    assert_eq!(
        result.threads[0].thread_type,
        aiobscura_core::types::ThreadType::Agent
    );

    // Should have parsed agent messages
    assert!(!result.messages.is_empty());
}

// ============================================
// Incremental Parsing Tests
// ============================================

#[test]
fn test_incremental_parsing() {
    let path = fixture_path("minimal-session.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());

    // First parse - from beginning
    let ctx1 = parse_context(&path);
    let result1 = parser.parse(&ctx1).expect("first parse should succeed");

    let first_message_count = result1.messages.len();
    assert!(first_message_count > 0);

    // Get checkpoint from first parse
    let checkpoint = result1.new_checkpoint.clone();

    // Second parse - from checkpoint (should find no new messages)
    let metadata = std::fs::metadata(&path).unwrap();
    let ctx2 = ParseContext {
        path: &path,
        checkpoint: &checkpoint,
        file_size: metadata.len(),
        modified_at: chrono::Utc::now(),
    };

    let result2 = parser.parse(&ctx2).expect("second parse should succeed");

    // No new messages since file hasn't changed
    assert_eq!(
        result2.messages.len(),
        0,
        "incremental parse should find no new messages"
    );
}

#[test]
fn test_checkpoint_beyond_file_size_resets() {
    let path = fixture_path("minimal-session.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());

    // Create a checkpoint beyond the file size (simulating truncation)
    let metadata = std::fs::metadata(&path).unwrap();
    let fake_checkpoint = Checkpoint::ByteOffset {
        offset: metadata.len() + 1000,
    };

    let ctx = ParseContext {
        path: &path,
        checkpoint: &fake_checkpoint,
        file_size: metadata.len(),
        modified_at: chrono::Utc::now(),
    };

    let result = parser.parse(&ctx).expect("parse should reset and succeed");

    // Should have parsed from beginning (reset behavior)
    assert!(
        !result.messages.is_empty(),
        "should have parsed messages after reset"
    );

    // Should have warning about truncation
    // Note: The parser may or may not emit a warning here - implementation dependent
}

// ============================================
// Database Integration Tests
// ============================================

#[test]
fn test_full_sync_pipeline() {
    // Create temporary database
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db = Database::open(&db_path).expect("database should open");
    db.migrate().expect("migrations should run");

    // Parse a fixture file
    let path = fixture_path("minimal-session.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);
    let result = parser.parse(&ctx).expect("parse should succeed");

    // Get session source_file_path to use for the SourceFile
    // (must match exactly for foreign key constraint)
    let session_source_path = result
        .session
        .as_ref()
        .map(|s| s.source_file_path.clone())
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    // Store source file first (required for foreign key constraint)
    let source_file = aiobscura_core::types::SourceFile {
        path: PathBuf::from(&session_source_path),
        file_type: aiobscura_core::types::FileType::Jsonl,
        assistant: aiobscura_core::types::Assistant::ClaudeCode,
        created_at: chrono::Utc::now(),
        modified_at: chrono::Utc::now(),
        size_bytes: std::fs::metadata(&path).unwrap().len(),
        last_parsed_at: Some(chrono::Utc::now()),
        checkpoint: result.new_checkpoint.clone(),
    };
    db.upsert_source_file(&source_file)
        .expect("source file insert should succeed");

    // Store project
    if let Some(ref project) = result.project {
        db.upsert_project(project)
            .expect("project insert should succeed");
    }

    // Insert backing model if session references one
    if let Some(ref session) = result.session {
        if let Some(ref model_id) = session.backing_model_id {
            // Create a minimal backing model record
            let backing_model = aiobscura_core::types::BackingModel {
                id: model_id.clone(),
                provider: "anthropic".to_string(),
                model_id: model_id.clone(),
                display_name: Some(model_id.clone()),
                first_seen_at: chrono::Utc::now(),
                metadata: serde_json::json!({}),
            };
            db.upsert_backing_model(&backing_model)
                .expect("backing model insert should succeed");
        }
    }

    // Store session
    if let Some(ref session) = result.session {
        db.upsert_session(session)
            .expect("session insert should succeed");
    }

    // Store threads
    for thread in &result.threads {
        db.insert_thread(thread)
            .expect("thread insert should succeed");
    }

    // Store messages
    if !result.messages.is_empty() {
        db.insert_messages(&result.messages)
            .expect("message insert should succeed");
    }

    // Verify data was stored
    let session = db
        .get_session("test-session-001")
        .expect("query should succeed");
    assert!(session.is_some(), "session should exist in database");
    let session = session.unwrap();
    assert_eq!(session.id, "test-session-001");

    let threads = db
        .get_session_threads(&session.id)
        .expect("query should succeed");
    assert_eq!(threads.len(), 1);

    let messages = db
        .get_session_messages(&session.id, 100)
        .expect("query should succeed");
    assert_eq!(messages.len(), 4);
}

// ============================================
// Edge Case Tests
// ============================================

#[test]
fn test_session_metadata_extraction() {
    let path = fixture_path("minimal-session.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    let session = result.session.as_ref().unwrap();

    // Check metadata fields are captured
    let metadata = &session.metadata;
    assert!(metadata.get("cwd").is_some(), "cwd should be in metadata");
    assert!(
        metadata.get("git_branch").is_some(),
        "git_branch should be in metadata"
    );
}

#[test]
fn test_backing_model_extraction() {
    let path = fixture_path("minimal-session.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    let session = result.session.as_ref().unwrap();

    // Should have extracted backing model
    assert!(session.backing_model_id.is_some());
    let model_id = session.backing_model_id.as_ref().unwrap();
    assert!(
        model_id.contains("claude"),
        "model ID should contain 'claude'"
    );
}

// ============================================
// Codex Parser Tests
// ============================================

/// Get the path to a Codex fixture file
fn codex_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codex")
        .join(name)
}

/// Create a parse context for a Codex fixture file
fn codex_parse_context(path: &PathBuf) -> ParseContext<'_> {
    let metadata = std::fs::metadata(path).unwrap();
    ParseContext {
        path,
        checkpoint: &Checkpoint::None,
        file_size: metadata.len(),
        modified_at: chrono::Utc::now(),
    }
}

#[test]
fn test_codex_parse_minimal_session() {
    let path = codex_fixture_path("minimal-session.jsonl");
    let parser = CodexParser::with_root(codex_fixture_path("").parent().unwrap().to_path_buf());
    let ctx = codex_parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    // Should have created a session
    assert!(result.session.is_some());
    let session = result.session.as_ref().unwrap();
    assert_eq!(session.id, "019ab86e-1e83-75b0-b2d7-d335492e7026");
    assert_eq!(session.assistant, Assistant::Codex);

    // Should have backing model
    assert!(session.backing_model_id.is_some());
    let model_id = session.backing_model_id.as_ref().unwrap();
    assert!(
        model_id.contains("openai:"),
        "model ID should have openai prefix"
    );
    assert!(model_id.contains("gpt"), "model ID should contain 'gpt'");

    // Should have created one main thread
    assert_eq!(result.threads.len(), 1);
    assert_eq!(
        result.threads[0].thread_type,
        aiobscura_core::types::ThreadType::Main
    );

    // Should have parsed messages (user + assistant)
    assert!(!result.messages.is_empty());

    // Check for caller context messages (environment_context, etc.)
    let caller_context_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.author_role == AuthorRole::Caller && m.message_type == MessageType::Context)
        .collect();
    assert!(
        !caller_context_msgs.is_empty(),
        "should have at least 1 caller context message (environment_context)"
    );

    // Check for caller prompt messages (CLI invocation)
    let caller_prompt_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.author_role == AuthorRole::Caller && m.message_type == MessageType::Prompt)
        .collect();
    assert!(
        !caller_prompt_msgs.is_empty(),
        "should have at least 1 caller prompt (CLI invocation)"
    );

    // Check for human messages (subsequent user prompts)
    let human_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.author_role == AuthorRole::Human)
        .collect();
    assert!(
        !human_msgs.is_empty(),
        "should have at least 1 human message"
    );

    // Check for assistant messages
    let assistant_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.author_role == AuthorRole::Assistant)
        .collect();
    assert!(
        assistant_msgs.len() >= 2,
        "should have at least 2 assistant messages"
    );

    // Check caller prompt content (CLI invocation)
    assert!(caller_prompt_msgs[0]
        .content
        .as_ref()
        .unwrap()
        .contains("list"));
}

#[test]
fn test_codex_parse_with_tool_calls() {
    let path = codex_fixture_path("with-tool-calls.jsonl");
    let parser = CodexParser::with_root(codex_fixture_path("").parent().unwrap().to_path_buf());
    let ctx = codex_parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    // Should have created a session
    assert!(result.session.is_some());
    let session = result.session.as_ref().unwrap();
    assert_eq!(session.id, "019ab86e-2222-3333-4444-555566667777");

    // Count tool calls and results
    let tool_calls: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.message_type == MessageType::ToolCall)
        .collect();
    let tool_results: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.message_type == MessageType::ToolResult)
        .collect();

    // Should have 2 tool calls (shell_command and read_file)
    assert_eq!(tool_calls.len(), 2, "should have 2 tool calls");

    // Should have 2 tool results
    assert_eq!(tool_results.len(), 2, "should have 2 tool results");

    // Check tool names
    let tool_names: Vec<_> = tool_calls
        .iter()
        .filter_map(|m| m.tool_name.as_ref())
        .collect();
    assert!(tool_names.contains(&&"shell_command".to_string()));
    assert!(tool_names.contains(&&"read_file".to_string()));

    // Check tool input is captured
    let shell_call = tool_calls
        .iter()
        .find(|m| m.tool_name.as_ref() == Some(&"shell_command".to_string()))
        .unwrap();
    assert!(shell_call.tool_input.is_some());

    // Check tool result is captured
    let first_result = &tool_results[0];
    assert!(first_result.tool_result.is_some());
    assert!(first_result
        .tool_result
        .as_ref()
        .unwrap()
        .contains("Exit code: 0"));

    // Check call_id is in metadata
    assert!(shell_call.metadata.get("call_id").is_some());
}

#[test]
fn test_codex_parse_empty_file() {
    let path = codex_fixture_path("empty.jsonl");
    let parser = CodexParser::with_root(codex_fixture_path("").parent().unwrap().to_path_buf());
    let ctx = codex_parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    // Empty file should produce no session, threads, or messages
    assert!(result.session.is_none());
    assert!(result.threads.is_empty());
    assert!(result.messages.is_empty());
}

#[test]
fn test_codex_incremental_parsing() {
    let path = codex_fixture_path("minimal-session.jsonl");
    let parser = CodexParser::with_root(codex_fixture_path("").parent().unwrap().to_path_buf());

    // First parse - from beginning
    let ctx1 = codex_parse_context(&path);
    let result1 = parser.parse(&ctx1).expect("first parse should succeed");

    let first_message_count = result1.messages.len();
    assert!(first_message_count > 0);

    // Get checkpoint from first parse
    let checkpoint = result1.new_checkpoint.clone();

    // Second parse - from checkpoint (should find no new messages)
    let metadata = std::fs::metadata(&path).unwrap();
    let ctx2 = ParseContext {
        path: &path,
        checkpoint: &checkpoint,
        file_size: metadata.len(),
        modified_at: chrono::Utc::now(),
    };

    let result2 = parser.parse(&ctx2).expect("second parse should succeed");

    // No new messages since file hasn't changed
    assert_eq!(
        result2.messages.len(),
        0,
        "incremental parse should find no new messages"
    );
}

#[test]
fn test_codex_session_metadata() {
    let path = codex_fixture_path("minimal-session.jsonl");
    let parser = CodexParser::with_root(codex_fixture_path("").parent().unwrap().to_path_buf());
    let ctx = codex_parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    let session = result.session.as_ref().unwrap();

    // Check metadata fields are captured
    let metadata = &session.metadata;
    assert!(metadata.get("cwd").is_some(), "cwd should be in metadata");
    assert!(metadata.get("git").is_some(), "git should be in metadata");

    // Check git info
    let git = metadata.get("git").unwrap();
    assert!(git.get("branch").is_some(), "git branch should be captured");
    assert!(
        git.get("commit_hash").is_some(),
        "git commit_hash should be captured"
    );
}

/// Regression test for incremental Codex parsing bug:
/// When new messages are appended to a Codex log file after the initial parse,
/// the session_id must still be available (extracted from filename) even though
/// the session_meta event at the beginning is skipped.
#[test]
fn test_codex_incremental_parsing_with_new_messages() {
    use std::io::Write;

    // Create a temp file with Codex naming convention (UUID in filename)
    let temp_dir = TempDir::new().unwrap();
    let session_uuid = "019b0113-9f8c-7410-af77-c78e77f3128b";
    let path = temp_dir.path().join(format!(
        "rollout-2025-12-08T22-07-01-{}.jsonl",
        session_uuid
    ));

    // Write initial content: session_meta + one assistant message
    let initial_content = r#"{"timestamp":"2025-12-09T03:07:01.920Z","type":"session_meta","payload":{"id":"019b0113-9f8c-7410-af77-c78e77f3128b","cwd":"/test","git":{"branch":"main"}}}
{"timestamp":"2025-12-09T03:07:01.921Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hello"}]}}"#;
    std::fs::write(&path, initial_content).unwrap();

    // First parse - from beginning
    let parser = CodexParser::with_root(temp_dir.path().to_path_buf());
    let metadata = std::fs::metadata(&path).unwrap();
    let ctx1 = ParseContext {
        path: &path,
        checkpoint: &Checkpoint::None,
        file_size: metadata.len(),
        modified_at: chrono::Utc::now(),
    };
    let result1 = parser.parse(&ctx1).expect("first parse should succeed");

    // Should have 1 message from initial parse
    assert!(!result1.messages.is_empty(), "should have initial messages");

    // Verify initial messages have correct session_id
    let first_msg = &result1.messages[0];
    assert!(
        !first_msg.session_id.is_empty(),
        "initial message should have session_id"
    );
    assert!(
        first_msg.session_id.contains(session_uuid),
        "session_id should contain UUID from filename"
    );

    // Get checkpoint from first parse
    let checkpoint = result1.new_checkpoint.clone();

    // Append a new user message to the file (simulating live Codex activity)
    let new_message = r#"
{"timestamp":"2025-12-09T03:11:12.760Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Tell me a joke"}]}}"#;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(new_message.as_bytes()).unwrap();
    drop(file);

    // Incremental parse - from checkpoint (should find the NEW message)
    let metadata2 = std::fs::metadata(&path).unwrap();
    let ctx2 = ParseContext {
        path: &path,
        checkpoint: &checkpoint,
        file_size: metadata2.len(),
        modified_at: chrono::Utc::now(),
    };
    let result2 = parser
        .parse(&ctx2)
        .expect("incremental parse should succeed");

    // Should have found exactly 1 new message
    assert_eq!(
        result2.messages.len(),
        1,
        "incremental parse should find the appended message"
    );

    // CRITICAL: The new message must have valid session_id and thread_id
    // This is the regression test - before the fix, these would be empty strings
    let new_msg = &result2.messages[0];
    assert!(
        !new_msg.session_id.is_empty(),
        "session_id must NOT be empty on incremental parse (was bug)"
    );
    assert!(
        !new_msg.thread_id.is_empty(),
        "thread_id must NOT be empty on incremental parse (was bug)"
    );
    assert!(
        new_msg.session_id.contains(session_uuid),
        "session_id should contain UUID from filename: got '{}'",
        new_msg.session_id
    );

    // Verify message content
    assert_eq!(new_msg.author_role, AuthorRole::Human);
    assert!(new_msg.content.as_ref().unwrap().contains("joke"));
}

#[test]
fn test_codex_project_creation() {
    let path = codex_fixture_path("minimal-session.jsonl");
    let parser = CodexParser::with_root(codex_fixture_path("").parent().unwrap().to_path_buf());
    let ctx = codex_parse_context(&path);

    let result = parser.parse(&ctx).expect("parse should succeed");

    // Should have created a project from cwd
    assert!(result.project.is_some());
    let project = result.project.as_ref().unwrap();

    // Project path should match cwd from session_meta
    assert_eq!(project.path.to_string_lossy(), "/Users/test/dev/myproject");
    assert_eq!(project.name, Some("myproject".to_string()));
}

// ============================================
// Analytics Plugin Framework Tests
// ============================================

#[test]
fn test_analytics_plugin_framework() {
    use aiobscura_core::analytics::{create_default_engine, PluginRunStatus};

    // Create temporary database
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("analytics-test.db");
    let db = Database::open(&db_path).expect("database should open");
    db.migrate().expect("migrations should run");

    // Parse a fixture file with tool calls
    let path = fixture_path("with-tool-calls.jsonl");
    let parser = ClaudeCodeParser::with_root(fixture_path("").parent().unwrap().to_path_buf());
    let ctx = parse_context(&path);
    let result = parser.parse(&ctx).expect("parse should succeed");

    // Store all the necessary data
    let session_source_path = result
        .session
        .as_ref()
        .map(|s| s.source_file_path.clone())
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    let source_file = aiobscura_core::types::SourceFile {
        path: PathBuf::from(&session_source_path),
        file_type: aiobscura_core::types::FileType::Jsonl,
        assistant: aiobscura_core::types::Assistant::ClaudeCode,
        created_at: chrono::Utc::now(),
        modified_at: chrono::Utc::now(),
        size_bytes: std::fs::metadata(&path).unwrap().len(),
        last_parsed_at: Some(chrono::Utc::now()),
        checkpoint: result.new_checkpoint.clone(),
    };
    db.upsert_source_file(&source_file).unwrap();

    if let Some(ref project) = result.project {
        db.upsert_project(project).unwrap();
    }

    if let Some(ref session) = result.session {
        if let Some(ref model_id) = session.backing_model_id {
            let backing_model = aiobscura_core::types::BackingModel {
                id: model_id.clone(),
                provider: "anthropic".to_string(),
                model_id: model_id.clone(),
                display_name: Some(model_id.clone()),
                first_seen_at: chrono::Utc::now(),
                metadata: serde_json::json!({}),
            };
            db.upsert_backing_model(&backing_model).unwrap();
        }
    }

    let session = result.session.as_ref().expect("should have session");
    db.upsert_session(session).unwrap();

    for thread in &result.threads {
        db.insert_thread(thread).unwrap();
    }

    if !result.messages.is_empty() {
        db.insert_messages(&result.messages).unwrap();
    }

    // Insert synthetic Edit messages for testing the edit_churn plugin
    // This creates: src/main.rs (3 edits), src/lib.rs (2 edits), Cargo.toml (1 edit) = 6 total
    let thread_id = result
        .threads
        .first()
        .map(|t| t.id.as_str())
        .unwrap_or("test-main");
    let edit_messages = vec![
        make_edit_msg(
            &session.id,
            thread_id,
            100,
            "src/main.rs",
            &session_source_path,
        ),
        make_edit_msg(
            &session.id,
            thread_id,
            101,
            "src/main.rs",
            &session_source_path,
        ),
        make_edit_msg(
            &session.id,
            thread_id,
            102,
            "src/main.rs",
            &session_source_path,
        ),
        make_edit_msg(
            &session.id,
            thread_id,
            103,
            "src/lib.rs",
            &session_source_path,
        ),
        make_edit_msg(
            &session.id,
            thread_id,
            104,
            "src/lib.rs",
            &session_source_path,
        ),
        make_edit_msg(
            &session.id,
            thread_id,
            105,
            "Cargo.toml",
            &session_source_path,
        ),
    ];
    db.insert_messages(&edit_messages).unwrap();

    // Create analytics engine and run plugins
    let engine = create_default_engine();

    // Verify plugins are registered
    let plugin_names = engine.plugin_names();
    assert!(
        plugin_names.contains(&"core.edit_churn"),
        "edit_churn plugin should be registered"
    );

    // Load session and messages from database
    let stored_session = db.get_session(&session.id).unwrap().unwrap();
    let stored_messages = db.get_session_messages(&session.id, 10000).unwrap();

    // Run all plugins
    let results = engine.run_all(&stored_session, &stored_messages, &db);
    let result = results
        .iter()
        .find(|r| r.plugin_name == "core.edit_churn")
        .unwrap();
    assert_eq!(result.status, PluginRunStatus::Success);
    assert!(
        result.metrics_produced > 0,
        "Plugin should produce metrics, got {}",
        result.metrics_produced
    );

    // Verify metrics stored in database
    let metrics = db.get_session_plugin_metrics(&session.id).unwrap();
    assert!(
        !metrics.is_empty(),
        "Should have metrics in database for session {}",
        session.id
    );

    let edit_count = metrics
        .iter()
        .find(|m| m.metric_name == "edit_count")
        .expect("should have edit_count metric");
    assert_eq!(edit_count.metric_value.as_i64().unwrap(), 6);

    let unique_files = metrics
        .iter()
        .find(|m| m.metric_name == "unique_files")
        .unwrap();
    assert_eq!(unique_files.metric_value.as_i64().unwrap(), 3);

    let churn_ratio = metrics
        .iter()
        .find(|m| m.metric_name == "churn_ratio")
        .unwrap();
    let ratio = churn_ratio.metric_value.as_f64().unwrap();
    assert!(
        (ratio - 0.5).abs() < 0.001,
        "churn_ratio should be ~0.5, got {}",
        ratio
    );

    let high_churn = metrics
        .iter()
        .find(|m| m.metric_name == "high_churn_files")
        .unwrap();
    let high_churn_arr = high_churn.metric_value.as_array().unwrap();
    assert_eq!(high_churn_arr.len(), 1, "only src/main.rs has 3+ edits");
    assert_eq!(high_churn_arr[0].as_str().unwrap(), "src/main.rs");
}

/// Helper to create an Edit tool message
fn make_edit_msg(
    session_id: &str,
    thread_id: &str,
    seq: i32,
    file_path: &str,
    source_file_path: &str,
) -> Message {
    let now = chrono::Utc::now();
    Message {
        id: seq as i64,
        session_id: session_id.to_string(),
        thread_id: thread_id.to_string(),
        seq,
        emitted_at: now,
        observed_at: now,
        author_role: AuthorRole::Tool,
        author_name: Some("Edit".to_string()),
        message_type: MessageType::ToolCall,
        content: None,
        content_type: None,
        tool_name: Some("Edit".to_string()),
        tool_input: Some(serde_json::json!({
            "file_path": file_path,
            "old_string": "foo",
            "new_string": "bar"
        })),
        tool_result: None,
        tokens_in: None,
        tokens_out: None,
        duration_ms: None,
        source_file_path: source_file_path.to_string(),
        source_offset: 0,
        source_line: None,
        raw_data: serde_json::json!({}),
        metadata: serde_json::json!({}),
    }
}
