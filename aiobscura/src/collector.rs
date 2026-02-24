//! aiobscura-collector - CLI tool for managing Catsyphon collector integration
//!
//! This tool provides commands for:
//! - Checking collector status and configuration
//! - Resuming incomplete publishes after crashes
//! - Manually flushing pending events
//!
//! Uses XDG Base Directory specification for file locations:
//! - Database: $XDG_DATA_HOME/aiobscura/data.db (~/.local/share/aiobscura/data.db)
//! - Config: $XDG_CONFIG_HOME/aiobscura/config.toml (~/.config/aiobscura/config.toml)

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command as ProcessCommand;

use aiobscura_core::collector::{CollectorClient, CollectorRegisterRequest, StatefulSyncPublisher};
use aiobscura_core::{Config, Database};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use toml::{map::Map, Value};

#[derive(Parser)]
#[command(name = "aiobscura-collector")]
#[command(about = "Manage Catsyphon collector integration")]
#[command(version)]
struct Args {
    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Register this machine as an edge collector and write local config.toml
    Register {
        /// Base URL of CatSyphon server
        #[arg(long)]
        server_url: String,

        /// Workspace UUID on CatSyphon
        #[arg(long)]
        workspace_id: String,

        /// Collector type (default: aiobscura)
        #[arg(long, default_value = "aiobscura")]
        collector_type: String,

        /// Hostname override (default: uname -n)
        #[arg(long)]
        hostname: Option<String>,

        /// Overwrite existing collector credentials in config.toml
        #[arg(long)]
        force: bool,
    },

    /// Show collector configuration and status
    Status,

    /// Resume publishing for sessions with unpublished messages
    Resume {
        /// Batch size for publishing (default: from config)
        #[arg(short, long)]
        batch_size: Option<usize>,
    },

    /// Flush any pending events in memory
    Flush,

