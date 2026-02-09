//! Shared message formatting helpers for TUI rendering.

use aiobscura_core::{AuthorRole, Message, MessageType, MessageWithContext};
use ratatui::style::{Color, Style};

/// Role prefix and style for session detail message rows.
pub fn session_role_prefix(msg: &Message) -> (&'static str, Style) {
    match msg.author_role {
        AuthorRole::Human => ("[human]", Style::default().fg(Color::Green)),
        AuthorRole::Caller => ("[caller]", Style::default().fg(Color::Cyan)),
        AuthorRole::Assistant => ("[assistant]", Style::default().fg(Color::Blue)),
        AuthorRole::Agent => ("[agent]", Style::default().fg(Color::Cyan)),
        AuthorRole::Tool => ("[tool]", Style::default().fg(Color::Magenta)),
        AuthorRole::System => {
            // Snapshot events are a distinct system subtype worth labeling.
            if msg.author_name.as_deref() == Some("snapshot") {
                ("[snapshot]", Style::default().fg(Color::DarkGray))
            } else {
                ("[system]", Style::default().fg(Color::DarkGray))
            }
        }
    }
}

/// Role label and style for live stream rows.
pub fn live_role_label(role: AuthorRole) -> (&'static str, Style) {
    match role {
        AuthorRole::Human => ("human", Style::default().fg(Color::Cyan)),
        AuthorRole::Caller => ("caller", Style::default().fg(Color::Cyan)),
        AuthorRole::Assistant => ("assistant", Style::default().fg(Color::Green)),
        AuthorRole::Tool => ("tool", Style::default().fg(Color::Yellow)),
        AuthorRole::Agent => ("agent", Style::default().fg(Color::Rgb(220, 180, 0))),
        AuthorRole::System => ("system", Style::default().fg(Color::DarkGray)),
    }
}

/// Extract displayable content for thread/session detail views.
pub fn detail_content(msg: &Message) -> String {
    if msg.message_type == MessageType::ToolCall {
        if let Some(input) = &msg.tool_input {
            return serde_json::to_string_pretty(input)
                .unwrap_or_else(|_| "[invalid tool input]".to_string());
        }
    }

    if msg.message_type == MessageType::ToolResult {
        if let Some(result) = &msg.tool_result {
            return result.clone();
        }
    }

    msg.content.clone().unwrap_or_default()
}

/// Short preview text for thread/session detail rows.
pub fn detail_preview(msg: &Message, max_chars: usize) -> String {
    msg.preview(max_chars)
}

/// Preview text for live stream rows.
pub fn live_preview(msg: &MessageWithContext) -> String {
    if msg.message_type == MessageType::ToolCall {
        if let Some(tool_name) = &msg.tool_name {
            return format!("{}: {}", tool_name, truncate_preview(&msg.preview, 40));
        }
    }
    truncate_preview(&msg.preview, 50).to_string()
}

fn truncate_preview(input: &str, max_chars: usize) -> &str {
    if input.chars().count() <= max_chars {
        return input;
    }
    input
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| &input[..idx])
        .unwrap_or(input)
}
