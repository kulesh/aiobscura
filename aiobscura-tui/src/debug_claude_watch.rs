//! aiobscura-debug-claude-watch - Incremental Claude Code parser watcher
//!
//! Watches Claude Code log files for changes and outputs new content incrementally.
//! Uses polling to reliably detect append operations on macOS.
//! Outputs JSON in the same format as debug_claude for composability.

use aiobscura_core::ingest::parsers::ClaudeCodeParser;
use aiobscura_core::ingest::{AssistantParser, ParseContext};
use aiobscura_core::types::{Checkpoint, Message};
use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "aiobscura-debug-claude-watch")]
#[command(about = "Watch Claude Code logs and output new content incrementally")]
#[command(version)]
struct Args {
    /// Path to file or directory to watch
    #[arg(required = true)]
    path: PathBuf,

    /// Show only summary statistics (no messages)
    #[arg(long)]
    summary: bool,

    /// Compact JSON output (default: pretty)
    #[arg(long)]
    compact: bool,

    /// Poll interval in milliseconds
    #[arg(long, default_value = "1000")]
    poll: u64,
}

/// Output structure for incremental parse events
#[derive(Serialize)]
struct WatchOutput {
    event: String,
    file: String,
    from_offset: u64,
    to_offset: u64,
    stats: WatchStats,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    messages: Vec<MessageOutput>,
}

/// Summary statistics for incremental parse
#[derive(Serialize)]
struct WatchStats {
    message_count: usize,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    by_type: HashMap<String, usize>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    by_author: HashMap<String, usize>,
}

/// Simplified message output
#[derive(Serialize)]
struct MessageOutput {
    seq: i32,
    ts: chrono::DateTime<chrono::Utc>,
    author_role: String,
    message_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

/// State tracker for watched files
struct FileState {
    size: u64,
    checkpoint: Checkpoint,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Validate path exists
    if !args.path.exists() {
        anyhow::bail!("Path does not exist: {}", args.path.display());
    }

    // Initialize parser and state
    let parser = ClaudeCodeParser::new();
    let mut file_states: HashMap<PathBuf, FileState> = HashMap::new();

    // Collect files to watch
    let files: Vec<PathBuf> = if args.path.is_file() {
        vec![args.path.clone()]
    } else {
        std::fs::read_dir(&args.path)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
            .collect()
    };

    // Initialize file states with current sizes
    for file in &files {
        if let Ok(metadata) = std::fs::metadata(file) {
            let size = metadata.len();
            file_states.insert(
                file.clone(),
                FileState {
                    size,
                    checkpoint: Checkpoint::ByteOffset { offset: size },
                },
            );
        }
    }

    eprintln!(
        "Watching {} file(s) in {} (poll every {}ms)",
        files.len(),
        args.path.display(),
        args.poll
    );
    eprintln!("Use debug_claude for existing content. Ctrl+C to stop...");

    let poll_duration = Duration::from_millis(args.poll);

    // Main poll loop
    loop {
        // Check for new files if watching directory
        if args.path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&args.path) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "jsonl")
                        && !file_states.contains_key(&path)
                    {
                        // New file detected
                        if std::fs::metadata(&path).is_ok() {
                            eprintln!("New file detected: {}", path.display());
                            file_states.insert(
                                path.clone(),
                                FileState {
                                    size: 0, // Start from beginning for new files
                                    checkpoint: Checkpoint::None,
                                },
                            );
                        }
                    }
                }
            }
        }

        // Check each tracked file for changes
        for (path, state) in file_states.iter_mut() {
            if let Ok(metadata) = std::fs::metadata(path) {
                let current_size = metadata.len();

                // Skip if file hasn't grown
                if current_size <= state.size {
                    continue;
                }

                // File has grown - parse new content
                if let Err(e) = process_file_change(&parser, path, state, current_size, &args) {
                    eprintln!("Warning: Failed to process {}: {}", path.display(), e);
                }
            }
        }

        thread::sleep(poll_duration);
    }
}

fn process_file_change(
    parser: &ClaudeCodeParser,
    path: &Path,
    state: &mut FileState,
    new_size: u64,
    args: &Args,
) -> Result<()> {
    let from_offset = match &state.checkpoint {
        Checkpoint::ByteOffset { offset } => *offset,
        _ => 0,
    };

    let metadata = std::fs::metadata(path)
        .with_context(|| format!("Failed to read metadata: {}", path.display()))?;

    let modified_at = metadata
        .modified()
        .ok()
        .map(chrono::DateTime::from)
        .unwrap_or_else(chrono::Utc::now);

    // Parse from checkpoint
    let ctx = ParseContext {
        path,
        checkpoint: &state.checkpoint,
        file_size: new_size,
        modified_at,
    };

    let result = parser
        .parse(&ctx)
        .with_context(|| format!("Failed to parse: {}", path.display()))?;

    // Update state
    let to_offset = match &result.new_checkpoint {
        Checkpoint::ByteOffset { offset } => *offset,
        _ => new_size,
    };

    state.size = new_size;
    state.checkpoint = result.new_checkpoint.clone();

    // Skip output if no new messages
    if result.messages.is_empty() {
        return Ok(());
    }

    // Build output
    let output = WatchOutput {
        event: "change".to_string(),
        file: path.display().to_string(),
        from_offset,
        to_offset,
        stats: compute_stats(&result.messages),
        messages: if args.summary {
            vec![]
        } else {
            result.messages.iter().map(message_to_output).collect()
        },
    };

    // Output JSON
    if args.compact {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}

fn compute_stats(messages: &[Message]) -> WatchStats {
    let mut by_type: HashMap<String, usize> = HashMap::new();
    let mut by_author: HashMap<String, usize> = HashMap::new();

    for msg in messages {
        *by_type
            .entry(msg.message_type.as_str().to_string())
            .or_insert(0) += 1;
        *by_author
            .entry(msg.author_role.as_str().to_string())
            .or_insert(0) += 1;
    }

    WatchStats {
        message_count: messages.len(),
        by_type,
        by_author,
    }
}

fn message_to_output(msg: &Message) -> MessageOutput {
    MessageOutput {
        seq: msg.seq,
        ts: msg.ts,
        author_role: msg.author_role.as_str().to_string(),
        message_type: msg.message_type.as_str().to_string(),
        content: msg.content.clone(),
        tool_name: msg.tool_name.clone(),
    }
}
