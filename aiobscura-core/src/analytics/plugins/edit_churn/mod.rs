//! Edit Churn Analyzer
//!
//! Tracks how many times each file is modified in a session.
//! High churn (same file edited many times) can indicate:
//! - Iterative debugging
//! - Unclear requirements
//! - AI making incremental mistakes
//!
//! ## Metrics Produced
//!
//! For each session:
//!
//! | Metric | Type | Description |
//! |--------|------|-------------|
//! | `edit_count` | integer | Total Edit/Write/MultiEdit tool calls |
//! | `unique_files` | integer | Number of distinct files modified |
//! | `churn_ratio` | float | `(total_edits - unique_files) / total_edits` |
//! | `file_edit_counts` | object | Map of file path to edit count |
//! | `high_churn_files` | array | Files edited 3+ times, sorted by count |
//!
//! ## Churn Ratio Interpretation
//!
//! - `0.0` = Each file edited exactly once (no churn)
//! - `0.5` = On average, each file edited twice
//! - `0.67` = On average, each file edited three times
//! - Higher values indicate more re-editing of the same files
//!
//! ## Example
//!
//! Session with these edits:
//! - `src/main.rs` (3 times)
//! - `src/lib.rs` (2 times)
//! - `Cargo.toml` (1 time)
//!
//! Results:
//! - `edit_count`: 6
//! - `unique_files`: 3
//! - `churn_ratio`: (6 - 3) / 6 = 0.5
//! - `high_churn_files`: `["src/main.rs"]`

use crate::analytics::engine::{AnalyticsContext, AnalyticsPlugin, AnalyticsTrigger, MetricOutput};
use crate::error::Result;
use crate::types::{Message, MessageType, Session};
use std::collections::HashMap;

/// Minimum edit count to be considered "high churn".
const HIGH_CHURN_THRESHOLD: i64 = 3;

/// Paths containing these patterns are excluded from churn analysis.
/// These are typically AI-generated planning docs, not user code.
const EXCLUDED_PATH_PATTERNS: &[&str] = &[
    "/.claude/plans/",   // Claude Code planning documents
    "/.claude/todos/",   // Claude Code todo files
    "/PLAN.md",          // Common AI planning file
    "/IMPLEMENTATION.md",
    "/DESIGN.md",
    "/ARCHITECTURE.md",
];

/// Analyzer that tracks file modification patterns.
pub struct EditChurnAnalyzer;

impl EditChurnAnalyzer {
    /// Create a new analyzer.
    pub fn new() -> Self {
        Self
    }

