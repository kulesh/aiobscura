//! Analytics plugins
//!
//! Each plugin lives in its own subdirectory to support
//! multiple files and resources if needed.
//!
//! ## Built-in Plugins
//!
//! - [`edit_churn`]: Tracks file modification patterns and churn ratio
//!
//! ## Creating Custom Plugins
//!
//! 1. Create a new module implementing [`AnalyticsPlugin`](super::AnalyticsPlugin)
//! 2. Register it with the engine via [`AnalyticsEngine::register`](super::AnalyticsEngine::register)
//!
//! Or use [`create_default_engine`] to get an engine with all built-in plugins.

pub mod edit_churn;

use super::AnalyticsEngine;

/// Create an engine with all built-in plugins registered.
///
/// This is the recommended way to get a ready-to-use analytics engine:
///
/// ```rust,ignore
/// use aiobscura_core::analytics::create_default_engine;
///
/// let engine = create_default_engine();
/// println!("Registered plugins: {:?}", engine.plugin_names());
/// ```
pub fn create_default_engine() -> AnalyticsEngine {
    let mut engine = AnalyticsEngine::new();
    engine.register(Box::new(edit_churn::EditChurnAnalyzer::new()));
    engine
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_engine_has_plugins() {
        let engine = create_default_engine();
        let names = engine.plugin_names();

        assert!(!names.is_empty(), "Default engine should have plugins");
        assert!(
            names.contains(&"core.edit_churn"),
            "Should include edit_churn plugin"
        );
    }
}
