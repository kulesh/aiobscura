//! Assistant-specific parsers
//!
//! Each supported assistant has a parser module that implements
//! the [`AssistantParser`](super::AssistantParser) trait.
//!
//! ## Supported Assistants
//!
//! | Assistant | Module | Status |
//! |-----------|--------|--------|
//! | Claude Code | [`claude`] | ✅ Implemented |
//! | Codex | [`codex`] | ✅ Implemented |
//! | Aider | `aider` | 📋 Planned |
//! | Cursor | `cursor` | 📋 Planned |

mod claude;
mod codex;

pub use claude::ClaudeCodeParser;
pub use codex::CodexParser;

use super::AssistantParser;
use crate::types::Assistant;

/// Create all available parsers.
///
/// Returns a vector of boxed parsers for all supported assistants.
/// Use this to initialize an [`IngestCoordinator`](super::IngestCoordinator).
pub fn create_all_parsers() -> Vec<Box<dyn AssistantParser>> {
    vec![
        Box::new(ClaudeCodeParser::new()),
        Box::new(CodexParser::new()),
        // Future: Box::new(AiderParser::new()),
        // Future: Box::new(CursorParser::new()),
    ]
}

/// Get a parser for a specific assistant.
///
/// Returns `None` if no parser is implemented for the given assistant.
pub fn parser_for(assistant: Assistant) -> Option<Box<dyn AssistantParser>> {
    match assistant {
        Assistant::ClaudeCode => Some(Box::new(ClaudeCodeParser::new())),
        Assistant::Codex => Some(Box::new(CodexParser::new())),
        _ => None, // Not yet implemented
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_all_parsers() {
        let parsers = create_all_parsers();
        assert!(!parsers.is_empty());
        assert!(parsers
            .iter()
            .any(|p| p.assistant() == Assistant::ClaudeCode));
    }

    #[test]
    fn test_parser_for_claude_code() {
        let parser = parser_for(Assistant::ClaudeCode);
        assert!(parser.is_some());
        assert_eq!(parser.unwrap().assistant(), Assistant::ClaudeCode);
    }

    #[test]
    fn test_parser_for_codex() {
        let parser = parser_for(Assistant::Codex);
        assert!(parser.is_some());
        assert_eq!(parser.unwrap().assistant(), Assistant::Codex);
    }

    #[test]
    fn test_parser_for_unimplemented() {
        let parser = parser_for(Assistant::Aider);
        assert!(parser.is_none());
    }
}