    /// Extract file path from a tool_input JSON value.
    ///
    /// Handles both `file_path` (Edit/Write) and `filePath` (some tools) keys.
    fn extract_file_path(tool_input: &serde_json::Value) -> Option<String> {
        tool_input
            .get("file_path")
            .or_else(|| tool_input.get("filePath"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Check if a file path should be excluded from churn analysis.
    ///
    /// Excludes AI planning documents and other non-code files that
    /// would skew the churn metrics.
    fn should_exclude_path(path: &str) -> bool {
        EXCLUDED_PATH_PATTERNS
            .iter()
            .any(|pattern| path.contains(pattern))
    }

    /// Check if a message is a file modification tool call.
    fn is_file_edit(message: &Message) -> bool {
        if message.message_type != MessageType::ToolCall {
            return false;
        }
        matches!(
            message.tool_name.as_deref(),
            Some("Edit") | Some("Write") | Some("MultiEdit") | Some("write") | Some("edit")
        )
    }

    /// Compute churn ratio from counts.
    ///
    /// Returns 0.0 if there are no edits.
    fn compute_churn_ratio(total_edits: i64, unique_files: i64) -> f64 {
        if total_edits == 0 {
            return 0.0;
        }
        (total_edits - unique_files) as f64 / total_edits as f64
    }
}

impl Default for EditChurnAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyticsPlugin for EditChurnAnalyzer {
    fn name(&self) -> &str {
        "core.edit_churn"
    }

    fn triggers(&self) -> Vec<AnalyticsTrigger> {
        vec![AnalyticsTrigger::OnDemand]
    }

    fn analyze_session(
        &self,
        session: &Session,
        messages: &[Message],
        _ctx: &AnalyticsContext,
    ) -> Result<Vec<MetricOutput>> {
        let mut file_counts: HashMap<String, i64> = HashMap::new();
        let mut total_edits = 0i64;

        // Count edits per file (excluding plan files and other non-code)
        for msg in messages {
            if !Self::is_file_edit(msg) {
                continue;
            }

            if let Some(ref tool_input) = msg.tool_input {
                if let Some(file_path) = Self::extract_file_path(tool_input) {
                    // Skip excluded paths (plan files, etc.)
                    if Self::should_exclude_path(&file_path) {
                        continue;
                    }
                    total_edits += 1;
                    *file_counts.entry(file_path).or_insert(0) += 1;
                }
            }
        }

        let unique_files = file_counts.len() as i64;
        let churn_ratio = Self::compute_churn_ratio(total_edits, unique_files);

        // Sort files by edit count (descending)
        let mut file_edit_list: Vec<(&String, &i64)> = file_counts.iter().collect();
        file_edit_list.sort_by(|a, b| b.1.cmp(a.1));

        // Build file_edit_counts as a JSON object
        let file_counts_json: serde_json::Value = file_edit_list
            .iter()
            .map(|(k, v)| ((*k).clone(), serde_json::json!(**v)))
            .collect();

        // Find high-churn files (edited 3+ times)
        let high_churn_files: Vec<&String> = file_edit_list
            .iter()
            .filter(|(_, count)| **count >= HIGH_CHURN_THRESHOLD)
            .map(|(path, _)| *path)
            .collect();

        Ok(vec![
            MetricOutput::session(&session.id, "edit_count", serde_json::json!(total_edits)),
            MetricOutput::session(&session.id, "unique_files", serde_json::json!(unique_files)),
            MetricOutput::session(
                &session.id,
                "churn_ratio",
                serde_json::json!(churn_ratio),
            ),
            MetricOutput::session(&session.id, "file_edit_counts", file_counts_json),
            MetricOutput::session(
                &session.id,
                "high_churn_files",
                serde_json::json!(high_churn_files),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Assistant, AuthorRole, SessionStatus};
    use chrono::Utc;

    #[allow(dead_code)]
    fn make_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            assistant: Assistant::ClaudeCode,
            backing_model_id: None,
            project_id: None,
            started_at: Utc::now(),
            last_activity_at: None,
            status: SessionStatus::Stale,
            source_file_path: "/test".to_string(),
            metadata: serde_json::json!({}),
        }
    }

    fn make_edit_message(file_path: &str, seq: i32) -> Message {
        Message {
            id: seq as i64,
            session_id: "test".to_string(),
            thread_id: "test-main".to_string(),
            seq,
            ts: Utc::now(),
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
            source_file_path: "/test".to_string(),
            source_offset: 0,
            source_line: None,
            raw_data: serde_json::json!({}),
            metadata: serde_json::json!({}),
        }
    }

    fn make_write_message(file_path: &str, seq: i32) -> Message {
        let mut msg = make_edit_message(file_path, seq);
        msg.tool_name = Some("Write".to_string());
        msg.author_name = Some("Write".to_string());
        msg.tool_input = Some(serde_json::json!({
            "file_path": file_path,
            "content": "new content"
        }));
        msg
    }

    fn make_prompt_message(seq: i32) -> Message {
        Message {
            id: seq as i64,
            session_id: "test".to_string(),
            thread_id: "test-main".to_string(),
            seq,
            ts: Utc::now(),
            author_role: AuthorRole::Human,
            author_name: None,
            message_type: MessageType::Prompt,
            content: Some("Do something".to_string()),
            content_type: None,
            tool_name: None,
            tool_input: None,
            tool_result: None,
            tokens_in: Some(10),
            tokens_out: None,
            duration_ms: None,
            source_file_path: "/test".to_string(),
            source_offset: 0,
            source_line: None,
            raw_data: serde_json::json!({}),
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn test_no_edits() {
        // Just prompts, no edits - none should be classified as file edits
        let messages = vec![make_prompt_message(1), make_prompt_message(2)];

        for msg in &messages {
            assert!(!EditChurnAnalyzer::is_file_edit(msg));
        }
    }

    #[test]
    fn test_is_file_edit() {
        let edit = make_edit_message("test.rs", 1);
        let write = make_write_message("test.rs", 2);
        let prompt = make_prompt_message(3);

        assert!(EditChurnAnalyzer::is_file_edit(&edit));
        assert!(EditChurnAnalyzer::is_file_edit(&write));
        assert!(!EditChurnAnalyzer::is_file_edit(&prompt));
    }

    #[test]
    fn test_extract_file_path() {
        let input1 = serde_json::json!({
            "file_path": "/path/to/file.rs"
        });
        assert_eq!(
            EditChurnAnalyzer::extract_file_path(&input1),
            Some("/path/to/file.rs".to_string())
        );

        let input2 = serde_json::json!({
            "filePath": "/another/path.rs"
        });
        assert_eq!(
            EditChurnAnalyzer::extract_file_path(&input2),
            Some("/another/path.rs".to_string())
        );

        let input3 = serde_json::json!({
            "content": "no path here"
        });
        assert_eq!(EditChurnAnalyzer::extract_file_path(&input3), None);
    }

    #[test]
    fn test_churn_ratio_calculation() {
        // No edits
        assert_eq!(EditChurnAnalyzer::compute_churn_ratio(0, 0), 0.0);

        // Each file edited once (no churn)
        assert_eq!(EditChurnAnalyzer::compute_churn_ratio(5, 5), 0.0);

        // 6 edits to 3 files = (6-3)/6 = 0.5
        let ratio = EditChurnAnalyzer::compute_churn_ratio(6, 3);
        assert!((ratio - 0.5).abs() < 0.001);

        // 10 edits to 2 files = (10-2)/10 = 0.8
        let ratio = EditChurnAnalyzer::compute_churn_ratio(10, 2);
        assert!((ratio - 0.8).abs() < 0.001);

        // 9 edits to 3 files = (9-3)/9 = 0.667
        let ratio = EditChurnAnalyzer::compute_churn_ratio(9, 3);
        assert!((ratio - 0.6667).abs() < 0.001);
    }

    #[test]
    fn test_high_churn_threshold() {
        // A file edited 2 times should NOT be high churn
        assert!(2 < HIGH_CHURN_THRESHOLD);

        // A file edited 3 times should be high churn
        assert!(3 >= HIGH_CHURN_THRESHOLD);
    }

    #[test]
    fn test_should_exclude_path() {
        // Claude plan files should be excluded
        assert!(EditChurnAnalyzer::should_exclude_path(
            "/Users/kulesh/.claude/plans/glimmering-church.md"
        ));
        assert!(EditChurnAnalyzer::should_exclude_path(
            "/home/user/.claude/plans/test-plan.md"
        ));
        assert!(EditChurnAnalyzer::should_exclude_path(
            "/Users/kulesh/.claude/todos/todo.md"
        ));

        // Common AI planning files should be excluded
        assert!(EditChurnAnalyzer::should_exclude_path("/project/PLAN.md"));
        assert!(EditChurnAnalyzer::should_exclude_path(
            "/project/IMPLEMENTATION.md"
        ));
        assert!(EditChurnAnalyzer::should_exclude_path("/project/DESIGN.md"));

        // Regular code files should NOT be excluded
        assert!(!EditChurnAnalyzer::should_exclude_path("/project/src/main.rs"));
        assert!(!EditChurnAnalyzer::should_exclude_path(
            "/project/Cargo.toml"
        ));
        assert!(!EditChurnAnalyzer::should_exclude_path(
            "/project/README.md"
        ));
        assert!(!EditChurnAnalyzer::should_exclude_path(
            "/project/docs/plan.md"
        )); // lowercase, not in .claude
    }
}