    /// Show publish state for all active sessions
    Sessions {
        /// Show all sessions (including completed)
        #[arg(short, long)]
        all: bool,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    Config::ensure_xdg_env();

    // Load configuration
    let config = Config::load().context("failed to load configuration")?;

    // Initialize logging if verbose
    if args.verbose {
        let _log_guard = aiobscura_core::logging::init(&config.logging)
            .context("failed to initialize logging")?;
    }

    match args.command {
        Command::Register {
            server_url,
            workspace_id,
            collector_type,
            hostname,
            force,
        } => cmd_register(
            &config,
            &server_url,
            &workspace_id,
            &collector_type,
            hostname,
            force,
        ),
        Command::Status => cmd_status(&config),
        Command::Resume { batch_size } => cmd_resume(&config, batch_size),
        Command::Flush => cmd_flush(&config),
        Command::Sessions { all } => cmd_sessions(&config, all),
    }
}

struct RegisteredCollectorConfig {
    server_url: String,
    collector_id: String,
    api_key: String,
}

fn cmd_register(
    config: &Config,
    server_url: &str,
    workspace_id: &str,
    collector_type: &str,
    hostname: Option<String>,
    force: bool,
) -> Result<()> {
    let effective_hostname = hostname.unwrap_or_else(default_hostname);
    let request = CollectorRegisterRequest {
        collector_type: collector_type.to_string(),
        collector_version: env!("CARGO_PKG_VERSION").to_string(),
        hostname: effective_hostname.clone(),
        workspace_id: workspace_id.to_string(),
        metadata: Some(serde_json::json!({
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        })),
    };

    let timeout_secs = config.collector.timeout_secs.max(1);
    println!(
        "Registering '{}' on {} to {} (workspace: {})",
        collector_type, effective_hostname, server_url, workspace_id
    );

    let response = CollectorClient::register_collector_blocking(server_url, &request, timeout_secs)
        .context("failed to register collector with CatSyphon")?;

    let config_path = Config::config_path();
    let stored = RegisteredCollectorConfig {
        server_url: server_url.trim_end_matches('/').to_string(),
        collector_id: response.collector_id,
        api_key: response.api_key,
    };
    write_collector_config(&config_path, &stored, force)?;

    println!("Registration successful.");
    println!("Collector ID:    {}", stored.collector_id);
    println!("API Key Prefix:  {}", response.api_key_prefix);
    println!("Config updated:  {}", config_path.display());
    println!("Next step:       aiobscura-sync --watch");

    Ok(())
}

fn default_hostname() -> String {
    if let Ok(output) = ProcessCommand::new("uname").arg("-n").output() {
        if output.status.success() {
            let hostname = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !hostname.is_empty() {
                return hostname;
            }
        }
    }

    std::env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

fn write_collector_config(
    config_path: &Path,
    collector: &RegisteredCollectorConfig,
    force: bool,
) -> Result<()> {
    let existing = if config_path.exists() {
        fs::read_to_string(config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?
    } else {
        String::new()
    };

    let mut root = if existing.trim().is_empty() {
        Value::Table(Map::new())
    } else {
        toml::from_str::<Value>(&existing)
            .with_context(|| format!("failed to parse TOML config at {}", config_path.display()))?
    };

    let root_table = root
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("config root must be a TOML table"))?;
    let collector_value = root_table
        .entry("collector")
        .or_insert_with(|| Value::Table(Map::new()));

    let collector_table = match collector_value {
        Value::Table(table) => table,
        _ => {
            *collector_value = Value::Table(Map::new());
            collector_value
                .as_table_mut()
                .expect("collector table must be available")
        }
    };

    let existing_id = collector_table
        .get("collector_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let existing_key = collector_table
        .get("api_key")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if !force && (!existing_id.is_empty() || !existing_key.is_empty()) {
        bail!(
            "collector credentials already configured at {} (use --force to overwrite)",
            config_path.display()
        );
    }

    collector_table.insert("enabled".to_string(), Value::Boolean(true));
    collector_table.insert(
        "server_url".to_string(),
        Value::String(collector.server_url.clone()),
    );
    collector_table.insert(
        "collector_id".to_string(),
        Value::String(collector.collector_id.clone()),
    );
    collector_table.insert(
        "api_key".to_string(),
        Value::String(collector.api_key.clone()),
    );

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let tmp_path = config_path.with_extension("toml.tmp");
    let rendered = format!(
        "{}\n",
        toml::to_string_pretty(&root).context("failed to serialize config TOML")?
    );
    fs::write(&tmp_path, rendered)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    set_permissions_600(&tmp_path)?;
    fs::rename(&tmp_path, config_path)
        .with_context(|| format!("failed to atomically replace {}", config_path.display()))?;
    set_permissions_600(config_path)?;

    Ok(())
}

#[cfg(unix)]
fn set_permissions_600(path: &Path) -> Result<()> {
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set 0600 permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_permissions_600(_path: &Path) -> Result<()> {
    Ok(())
}

fn cmd_status(config: &Config) -> Result<()> {
    println!("Catsyphon Collector Configuration");
    println!("==================================");
    println!();

    let collector = &config.collector;

    println!("Enabled:         {}", collector.enabled);

    if !collector.enabled {
        println!();
        println!("Collector is disabled. Enable it in config.toml:");
        println!();
        println!("  [collector]");
        println!("  enabled = true");
        println!("  server_url = \"https://your-catsyphon-server.com\"");
        println!("  collector_id = \"your-collector-id\"");
        println!("  api_key = \"cs_live_xxxxxxxxxxxx\"");
        println!();
        println!("Or auto-register:");
        println!(
            "  aiobscura-collector register --server-url <url> --workspace-id <workspace-uuid>"
        );
        return Ok(());
    }

    println!(
        "Server URL:      {}",
        collector.server_url.as_deref().unwrap_or("<not set>")
    );
    println!(
        "Collector ID:    {}",
        collector.collector_id.as_deref().unwrap_or("<not set>")
    );
    println!(
        "API Key:         {}",
        if collector.api_key.is_some() {
            "<set>"
        } else {
            "<not set>"
        }
    );
    println!("Batch Size:      {}", collector.batch_size);
    println!("Flush Interval:  {}s", collector.flush_interval_secs);
    println!("Timeout:         {}s", collector.timeout_secs);
    println!("Max Retries:     {}", collector.max_retries);

    // Check if ready
    println!();
    if collector.is_ready() {
        println!("Status: Ready to publish");

        // Show database stats
        let db_path = Config::database_path();
        if db_path.exists() {
            let db = Database::open(&db_path).context("failed to open database")?;
            let states = db.get_active_publish_states()?;

            println!();
            println!("Active Sessions: {}", states.len());

            let incomplete = db.get_incomplete_publish_states()?;
            if !incomplete.is_empty() {
                println!(
                    "Incomplete:      {} (run 'resume' to publish)",
                    incomplete.len()
                );
            }
        }
    } else {
        println!("Status: Not ready (missing required configuration)");
    }

    Ok(())
}

fn cmd_resume(config: &Config, batch_size_override: Option<usize>) -> Result<()> {
    if !config.collector.is_ready() {
        println!("Collector is not configured. Run 'status' for details.");
        return Ok(());
    }

    let db_path = Config::database_path();
    if !db_path.exists() {
        println!("Database not found at {}", db_path.display());
        return Ok(());
    }

    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    let batch_size = batch_size_override.unwrap_or(config.collector.batch_size);

    let mut publisher = StatefulSyncPublisher::new(&config.collector, db)
        .context("failed to create publisher")?
        .expect("collector should be ready");

    println!("Checking for incomplete publishes...");

    let incomplete = publisher.database().get_incomplete_publish_states()?;
    if incomplete.is_empty() {
        println!("No incomplete publishes found.");
        return Ok(());
    }

    println!(
        "Found {} session(s) with unpublished messages",
        incomplete.len()
    );
    println!();

    for state in &incomplete {
        println!(
            "  Session: {} (last_seq: {})",
            &state.session_id[..8.min(state.session_id.len())],
            state.last_published_seq
        );
    }

    println!();
    println!("Resuming...");

    let sent = publisher.resume_incomplete(batch_size)?;

    println!();
    if sent > 0 {
        println!("Published {} event(s)", sent);
    } else {
        println!("No events published");
    }

    // Print stats
    let stats = publisher.stats();
    if stats.api_calls > 0 {
        println!();
        println!("Stats:");
        println!("  API Calls:  {}", stats.api_calls);
        println!("  Sent:       {}", stats.events_sent);
        println!("  Rejected:   {}", stats.events_rejected);
        println!("  Failures:   {}", stats.api_failures);
    }

    Ok(())
}

fn cmd_flush(config: &Config) -> Result<()> {
    if !config.collector.is_ready() {
        println!("Collector is not configured. Run 'status' for details.");
        return Ok(());
    }

    let db_path = Config::database_path();
    if !db_path.exists() {
        println!("Database not found at {}", db_path.display());
        return Ok(());
    }

    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    let mut publisher = StatefulSyncPublisher::new(&config.collector, db)
        .context("failed to create publisher")?
        .expect("collector should be ready");

    if !publisher.has_pending() {
        println!("No pending events to flush.");
        return Ok(());
    }

    println!("Flushing {} pending event(s)...", publisher.pending_count());

    let sent = publisher.flush_all()?;

    if sent > 0 {
        println!("Flushed {} event(s)", sent);
    } else {
        println!("No events flushed");
    }

    Ok(())
}

fn cmd_sessions(config: &Config, show_all: bool) -> Result<()> {
    let db_path = Config::database_path();
    if !db_path.exists() {
        println!("Database not found at {}", db_path.display());
        return Ok(());
    }

    let db = Database::open(&db_path).context("failed to open database")?;
    db.migrate().context("failed to run database migrations")?;

    let states = if show_all {
        // Get all publish states (would need a new method, for now just use active)
        db.get_active_publish_states()?
    } else {
        db.get_active_publish_states()?
    };

    if states.is_empty() {
        println!("No publish states found.");
        if !config.collector.is_ready() {
            println!();
            println!("Note: Collector is not configured. Run 'status' for details.");
        }
        return Ok(());
    }

    println!("Session Publish States");
    println!("======================");
    println!();
    println!(
        "{:<20} {:>10} {:>8} {:>20}",
        "Session ID", "Last Seq", "Status", "Last Published"
    );
    println!("{:-<62}", "");

    for state in states {
        let session_short = if state.session_id.len() > 18 {
            format!("{}...", &state.session_id[..15])
        } else {
            state.session_id.clone()
        };

        let last_published = state
            .last_published_at
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "never".to_string());

        println!(
            "{:<20} {:>10} {:>8} {:>20}",
            session_short, state.last_published_seq, state.status, last_published
        );
    }

    // Check for incomplete
    let incomplete = db.get_incomplete_publish_states()?;
    if !incomplete.is_empty() {
        println!();
        println!(
            "{} session(s) have unpublished messages. Run 'resume' to publish.",
            incomplete.len()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_collector_config_creates_collector_section() {
        let temp = TempDir::new().expect("failed to create temp dir");
        let config_path = temp.path().join("config.toml");
        let collector = RegisteredCollectorConfig {
            server_url: "https://catsyphon.example.com".to_string(),
            collector_id: "collector-123".to_string(),
            api_key: "cs_live_abc".to_string(),
        };

        write_collector_config(&config_path, &collector, false).expect("write should succeed");

        let content = fs::read_to_string(&config_path).expect("failed to read config");
        let parsed: Value = toml::from_str(&content).expect("failed to parse written TOML");
        let table = parsed
            .get("collector")
            .and_then(Value::as_table)
            .expect("missing collector table");

        assert_eq!(table.get("enabled").and_then(Value::as_bool), Some(true));
        assert_eq!(
            table.get("server_url").and_then(Value::as_str),
            Some("https://catsyphon.example.com")
        );
        assert_eq!(
            table.get("collector_id").and_then(Value::as_str),
            Some("collector-123")
        );
        assert_eq!(
            table.get("api_key").and_then(Value::as_str),
            Some("cs_live_abc")
        );
    }

    #[test]
    fn write_collector_config_preserves_existing_sections() {
        let temp = TempDir::new().expect("failed to create temp dir");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            r#"[logging]
level = "debug"

[collector]
batch_size = 50
"#,
        )
        .expect("failed to seed config");

        let collector = RegisteredCollectorConfig {
            server_url: "https://catsyphon.example.com".to_string(),
            collector_id: "collector-123".to_string(),
            api_key: "cs_live_abc".to_string(),
        };
        write_collector_config(&config_path, &collector, false).expect("write should succeed");

        let content = fs::read_to_string(&config_path).expect("failed to read config");
        let parsed: Value = toml::from_str(&content).expect("failed to parse written TOML");
        assert_eq!(
            parsed
                .get("logging")
                .and_then(Value::as_table)
                .and_then(|t| t.get("level"))
                .and_then(Value::as_str),
            Some("debug")
        );
        assert_eq!(
            parsed
                .get("collector")
                .and_then(Value::as_table)
                .and_then(|t| t.get("batch_size"))
                .and_then(Value::as_integer),
            Some(50)
        );
    }

    #[test]
    fn write_collector_config_requires_force_for_overwrite() {
        let temp = TempDir::new().expect("failed to create temp dir");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            r#"[collector]
enabled = true
collector_id = "old-collector"
api_key = "cs_live_old"
"#,
        )
        .expect("failed to seed config");

        let collector = RegisteredCollectorConfig {
            server_url: "https://catsyphon.example.com".to_string(),
            collector_id: "new-collector".to_string(),
            api_key: "cs_live_new".to_string(),
        };

        let result = write_collector_config(&config_path, &collector, false);
        assert!(result.is_err(), "overwrite should fail without --force");

        write_collector_config(&config_path, &collector, true).expect("forced overwrite failed");
        let content = fs::read_to_string(&config_path).expect("failed to read config");
        assert!(content.contains("new-collector"));
        assert!(content.contains("cs_live_new"));
    }

    #[test]
    fn default_hostname_falls_back_to_non_empty_value() {
        let hostname = default_hostname();
        assert!(!hostname.trim().is_empty());
    }
}
