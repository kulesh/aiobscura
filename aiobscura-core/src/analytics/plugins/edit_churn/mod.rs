//! Edit Churn Analyzer
//!
//! Tracks file modification patterns in AI coding sessions using a two-pronged approach:
//!
//! 1. **Statistical Outliers** - Files with significantly more edits than the session average
//! 2. **Burst Detection** - Files with rapid consecutive edits (debugging loops)
//!
//! See `docs/edit-churn-algorithm.md` for detailed algorithm documentation.
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
//! | `high_churn_files` | array | Statistical outliers (dynamic threshold) |
//! | `high_churn_threshold` | float | Computed threshold for this session |
//! | `burst_edit_files` | object | Files with burst patterns: {path: count} |
//! | `burst_edit_count` | integer | Total burst incidents detected |
//! | `lines_added` | integer | Total lines added across all edits |
//! | `lines_removed` | integer | Total lines removed across all edits |
//! | `lines_changed` | integer | Total lines changed (added + removed) |
//! | `edits_by_extension` | object | Map of file extension to edit count |
//! | `first_try_files` | integer | Files edited exactly once (no rework) |
//! | `first_try_rate` | float | Percentage of files edited exactly once |
//!
//! ## High Churn Detection
//!
//! Uses dynamic threshold: `max(3, median + 2*stddev)` to identify statistical outliers.
//! Falls back to threshold=3 for sessions with fewer than 5 files.
//!
//! ## Burst Detection
//!
//! A "burst" is 3+ edits to the same file within 2 minutes.
//! Indicates debugging loops or trial-and-error fixing.
//!
//! ## Example
//!
//! Session with these edits:
//! - `src/main.rs` (15 times, 3 bursts)
//! - `src/lib.rs` (2 times)
//! - `Cargo.toml` (1 time)
//!
//! Results:
//! - `edit_count`: 18
//! - `unique_files`: 3
//! - `high_churn_threshold`: ~10 (computed from session stats)
//! - `high_churn_files`: `["src/main.rs"]` (15 >= 10)
//! - `burst_edit_files`: `{"src/main.rs": 3}`
//! - `burst_edit_count`: 3

use crate::analytics::engine::{AnalyticsContext, AnalyticsPlugin, AnalyticsTrigger, MetricOutput};
use crate::error::Result;
use crate::types::{Message, MessageType, Session, Thread};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Minimum edit count to be considered "high churn" (absolute floor).
const HIGH_CHURN_THRESHOLD: i64 = 3;

/// Window for burst detection (2 minutes in seconds).
const BURST_WINDOW_SECONDS: i64 = 120;

/// Minimum files needed for statistical threshold calculation.
/// Below this, we fall back to the fixed HIGH_CHURN_THRESHOLD.
const MIN_FILES_FOR_STATS: usize = 5;

/// Standard deviation multiplier for outlier detection.
/// 2.0 is more conservative than 1.5, reducing false positives.
const OUTLIER_STDDEV_MULTIPLIER: f64 = 2.0;

/// Paths containing these patterns are excluded from churn analysis.
/// These are typically AI-generated planning docs, not user code.
const EXCLUDED_PATH_PATTERNS: &[&str] = &[
    "/.claude/plans/", // Claude Code planning documents
    "/.claude/todos/", // Claude Code todo files
    "/PLAN.md",        // Common AI planning file
    "/IMPLEMENTATION.md",
    "/DESIGN.md",
    "/ARCHITECTURE.md",
];

