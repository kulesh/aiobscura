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
