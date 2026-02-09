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

/// Returns a best-effort home directory path.
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
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

    /// Catsyphon collector configuration (optional)
    #[serde(default)]
    pub collector: CollectorConfig,
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

/// Catsyphon collector configuration
///
/// When enabled, aiobscura will push events to a Catsyphon server
/// in addition to storing them locally in SQLite.
#[derive(Debug, Deserialize, Clone)]
pub struct CollectorConfig {
    /// Enable/disable Catsyphon integration
    #[serde(default)]
    pub enabled: bool,

    /// Catsyphon server URL (e.g., `https://catsyphon.example.com`)
    pub server_url: Option<String>,

    /// Collector ID (UUID from registration)
    pub collector_id: Option<String>,

    /// API key (from registration, format: "cs_live_xxxx")
    pub api_key: Option<String>,

    /// Events per API call (max 50, default 20)
    #[serde(default = "default_collector_batch_size")]
    pub batch_size: usize,

    /// Max seconds before flushing incomplete batch
    #[serde(default = "default_collector_flush_interval")]
    pub flush_interval_secs: u64,

    /// HTTP request timeout in seconds
    #[serde(default = "default_collector_timeout")]
    pub timeout_secs: u64,

    /// Max retry attempts for transient failures
    #[serde(default = "default_collector_max_retries")]
    pub max_retries: usize,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server_url: None,
            collector_id: None,
            api_key: None,
            batch_size: default_collector_batch_size(),
            flush_interval_secs: default_collector_flush_interval(),
            timeout_secs: default_collector_timeout(),
            max_retries: default_collector_max_retries(),
        }
    }
}

impl CollectorConfig {
    /// Check if collector is properly configured and enabled
    pub fn is_ready(&self) -> bool {
        self.enabled
            && self.server_url.is_some()
            && self.collector_id.is_some()
            && self.api_key.is_some()
    }

    /// Validate configuration, returning error message if invalid
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        if self.server_url.is_none() {
            return Err(Error::Config(
                "collector.server_url is required when collector is enabled".to_string(),
            ));
        }
        if self.collector_id.is_none() {
            return Err(Error::Config(
                "collector.collector_id is required when collector is enabled".to_string(),
            ));
        }
        if self.api_key.is_none() {
            return Err(Error::Config(
                "collector.api_key is required when collector is enabled".to_string(),
            ));
        }
        if self.batch_size == 0 || self.batch_size > 50 {
            return Err(Error::Config(
                "collector.batch_size must be between 1 and 50".to_string(),
            ));
        }
        Ok(())
    }
}

fn default_collector_batch_size() -> usize {
    20
}

fn default_collector_flush_interval() -> u64 {
    5
}

fn default_collector_timeout() -> u64 {
    30
}

fn default_collector_max_retries() -> usize {
    3
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

    /// Ensure XDG base directory environment variables are set.
    ///
    /// This is mainly for CLI binaries that want explicit, stable path behavior
    /// before invoking other components that read these env vars.
    pub fn ensure_xdg_env() {
        let home = home_dir();

        if std::env::var("XDG_DATA_HOME").is_err() {
            std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
        }

        if std::env::var("XDG_STATE_HOME").is_err() {
            std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
        }

        if std::env::var("XDG_CONFIG_HOME").is_err() {
            std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        }
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

    #[test]
    fn test_collector_config_defaults() {
        let config = CollectorConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.batch_size, 20);
        assert_eq!(config.flush_interval_secs, 5);
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_retries, 3);
        assert!(!config.is_ready());
    }

    #[test]
    fn test_collector_config_validation() {
        // Disabled config is always valid
        let config = CollectorConfig::default();
        assert!(config.validate().is_ok());

        // Enabled without credentials should fail
        let config = CollectorConfig {
            enabled: true,
            ..Default::default()
        };
        assert!(config.validate().is_err());

        // Enabled with all credentials should pass
        let config = CollectorConfig {
            enabled: true,
            server_url: Some("https://catsyphon.example.com".to_string()),
            collector_id: Some("test-id".to_string()),
            api_key: Some("cs_live_test".to_string()),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
        assert!(config.is_ready());
    }

    #[test]
    fn test_parse_collector_config() {
        let toml = r#"
[collector]
enabled = true
server_url = "https://catsyphon.example.com"
collector_id = "550e8400-e29b-41d4-a716-446655440000"
api_key = "cs_live_xxxxxxxxxxxx"
batch_size = 30
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.collector.enabled);
        assert_eq!(
            config.collector.server_url.as_deref(),
            Some("https://catsyphon.example.com")
        );
        assert_eq!(config.collector.batch_size, 30);
        assert!(config.collector.is_ready());
    }
}