/// Computed churn metrics for a set of messages.
///
/// This intermediate struct holds all the computed values before they're
/// converted to MetricOutput. Used by both session and thread analysis.
#[derive(Debug)]
struct ChurnMetrics {
    total_edits: i64,
    unique_files: i64,
    churn_ratio: f64,
    file_counts: HashMap<String, i64>,
    high_churn_files: Vec<String>,
    high_churn_threshold: f64,
    burst_edit_files: HashMap<String, i64>,
    burst_edit_count: i64,
    lines_added: i64,
    lines_removed: i64,
    extension_counts: HashMap<String, i64>,
    first_try_files: i64,
    first_try_rate: f64,
}

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

    /// Extract line change counts from a tool_input.
    ///
    /// For Edit tool: compares old_string vs new_string line counts.
    /// For Write tool: counts lines in content (all additions).
    ///
    /// Returns (lines_added, lines_removed).
    fn extract_line_changes(tool_name: Option<&str>, tool_input: &serde_json::Value) -> (i64, i64) {
        match tool_name {
            Some("Edit") | Some("edit") => {
                let old_lines = tool_input
                    .get("old_string")
                    .and_then(|v| v.as_str())
                    .map(|s| s.lines().count())
                    .unwrap_or(0) as i64;
                let new_lines = tool_input
                    .get("new_string")
                    .and_then(|v| v.as_str())
                    .map(|s| s.lines().count())
                    .unwrap_or(0) as i64;

                // Simple heuristic: if new > old, added; if old > new, removed
                let added = (new_lines - old_lines).max(0);
                let removed = (old_lines - new_lines).max(0);
                (added, removed)
            }
            Some("Write") | Some("write") => {
                // Write is all additions
                let lines = tool_input
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.lines().count())
                    .unwrap_or(0) as i64;
                (lines, 0)
            }
            Some("MultiEdit") => {
                // MultiEdit has an array of edits
                let edits = tool_input
                    .get("edits")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.len())
                    .unwrap_or(0) as i64;
                // Approximate: assume 5 lines per edit on average
                (edits * 5, edits * 3)
            }
            _ => (0, 0),
        }
    }

    /// Extract file extension from a path.
    ///
    /// Returns the extension without the dot, or "no_ext" for files without extension.
    fn extract_extension(path: &str) -> &str {
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("no_ext")
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

    /// Compute median of a slice of i64 values.
    fn compute_median(counts: &[i64]) -> f64 {
        if counts.is_empty() {
            return 0.0;
        }
        let mut sorted = counts.to_vec();
        sorted.sort();
        let mid = sorted.len() / 2;
        if sorted.len().is_multiple_of(2) {
            (sorted[mid - 1] + sorted[mid]) as f64 / 2.0
        } else {
            sorted[mid] as f64
        }
    }

    /// Compute standard deviation of a slice of i64 values.
    fn compute_stddev(counts: &[i64], mean: f64) -> f64 {
        if counts.is_empty() {
            return 0.0;
        }
        let variance: f64 = counts
            .iter()
            .map(|&x| {
                let diff = x as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / counts.len() as f64;
        variance.sqrt()
    }

    /// Compute dynamic threshold for high churn based on session statistics.
    ///
    /// Uses median + 2*stddev for sessions with 5+ files, otherwise falls back
    /// to the fixed HIGH_CHURN_THRESHOLD.
    fn compute_dynamic_threshold(counts: &[i64]) -> f64 {
        if counts.len() < MIN_FILES_FOR_STATS {
            return HIGH_CHURN_THRESHOLD as f64;
        }

        let median = Self::compute_median(counts);
        let mean: f64 = counts.iter().sum::<i64>() as f64 / counts.len() as f64;
        let stddev = Self::compute_stddev(counts, mean);

        // threshold = max(3, median + 2*stddev)
        (median + OUTLIER_STDDEV_MULTIPLIER * stddev).max(HIGH_CHURN_THRESHOLD as f64)
    }

    /// Detect burst editing patterns in file timestamps.
    ///
    /// A "burst" is 3+ edits to the same file within BURST_WINDOW_SECONDS.
    /// Returns a map of file path to number of burst incidents detected.
    fn detect_burst_edits(
        file_timestamps: &HashMap<String, Vec<DateTime<Utc>>>,
    ) -> HashMap<String, i64> {
        let mut burst_files: HashMap<String, i64> = HashMap::new();

        for (file_path, timestamps) in file_timestamps {
            // Need at least 3 edits to have a burst
            if timestamps.len() < 3 {
                continue;
            }

            // Sort timestamps
            let mut sorted_ts = timestamps.clone();
            sorted_ts.sort();

            // Sliding window: check if any 3 consecutive edits are within the burst window
            let mut burst_count = 0i64;
            for i in 0..sorted_ts.len() - 2 {
                let window_start = sorted_ts[i];
                let window_end = sorted_ts[i + 2]; // Third edit in potential burst

                let duration_secs = (window_end - window_start).num_seconds();
                if duration_secs <= BURST_WINDOW_SECONDS {
                    burst_count += 1;
                }
            }

            if burst_count > 0 {
                burst_files.insert(file_path.clone(), burst_count);
            }
        }

        burst_files
    }

    /// Compute churn metrics from a set of messages.
    ///
    /// This is the core analysis logic used by both session and thread analysis.
    fn compute_metrics(messages: &[Message]) -> ChurnMetrics {
        let mut file_counts: HashMap<String, i64> = HashMap::new();
        let mut file_timestamps: HashMap<String, Vec<DateTime<Utc>>> = HashMap::new();
        let mut extension_counts: HashMap<String, i64> = HashMap::new();
        let mut total_edits = 0i64;
        let mut total_lines_added = 0i64;
        let mut total_lines_removed = 0i64;

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
                    *file_counts.entry(file_path.clone()).or_insert(0) += 1;

                    // Track timestamps for burst detection
                    file_timestamps
                        .entry(file_path.clone())
                        .or_default()
                        .push(msg.ts);

                    // Track by file extension
                    let ext = Self::extract_extension(&file_path).to_string();
                    *extension_counts.entry(ext).or_insert(0) += 1;

                    // Track line changes
                    let (added, removed) =
                        Self::extract_line_changes(msg.tool_name.as_deref(), tool_input);
                    total_lines_added += added;
                    total_lines_removed += removed;
                }
            }
        }

        let unique_files = file_counts.len() as i64;
        let churn_ratio = Self::compute_churn_ratio(total_edits, unique_files);

        // Sort files by edit count (descending)
        let mut file_edit_list: Vec<(&String, &i64)> = file_counts.iter().collect();
        file_edit_list.sort_by(|a, b| b.1.cmp(a.1));

        // Compute dynamic threshold for high churn (statistical outliers)
        let counts: Vec<i64> = file_counts.values().copied().collect();
        let high_churn_threshold = Self::compute_dynamic_threshold(&counts);

        // Find high-churn files (statistical outliers)
        let high_churn_files: Vec<String> = file_edit_list
            .iter()
            .filter(|(_, count)| **count as f64 >= high_churn_threshold)
            .map(|(path, _)| (*path).clone())
            .collect();

        // Detect burst editing patterns
        let burst_edit_files = Self::detect_burst_edits(&file_timestamps);
        let burst_edit_count: i64 = burst_edit_files.values().sum();

        // Calculate first-try rate (files edited exactly once)
        let first_try_files = file_counts.values().filter(|&&count| count == 1).count() as i64;
        let first_try_rate = if unique_files > 0 {
            first_try_files as f64 / unique_files as f64
        } else {
            0.0
        };

        ChurnMetrics {
            total_edits,
            unique_files,
            churn_ratio,
            file_counts,
            high_churn_files,
            high_churn_threshold,
            burst_edit_files,
            burst_edit_count,
            lines_added: total_lines_added,
            lines_removed: total_lines_removed,
            extension_counts,
            first_try_files,
            first_try_rate,
        }
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
        let m = Self::compute_metrics(messages);

        // Build file_edit_counts as a JSON object (sorted by count descending)
        let mut file_edit_list: Vec<(&String, &i64)> = m.file_counts.iter().collect();
        file_edit_list.sort_by(|a, b| b.1.cmp(a.1));
        let file_counts_json: serde_json::Value = file_edit_list
            .iter()
            .map(|(k, v)| ((*k).clone(), serde_json::json!(**v)))
            .collect();

        // Build burst_edit_files as JSON object
        let burst_files_json: serde_json::Value = m
            .burst_edit_files
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::json!(*v)))
            .collect();

        // Build extension counts as JSON object (sorted by count)
        let mut ext_list: Vec<(&String, &i64)> = m.extension_counts.iter().collect();
        ext_list.sort_by(|a, b| b.1.cmp(a.1));
        let ext_counts_json: serde_json::Value = ext_list
            .iter()
            .map(|(k, v)| ((*k).clone(), serde_json::json!(**v)))
            .collect();

        Ok(vec![
            MetricOutput::session(&session.id, "edit_count", serde_json::json!(m.total_edits)),
            MetricOutput::session(
                &session.id,
                "unique_files",
                serde_json::json!(m.unique_files),
            ),
            MetricOutput::session(&session.id, "churn_ratio", serde_json::json!(m.churn_ratio)),
            MetricOutput::session(&session.id, "file_edit_counts", file_counts_json),
            MetricOutput::session(
                &session.id,
                "high_churn_files",
                serde_json::json!(m.high_churn_files),
            ),
            MetricOutput::session(
                &session.id,
                "high_churn_threshold",
                serde_json::json!(m.high_churn_threshold),
            ),
            MetricOutput::session(&session.id, "burst_edit_files", burst_files_json),
            MetricOutput::session(
                &session.id,
                "burst_edit_count",
                serde_json::json!(m.burst_edit_count),
            ),
            MetricOutput::session(&session.id, "lines_added", serde_json::json!(m.lines_added)),
            MetricOutput::session(
                &session.id,
                "lines_removed",
                serde_json::json!(m.lines_removed),
            ),
            MetricOutput::session(
                &session.id,
                "lines_changed",
                serde_json::json!(m.lines_added + m.lines_removed),
            ),
            MetricOutput::session(&session.id, "edits_by_extension", ext_counts_json),
            MetricOutput::session(
                &session.id,
                "first_try_files",
                serde_json::json!(m.first_try_files),
            ),
            MetricOutput::session(
                &session.id,
                "first_try_rate",
                serde_json::json!(m.first_try_rate),
            ),
        ])
    }

    fn supports_thread_analysis(&self) -> bool {
        true
    }

    fn analyze_thread(
        &self,
        thread: &Thread,
        messages: &[Message],
        _ctx: &AnalyticsContext,
    ) -> Result<Vec<MetricOutput>> {
        let m = Self::compute_metrics(messages);

        // Build file_edit_counts as a JSON object (sorted by count descending)
        let mut file_edit_list: Vec<(&String, &i64)> = m.file_counts.iter().collect();
        file_edit_list.sort_by(|a, b| b.1.cmp(a.1));
        let file_counts_json: serde_json::Value = file_edit_list
            .iter()
            .map(|(k, v)| ((*k).clone(), serde_json::json!(**v)))
            .collect();

        // Build burst_edit_files as JSON object
        let burst_files_json: serde_json::Value = m
            .burst_edit_files
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::json!(*v)))
            .collect();

        // Build extension counts as JSON object (sorted by count)
        let mut ext_list: Vec<(&String, &i64)> = m.extension_counts.iter().collect();
        ext_list.sort_by(|a, b| b.1.cmp(a.1));
        let ext_counts_json: serde_json::Value = ext_list
            .iter()
            .map(|(k, v)| ((*k).clone(), serde_json::json!(**v)))
            .collect();

        Ok(vec![
            MetricOutput::thread(&thread.id, "edit_count", serde_json::json!(m.total_edits)),
            MetricOutput::thread(
                &thread.id,
                "unique_files",
                serde_json::json!(m.unique_files),
            ),
            MetricOutput::thread(&thread.id, "churn_ratio", serde_json::json!(m.churn_ratio)),
            MetricOutput::thread(&thread.id, "file_edit_counts", file_counts_json),
            MetricOutput::thread(
                &thread.id,
                "high_churn_files",
                serde_json::json!(m.high_churn_files),
            ),
            MetricOutput::thread(
                &thread.id,
                "high_churn_threshold",
                serde_json::json!(m.high_churn_threshold),
            ),
            MetricOutput::thread(&thread.id, "burst_edit_files", burst_files_json),
            MetricOutput::thread(
                &thread.id,
                "burst_edit_count",
                serde_json::json!(m.burst_edit_count),
            ),
            MetricOutput::thread(&thread.id, "lines_added", serde_json::json!(m.lines_added)),
            MetricOutput::thread(
                &thread.id,
                "lines_removed",
                serde_json::json!(m.lines_removed),
            ),
            MetricOutput::thread(
                &thread.id,
                "lines_changed",
                serde_json::json!(m.lines_added + m.lines_removed),
            ),
            MetricOutput::thread(&thread.id, "edits_by_extension", ext_counts_json),
            MetricOutput::thread(
                &thread.id,
                "first_try_files",
                serde_json::json!(m.first_try_files),
            ),
            MetricOutput::thread(
                &thread.id,
                "first_try_rate",
                serde_json::json!(m.first_try_rate),
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
        // Verify the constant is what we expect (3 edits minimum)
        assert_eq!(HIGH_CHURN_THRESHOLD, 3);
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
        assert!(!EditChurnAnalyzer::should_exclude_path(
            "/project/src/main.rs"
        ));
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

    #[test]
    fn test_extract_line_changes_edit() {
        // Edit: 2 lines -> 5 lines = 3 added, 0 removed
        let input = serde_json::json!({
            "old_string": "line1\nline2",
            "new_string": "line1\nline2\nline3\nline4\nline5"
        });
        let (added, removed) = EditChurnAnalyzer::extract_line_changes(Some("Edit"), &input);
        assert_eq!(added, 3);
        assert_eq!(removed, 0);

        // Edit: 5 lines -> 2 lines = 0 added, 3 removed
        let input = serde_json::json!({
            "old_string": "line1\nline2\nline3\nline4\nline5",
            "new_string": "line1\nline2"
        });
        let (added, removed) = EditChurnAnalyzer::extract_line_changes(Some("Edit"), &input);
        assert_eq!(added, 0);
        assert_eq!(removed, 3);
    }

    #[test]
    fn test_extract_line_changes_write() {
        // Write: all new content = all additions
        let input = serde_json::json!({
            "content": "line1\nline2\nline3"
        });
        let (added, removed) = EditChurnAnalyzer::extract_line_changes(Some("Write"), &input);
        assert_eq!(added, 3);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_extract_line_changes_unknown() {
        // Unknown tool: no changes tracked
        let input = serde_json::json!({});
        let (added, removed) = EditChurnAnalyzer::extract_line_changes(Some("Read"), &input);
        assert_eq!(added, 0);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_extract_extension() {
        assert_eq!(
            EditChurnAnalyzer::extract_extension("/path/to/file.rs"),
            "rs"
        );
        assert_eq!(
            EditChurnAnalyzer::extract_extension("/path/to/file.test.ts"),
            "ts"
        );
        assert_eq!(EditChurnAnalyzer::extract_extension("Cargo.toml"), "toml");
        assert_eq!(
            EditChurnAnalyzer::extract_extension("/path/Makefile"),
            "no_ext"
        );
        assert_eq!(EditChurnAnalyzer::extract_extension(".gitignore"), "no_ext");
    }

    #[test]
    fn test_first_try_rate_concept() {
        // If we have 10 files:
        // - 7 edited exactly once (first try success)
        // - 3 edited multiple times (required rework)
        // first_try_rate = 7/10 = 0.7 = 70%
        //
        // Higher is better - means less rework needed

        let first_try_files = 7i64;
        let unique_files = 10i64;
        let rate = first_try_files as f64 / unique_files as f64;
        assert!((rate - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_compute_median() {
        // Odd number of elements
        assert_eq!(EditChurnAnalyzer::compute_median(&[1, 2, 3, 4, 5]), 3.0);

        // Even number of elements
        assert_eq!(EditChurnAnalyzer::compute_median(&[1, 2, 3, 4]), 2.5);

        // Single element
        assert_eq!(EditChurnAnalyzer::compute_median(&[7]), 7.0);

        // Empty
        assert_eq!(EditChurnAnalyzer::compute_median(&[]), 0.0);

        // Unsorted input (should still work)
        assert_eq!(EditChurnAnalyzer::compute_median(&[5, 1, 3, 2, 4]), 3.0);
    }

    #[test]
    fn test_compute_stddev() {
        // [1, 2, 3, 4, 5] mean=3, variance=2, stddev=sqrt(2)≈1.414
        let stddev = EditChurnAnalyzer::compute_stddev(&[1, 2, 3, 4, 5], 3.0);
        assert!((stddev - 1.414).abs() < 0.01);

        // All same values = 0 stddev
        let stddev = EditChurnAnalyzer::compute_stddev(&[5, 5, 5, 5], 5.0);
        assert_eq!(stddev, 0.0);

        // Empty = 0
        assert_eq!(EditChurnAnalyzer::compute_stddev(&[], 0.0), 0.0);
    }

    #[test]
    fn test_compute_dynamic_threshold() {
        // Small sample (<5 files) falls back to fixed threshold
        assert_eq!(
            EditChurnAnalyzer::compute_dynamic_threshold(&[1, 2, 3, 4]),
            3.0
        );

        // Low variance session: [1, 1, 1, 1, 2]
        // median=1, mean=1.2, stddev≈0.4, threshold=max(3, 1+0.8)=3
        let threshold = EditChurnAnalyzer::compute_dynamic_threshold(&[1, 1, 1, 1, 2]);
        assert!((threshold - 3.0).abs() < 0.1);

        // High variance with outlier: [1, 1, 1, 1, 15]
        // median=1, mean=3.8, stddev≈5.5, threshold=max(3, 1+11)=12
        let threshold = EditChurnAnalyzer::compute_dynamic_threshold(&[1, 1, 1, 1, 15]);
        assert!(threshold > 10.0); // Should be high due to outlier

        // Normal distribution: [5, 6, 7, 8, 9]
        // median=7, mean=7, stddev≈1.4, threshold=max(3, 7+2.8)≈9.8
        let threshold = EditChurnAnalyzer::compute_dynamic_threshold(&[5, 6, 7, 8, 9]);
        assert!(threshold > 9.0 && threshold < 11.0);
    }

    #[test]
    fn test_detect_burst_edits() {
        use chrono::Duration;

        let base_time = Utc::now();

        // File with burst: 3 edits within 30 seconds
        let mut file_timestamps: HashMap<String, Vec<DateTime<Utc>>> = HashMap::new();
        file_timestamps.insert(
            "burst_file.rs".to_string(),
            vec![
                base_time,
                base_time + Duration::seconds(10),
                base_time + Duration::seconds(25), // 25s from start = burst
            ],
        );

        let bursts = EditChurnAnalyzer::detect_burst_edits(&file_timestamps);
        assert_eq!(bursts.get("burst_file.rs"), Some(&1));

        // File without burst: edits spread out
        let mut file_timestamps2: HashMap<String, Vec<DateTime<Utc>>> = HashMap::new();
        file_timestamps2.insert(
            "spread_file.rs".to_string(),
            vec![
                base_time,
                base_time + Duration::minutes(30),
                base_time + Duration::minutes(60),
            ],
        );

        let bursts2 = EditChurnAnalyzer::detect_burst_edits(&file_timestamps2);
        assert!(bursts2.is_empty());

        // File with only 2 edits: can't have a burst
        let mut file_timestamps3: HashMap<String, Vec<DateTime<Utc>>> = HashMap::new();
        file_timestamps3.insert(
            "two_edits.rs".to_string(),
            vec![base_time, base_time + Duration::seconds(5)],
        );

        let bursts3 = EditChurnAnalyzer::detect_burst_edits(&file_timestamps3);
        assert!(bursts3.is_empty());
    }

    #[test]
    fn test_detect_multiple_bursts() {
        use chrono::Duration;

        let base_time = Utc::now();

        // File with 2 separate burst incidents
        let mut file_timestamps: HashMap<String, Vec<DateTime<Utc>>> = HashMap::new();
        file_timestamps.insert(
            "multi_burst.rs".to_string(),
            vec![
                // First burst: edits at 0, 10, 20 seconds
                base_time,
                base_time + Duration::seconds(10),
                base_time + Duration::seconds(20),
                // Gap
                base_time + Duration::minutes(30),
                // Second burst: edits at 30:00, 30:10, 30:20
                base_time + Duration::minutes(30) + Duration::seconds(10),
                base_time + Duration::minutes(30) + Duration::seconds(20),
            ],
        );

        let bursts = EditChurnAnalyzer::detect_burst_edits(&file_timestamps);
        // Should detect 2 burst incidents (sliding window finds overlapping bursts)
        assert!(bursts.get("multi_burst.rs").unwrap() >= &2);
    }
}
