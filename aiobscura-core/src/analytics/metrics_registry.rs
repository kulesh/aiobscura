//! Metrics registry for discovery and documentation.

/// Type of metric value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricValueType {
    Integer,
    Float,
    Boolean,
    Text,
    Json,
}

impl MetricValueType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricValueType::Integer => "integer",
            MetricValueType::Float => "float",
            MetricValueType::Boolean => "boolean",
            MetricValueType::Text => "text",
            MetricValueType::Json => "json",
        }
    }
}

/// Descriptor for a metric produced by analytics plugins.
#[derive(Debug, Clone)]
pub struct MetricDescriptor {
    pub plugin: &'static str,
    pub entity_type: &'static str,
    pub name: &'static str,
    pub value_type: MetricValueType,
    pub summary: &'static str,
    pub description: &'static str,
}

/// Ranked search result for metric discovery.
#[derive(Debug, Clone)]
pub struct MetricSearchResult {
    pub metric: MetricDescriptor,
    pub score: f64,
}

const FIRST_ORDER_METRICS: &[MetricDescriptor] = &[
    MetricDescriptor {
        plugin: "core.first_order",
        entity_type: "session",
        name: "tokens_in",
        value_type: MetricValueType::Integer,
        summary: "Total input tokens for the session.",
        description: "Sum of input tokens across all messages in the session.",
    },
    MetricDescriptor {
        plugin: "core.first_order",
        entity_type: "session",
        name: "tokens_out",
        value_type: MetricValueType::Integer,
        summary: "Total output tokens for the session.",
        description: "Sum of output tokens across all messages in the session.",
    },
    MetricDescriptor {
        plugin: "core.first_order",
        entity_type: "session",
        name: "tokens_total",
        value_type: MetricValueType::Integer,
        summary: "Total tokens for the session.",
        description: "Sum of input and output tokens across the session.",
    },
    MetricDescriptor {
        plugin: "core.first_order",
        entity_type: "session",
        name: "tool_call_count",
        value_type: MetricValueType::Integer,
        summary: "Total tool calls in the session.",
        description: "Count of tool_call messages in the session.",
    },
    MetricDescriptor {
        plugin: "core.first_order",
        entity_type: "session",
        name: "tool_call_breakdown",
        value_type: MetricValueType::Json,
        summary: "Tool call counts by tool name.",
        description: "JSON object mapping tool name to call count.",
    },
    MetricDescriptor {
        plugin: "core.first_order",
        entity_type: "session",
        name: "error_count",
        value_type: MetricValueType::Integer,
        summary: "Total errors in the session.",
        description: "Count of messages classified as error events.",
    },
    MetricDescriptor {
        plugin: "core.first_order",
        entity_type: "session",
        name: "duration_ms",
        value_type: MetricValueType::Integer,
        summary: "Session duration in milliseconds.",
        description: "Elapsed time between first and last message in the session.",
    },
    MetricDescriptor {
        plugin: "core.first_order",
        entity_type: "session",
        name: "tool_success_rate",
        value_type: MetricValueType::Float,
        summary: "Tool success rate for the session.",
        description: "Ratio of successful tool calls to total tool calls.",
    },
];

const OUTCOME_METRICS: &[MetricDescriptor] = &[
    MetricDescriptor {
        plugin: "core.outcome",
        entity_type: "session",
        name: "outcome_success",
        value_type: MetricValueType::Boolean,
        summary: "Prototype boolean outcome for the session.",
        description: "True when tool results were observed without error messages.",
    },
    MetricDescriptor {
        plugin: "core.outcome",
        entity_type: "session",
        name: "outcome_evidence_type",
        value_type: MetricValueType::Text,
        summary: "Reason code supporting the prototype outcome.",
        description: "Categorical evidence (e.g. tool_result_no_errors, errors_only).",
    },
    MetricDescriptor {
        plugin: "core.outcome",
        entity_type: "session",
        name: "outcome_notes",
        value_type: MetricValueType::Text,
        summary: "Human-readable notes for the outcome prototype.",
        description: "Debug note with observed signal counts used in outcome heuristics.",
    },
];

fn all_metrics_iter() -> impl Iterator<Item = &'static MetricDescriptor> {
    FIRST_ORDER_METRICS.iter().chain(OUTCOME_METRICS.iter())
}

/// List all registered metrics.
pub fn list_metrics() -> Vec<MetricDescriptor> {
    all_metrics_iter().cloned().collect()
}

/// List metrics for a given plugin name.
pub fn list_metrics_for_plugin(plugin: &str) -> Vec<MetricDescriptor> {
    all_metrics_iter()
        .filter(|m| m.plugin == plugin)
        .cloned()
        .collect()
}

/// List metrics for a given entity type.
pub fn list_metrics_for_entity(entity_type: &str) -> Vec<MetricDescriptor> {
    all_metrics_iter()
        .filter(|m| m.entity_type == entity_type)
        .cloned()
        .collect()
}

/// Search metrics using a fallback string matcher.
///
/// This is a deterministic, dependency-free fallback when no semantic scorer
/// is available. For semantic search, use `search_metrics_with_scoring`.
pub fn search_metrics(query: &str) -> Vec<MetricSearchResult> {
    search_metrics_with_scoring(query, fallback_score)
}

/// Search metrics using a caller-provided semantic scorer.
///
/// The scorer should return `Some(score)` for matches, or `None` to skip.
pub fn search_metrics_with_scoring<F>(query: &str, scorer: F) -> Vec<MetricSearchResult>
where
    F: Fn(&MetricDescriptor, &str) -> Option<f64>,
{
    let mut results: Vec<MetricSearchResult> = all_metrics_iter()
        .filter_map(|metric| {
            scorer(metric, query).map(|score| MetricSearchResult {
                metric: metric.clone(),
                score,
            })
        })
        .collect();

    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    results
}

fn fallback_score(metric: &MetricDescriptor, query: &str) -> Option<f64> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }

    let name = metric.name.to_lowercase();
    let summary = metric.summary.to_lowercase();
    let description = metric.description.to_lowercase();
    let mut score = 0.0;

    if name.contains(&query) {
        score += 3.0;
    }
    if summary.contains(&query) {
        score += 2.0;
    }
    if description.contains(&query) {
        score += 1.0;
    }

    if score > 0.0 {
        Some(score)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_metrics_for_plugin() {
        let metrics = list_metrics_for_plugin("core.first_order");
        assert_eq!(metrics.len(), 8);
        assert!(metrics.iter().any(|m| m.name == "tokens_in"));

        let outcome_metrics = list_metrics_for_plugin("core.outcome");
        assert_eq!(outcome_metrics.len(), 3);
        assert!(outcome_metrics.iter().any(|m| m.name == "outcome_success"));
    }

    #[test]
    fn test_list_metrics_for_entity() {
        let metrics = list_metrics_for_entity("session");
        assert!(metrics.iter().any(|m| m.name == "tool_call_breakdown"));
    }

    #[test]
    fn test_search_metrics_matches_tokens() {
        let results = search_metrics("token");
        let names: Vec<_> = results.iter().map(|r| r.metric.name).collect();
        assert!(names.contains(&"tokens_in"));
        assert!(names.contains(&"tokens_out"));
        assert!(names.contains(&"tokens_total"));
    }
}
