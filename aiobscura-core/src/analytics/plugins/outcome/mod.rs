//! Prototype outcome tracking plugin.
//!
//! Captures coarse session outcomes in `plugin_metrics` until a first-class
//! outcome model is introduced.

use crate::analytics::engine::{AnalyticsContext, AnalyticsPlugin, AnalyticsTrigger, MetricOutput};
use crate::types::{Message, MessageType, Session};
use crate::Result;

pub struct OutcomeMetrics;

impl OutcomeMetrics {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OutcomeMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyticsPlugin for OutcomeMetrics {
    fn name(&self) -> &str {
        "core.outcome"
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
        let error_count = messages
            .iter()
            .filter(|m| matches!(m.message_type, MessageType::Error))
            .count();
        let tool_result_count = messages
            .iter()
            .filter(|m| matches!(m.message_type, MessageType::ToolResult))
            .count();

        let success = tool_result_count > 0 && error_count == 0;
        let evidence_type = if success {
            "tool_result_no_errors"
        } else if tool_result_count > 0 {
            "tool_result_with_errors"
        } else if error_count > 0 {
            "errors_only"
        } else {
            "insufficient_signal"
        };
        let notes = format!("tool_results={} errors={}", tool_result_count, error_count);

        Ok(vec![
            MetricOutput::session(&session.id, "outcome_success", success.into()),
            MetricOutput::session(&session.id, "outcome_evidence_type", evidence_type.into()),
            MetricOutput::session(&session.id, "outcome_notes", notes.into()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Assistant, AuthorRole, SessionStatus};
    use chrono::Utc;
    use serde_json::json;

    fn make_session() -> Session {
        let now = Utc::now();
        Session {
            id: "session-outcome".to_string(),
            assistant: Assistant::Codex,
            backing_model_id: None,
            project_id: None,
            started_at: now,
            last_activity_at: Some(now),
            status: SessionStatus::Active,
            source_file_path: "source.jsonl".to_string(),
            metadata: json!({}),
        }
    }

    fn make_message(id: i64, message_type: MessageType) -> Message {
        let now = Utc::now();
        Message {
            id,
            session_id: "session-outcome".to_string(),
            thread_id: "thread-1".to_string(),
            seq: id as i32,
            emitted_at: now,
            observed_at: now,
            author_role: AuthorRole::Assistant,
            author_name: None,
            message_type,
            content: None,
            content_type: None,
            tool_name: None,
            tool_input: None,
            tool_result: None,
            tokens_in: None,
            tokens_out: None,
            duration_ms: None,
            source_file_path: "source.jsonl".to_string(),
            source_offset: 0,
            source_line: None,
            raw_data: json!({}),
            metadata: json!({}),
        }
    }

    #[test]
    fn outcome_success_when_tool_results_without_errors() {
        let plugin = OutcomeMetrics::new();
        let session = make_session();
        let messages = vec![
            make_message(1, MessageType::ToolCall),
            make_message(2, MessageType::ToolResult),
        ];

        let db = crate::db::Database::open_in_memory().expect("db");
        db.migrate().expect("migrate");
        let ctx = AnalyticsContext { db: &db };

        let outputs = plugin
            .analyze_session(&session, &messages, &ctx)
            .expect("analysis should succeed");

        let mut values = std::collections::HashMap::new();
        for output in outputs {
            values.insert(output.metric_name, output.metric_value);
        }

        assert_eq!(
            values.get("outcome_success").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            values.get("outcome_evidence_type").and_then(|v| v.as_str()),
            Some("tool_result_no_errors")
        );
    }
}
