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

const ALL_METRICS: &[MetricDescriptor] = FIRST_ORDER_METRICS;

/// List all registered metrics.
pub fn list_metrics() -> Vec<MetricDescriptor> {
    ALL_METRICS.to_vec()
}

/// List metrics for a given plugin name.
pub fn list_metrics_for_plugin(plugin: &str) -> Vec<MetricDescriptor> {
    ALL_METRICS
        .iter()
        .filter(|m| m.plugin == plugin)
        .cloned()
        .collect()
}

/// List metrics for a given entity type.
pub fn list_metrics_for_entity(entity_type: &str) -> Vec<MetricDescriptor> {
    ALL_METRICS
        .iter()
        .filter(|m| m.entity_type == entity_type)
        .cloned()
        .collect()
}
