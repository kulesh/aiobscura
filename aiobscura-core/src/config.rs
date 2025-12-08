//! Configuration loading and management
//!
//! Configuration is loaded from `~/.config/aiobscura/config.toml`
//!
//! This module follows the XDG Base Directory Specification:
//! - Config: `$XDG_CONFIG_HOME/aiobscura/` (~/.config/aiobscura/)
//! - Data: `$XDG_DATA_HOME/aiobscura/` (~/.local/share/aiobscura/)
//! - State/Logs: `$XDG_STATE_HOME/aiobscura/` (~/.local/state/aiobscura/)

use crate::error::{Error, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Returns $HOME or panics
fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .expect("HOME environment variable not set")
}

/// Returns XDG_CONFIG_HOME or ~/.config
fn xdg_config_home() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".config"))
}

/// Returns XDG_DATA_HOME or ~/.local/share
fn xdg_data_home() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".local/share"))
}

/// Returns XDG_STATE_HOME or ~/.local/state
fn xdg_state_home() -> PathBuf {
    std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".local/state"))
}

/// Main configuration struct
#[derive(Debug, Deserialize, Default)]
pub struct Config {
    /// LLM configuration for assessments (optional)
    #[serde(default)]
    pub llm: Option<LlmConfig>,

    /// Analytics configuration
    #[serde(default)]
    pub analytics: AnalyticsConfig,

    /// Agent path overrides
    #[serde(default)]
    pub agents: AgentOverrides,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// LLM provider configuration
#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    /// Provider type
    pub provider: LlmProvider,
    /// Model to use
    pub model: String,
    /// API endpoint (optional, uses default for provider)
    pub endpoint: Option<String>,
    /// API key (can also use env var)
    pub api_key: Option<String>,
}

/// Supported LLM providers
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Ollama,
    Claude,
    OpenAI,
}

impl LlmProvider {
    /// Returns the default endpoint for this provider
    pub fn default_endpoint(&self) -> &'static str {
        match self {
            LlmProvider::Ollama => "http://localhost:11434",
            LlmProvider::Claude => "https://api.anthropic.com",
            LlmProvider::OpenAI => "https://api.openai.com",
        }
    }
}

/// Analytics and assessment configuration
#[derive(Debug, Deserialize)]
pub struct AnalyticsConfig {
    /// Minutes of inactivity before triggering assessment
    #[serde(default = "default_inactivity_minutes")]
    pub inactivity_minutes: u32,

    /// Number of tool calls before triggering assessment
    #[serde(default = "default_tool_call_threshold")]
    pub tool_call_threshold: u32,

    /// Default timeout for plugins in milliseconds
    #[serde(default = "default_plugin_timeout")]
    pub timeout_ms: u64,

    /// List of disabled plugins
    #[serde(default)]
    pub disabled_plugins: Vec<String>,

    /// Per-plugin timeout overrides
    #[serde(default)]
    pub plugin_timeouts: std::collections::HashMap<String, u64>,
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        Self {
            inactivity_minutes: default_inactivity_minutes(),
            tool_call_threshold: default_tool_call_threshold(),
            timeout_ms: default_plugin_timeout(),
            disabled_plugins: vec![],
            plugin_timeouts: std::collections::HashMap::new(),
        }
    }
}

fn default_inactivity_minutes() -> u32 {
    15
}

fn default_tool_call_threshold() -> u32 {
    20
}

fn default_plugin_timeout() -> u64 {
    30000
}

/// Override paths for agent directories
#[derive(Debug, Deserialize, Default)]
pub struct AgentOverrides {
    /// Override path for Claude Code data
    pub claude_code_path: Option<PathBuf>,
    /// Override path for Codex data
    pub codex_path: Option<PathBuf>,
    /// Override path for Aider data
    pub aider_path: Option<PathBuf>,
    /// Override path for Cursor data
    pub cursor_path: Option<PathBuf>,
}

/// Logging configuration
#[derive(Debug, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Maximum number of log files to keep
    #[serde(default = "default_max_log_files")]
    pub max_files: usize,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            max_files: default_max_log_files(),
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_max_log_files() -> usize {
    5
}

impl Config {
    /// Load configuration from the default path
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();

        if !config_path.exists() {
            tracing::info!("No config file found at {:?}, using defaults", config_path);
            return Ok(Config::default());
        }

        Self::load_from(&config_path)
    }

    /// Load configuration from a specific path
    pub fn load_from(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("failed to read config file {:?}: {}", path, e)))?;

        let config: Config = toml::from_str(&content)
            .map_err(|e| Error::Config(format!("failed to parse config: {}", e)))?;

        Ok(config)
    }

    /// Returns the default config file path
    ///
    /// `$XDG_CONFIG_HOME/aiobscura/config.toml` (~/.config/aiobscura/config.toml)
    pub fn config_path() -> PathBuf {
        xdg_config_home().join("aiobscura").join("config.toml")
    }

    /// Returns the data directory path (for SQLite database)
    ///
    /// `$XDG_DATA_HOME/aiobscura/` (~/.local/share/aiobscura/)
    pub fn data_dir() -> PathBuf {
        xdg_data_home().join("aiobscura")
    }

    /// Returns the state directory path (for logs)
    ///
    /// `$XDG_STATE_HOME/aiobscura/` (~/.local/state/aiobscura/)
    pub fn state_dir() -> PathBuf {
        xdg_state_home().join("aiobscura")
    }

    /// Returns the database file path
    ///
    /// `$XDG_DATA_HOME/aiobscura/data.db` (~/.local/share/aiobscura/data.db)
    pub fn database_path() -> PathBuf {
        Self::data_dir().join("data.db")
    }

    /// Returns the log file path
    ///
    /// `$XDG_STATE_HOME/aiobscura/aiobscura.log` (~/.local/state/aiobscura/aiobscura.log)
    pub fn log_path() -> PathBuf {
        Self::state_dir().join("aiobscura.log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.llm.is_none());
        assert_eq!(config.analytics.inactivity_minutes, 15);
        assert_eq!(config.analytics.tool_call_threshold, 20);
    }

    #[test]
    fn test_parse_config() {
        let toml = r#"
[llm]
provider = "ollama"
model = "llama3.2"

[analytics]
inactivity_minutes = 30
tool_call_threshold = 50

[logging]
level = "debug"
"#;
        let config: Config = toml::from_str(toml).unwrap();

        let llm = config.llm.unwrap();
        assert_eq!(llm.provider, LlmProvider::Ollama);
        assert_eq!(llm.model, "llama3.2");
        assert_eq!(config.analytics.inactivity_minutes, 30);
        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn test_llm_provider_endpoints() {
        assert_eq!(
            LlmProvider::Ollama.default_endpoint(),
            "http://localhost:11434"
        );
        assert_eq!(
            LlmProvider::Claude.default_endpoint(),
            "https://api.anthropic.com"
        );
    }
}
