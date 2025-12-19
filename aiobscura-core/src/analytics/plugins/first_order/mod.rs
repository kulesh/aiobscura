//! First-order session metrics plugin.

use crate::analytics::engine::{AnalyticsContext, AnalyticsPlugin, AnalyticsTrigger, MetricOutput};
use crate::types::{Message, MessageType, Session};
use crate::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub struct FirstOrderMetrics;

impl FirstOrderMetrics {
    pub fn new() -> Self {
        Self
    }

    fn compute_metrics(messages: &[Message]) -> FirstOrderSummary {
        let mut tokens_in: i64 = 0;
        let mut tokens_out: i64 = 0;
        let mut tool_call_count: i64 = 0;
        let mut tool_result_count: i64 = 0;
        let mut error_count: i64 = 0;
        let mut tool_breakdown: HashMap<String, i64> = HashMap::new();
        let mut min_ts: Option<DateTime<Utc>> = None;
        let mut max_ts: Option<DateTime<Utc>> = None;

        for msg in messages {
            tokens_in += msg.tokens_in.unwrap_or(0) as i64;
            tokens_out += msg.tokens_out.unwrap_or(0) as i64;

            match msg.message_type {
                MessageType::ToolCall => {
                    tool_call_count += 1;
                    let name = msg
                        .tool_name
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    *tool_breakdown.entry(name).or_insert(0) += 1;
                }
                MessageType::ToolResult => {
                    tool_result_count += 1;
                }
                MessageType::Error => {
                    error_count += 1;
                }
                _ => {}
            }

            let ts = msg.emitted_at;
            min_ts = Some(min_ts.map_or(ts, |current| current.min(ts)));
            max_ts = Some(max_ts.map_or(ts, |current| current.max(ts)));
        }

        let duration_ms = match (min_ts, max_ts) {
            (Some(start), Some(end)) => end.signed_duration_since(start).num_milliseconds(),
            _ => 0,
        };

        let tool_success_rate = if tool_call_count > 0 {
            tool_result_count as f64 / tool_call_count as f64
        } else {
            0.0
        };

        FirstOrderSummary {
            tokens_in,
            tokens_out,
            tokens_total: tokens_in + tokens_out,
            tool_call_count,
            tool_breakdown,
            error_count,
            duration_ms,
            tool_success_rate,
        }
    }
}

impl Default for FirstOrderMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyticsPlugin for FirstOrderMetrics {
    fn name(&self) -> &str {
        "core.first_order"
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
        let metrics = Self::compute_metrics(messages);

        Ok(vec![
            MetricOutput::session(&session.id, "tokens_in", metrics.tokens_in.into()),
            MetricOutput::session(&session.id, "tokens_out", metrics.tokens_out.into()),
            MetricOutput::session(&session.id, "tokens_total", metrics.tokens_total.into()),
            MetricOutput::session(
                &session.id,
                "tool_call_count",
                metrics.tool_call_count.into(),
            ),
            MetricOutput::session(
                &session.id,
                "tool_call_breakdown",
                serde_json::to_value(metrics.tool_breakdown)?,
            ),
            MetricOutput::session(&session.id, "error_count", metrics.error_count.into()),
            MetricOutput::session(&session.id, "duration_ms", metrics.duration_ms.into()),
            MetricOutput::session(
                &session.id,
                "tool_success_rate",
                metrics.tool_success_rate.into(),
            ),
        ])
    }
}

#[derive(Debug)]
struct FirstOrderSummary {
    tokens_in: i64,
    tokens_out: i64,
    tokens_total: i64,
    tool_call_count: i64,
    tool_breakdown: HashMap<String, i64>,
    error_count: i64,
    duration_ms: i64,
    tool_success_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::types::{Assistant, AuthorRole, SessionStatus};
    use chrono::{Duration, Utc};
    use serde_json::json;

    fn make_session() -> Session {
        let now = Utc::now();
        Session {
            id: "session-1".to_string(),
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

    fn make_message(
        seq: i32,
        message_type: MessageType,
        emitted_at: chrono::DateTime<Utc>,
        tokens_in: Option<i32>,
        tokens_out: Option<i32>,
        tool_name: Option<&str>,
    ) -> Message {
        Message {
            id: seq as i64,
            session_id: "session-1".to_string(),
            thread_id: "thread-1".to_string(),
            seq,
            emitted_at,
            observed_at: emitted_at,
            author_role: AuthorRole::Assistant,
            author_name: None,
            message_type,
            content: None,
            content_type: None,
            tool_name: tool_name.map(|name| name.to_string()),
            tool_input: None,
            tool_result: None,
            tokens_in,
            tokens_out,
            duration_ms: None,
            source_file_path: "source.jsonl".to_string(),
            source_offset: 0,
            source_line: None,
            raw_data: json!({}),
            metadata: json!({}),
        }
    }

    #[test]
    fn test_first_order_metrics_output() {
        let session = make_session();
        let start = Utc::now();
        let messages = vec![
            make_message(
                1,
                MessageType::ToolCall,
                start,
                Some(10),
                Some(5),
                Some("rg"),
            ),
            make_message(
                2,
                MessageType::ToolCall,
                start + Duration::seconds(1),
                Some(3),
                Some(7),
                Some("cat"),
            ),
            make_message(
                3,
                MessageType::ToolResult,
                start + Duration::seconds(2),
                None,
                None,
                None,
            ),
            make_message(
                4,
                MessageType::Error,
                start + Duration::seconds(2),
                None,
                None,
                None,
            ),
        ];

        let plugin = FirstOrderMetrics::new();
        let db = Database::open_in_memory().expect("db");
        db.migrate().expect("migrate");
        let ctx = AnalyticsContext { db: &db };
        let outputs = plugin
            .analyze_session(&session, &messages, &ctx)
            .expect("analysis succeeds");

        let mut values = std::collections::HashMap::new();
        for output in outputs {
            assert_eq!(output.entity_type, "session");
            values.insert(output.metric_name, output.metric_value);
        }

        assert_eq!(values.get("tokens_in").and_then(|v| v.as_i64()), Some(13));
        assert_eq!(values.get("tokens_out").and_then(|v| v.as_i64()), Some(12));
        assert_eq!(
            values.get("tokens_total").and_then(|v| v.as_i64()),
            Some(25)
        );
        assert_eq!(
            values.get("tool_call_count").and_then(|v| v.as_i64()),
            Some(2)
        );
        assert_eq!(values.get("error_count").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(
            values.get("duration_ms").and_then(|v| v.as_i64()),
            Some(2000)
        );
        assert_eq!(
            values
                .get("tool_success_rate")
                .and_then(|v| v.as_f64())
                .unwrap_or_default(),
            0.5
        );
        let breakdown = values
            .get("tool_call_breakdown")
            .and_then(|v| v.as_object())
            .expect("breakdown map");
        assert_eq!(breakdown.get("rg").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(breakdown.get("cat").and_then(|v| v.as_i64()), Some(1));
    }
}
