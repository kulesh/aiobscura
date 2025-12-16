//! aiobscura-debug-claude - Claude Code parser debugging tool
//!
//! Parses Claude Code log files and outputs canonical format for debugging.

use aiobscura_core::ingest::parsers::ClaudeCodeParser;
use aiobscura_core::ingest::{AssistantParser, ParseContext, ParseResult};
use aiobscura_core::types::{Checkpoint, Message, Plan, Project, Session, Thread};
use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "aiobscura-debug-claude")]
#[command(about = "Parse Claude Code logs and output canonical format")]
#[command(version)]
struct Args {
    /// Path(s) to JSONL file(s) to parse
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Compact JSON output (default: pretty)
    #[arg(long)]
    compact: bool,

    /// Show only summary statistics (no messages)
    #[arg(long)]
    summary: bool,

    /// Include raw_data in message output
    #[arg(long)]
    include_raw: bool,

    /// Verbose output (show warnings)
    #[arg(short, long)]
    verbose: bool,

    /// Show plan file content
    #[arg(long)]
    show_plans: bool,

    /// Include checkpoint info in output (for incremental parsing)
    #[arg(long)]
    show_checkpoint: bool,
}

/// Output structure for the parsed result
#[derive(Serialize)]
struct DebugOutput {
    file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<Project>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<Session>,
    threads: Vec<Thread>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    slugs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    plans: Vec<PlanOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    messages: Vec<MessageOutput>,
    stats: Stats,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    agent_spawn_map: HashMap<String, i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    checkpoint: Option<CheckpointOutput>,
}

/// Checkpoint info for incremental parsing
#[derive(Serialize)]
struct CheckpointOutput {
    #[serde(rename = "type")]
    checkpoint_type: String,
    offset: u64,
}

/// Simplified plan output for debugging
#[derive(Serialize)]
struct PlanOutput {
    slug: String,
    title: Option<String>,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_preview: Option<String>,
    content_hash: Option<String>,
}

/// Message output with optional raw_data filtering
#[derive(Serialize)]
struct MessageOutput {
    id: i64,
    session_id: String,
    thread_id: String,
    seq: i32,
    ts: chrono::DateTime<chrono::Utc>,
    author_role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    author_name: Option<String>,
    message_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens_in: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens_out: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_data: Option<serde_json::Value>,
}

/// Summary statistics
#[derive(Serialize)]
struct Stats {
    message_count: usize,
    thread_count: usize,
    by_type: HashMap<String, usize>,
    by_author: HashMap<String, usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_tokens_in: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_tokens_out: Option<i32>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let parser = ClaudeCodeParser::new();
    let mut outputs = Vec::new();

    for file in &args.files {
        // Validate file exists
        if !file.exists() {
            eprintln!("Warning: File not found: {}", file.display());
            continue;
        }

        match parse_file(&parser, file) {
            Ok(result) => {
                let output = build_output(&args, file, &result);
                outputs.push(output);
            }
            Err(e) => {
                eprintln!("Warning: Failed to parse {}: {}", file.display(), e);
            }
        }
    }

    if outputs.is_empty() {
        anyhow::bail!("No files were successfully parsed");
    }

    // Output: single object for one file, array for multiple
    if outputs.len() == 1 {
        if args.compact {
            println!("{}", serde_json::to_string(&outputs[0])?);
        } else {
            println!("{}", serde_json::to_string_pretty(&outputs[0])?);
        }
    } else if args.compact {
        println!("{}", serde_json::to_string(&outputs)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&outputs)?);
    }

    Ok(())
}

fn parse_file(parser: &ClaudeCodeParser, file: &PathBuf) -> Result<ParseResult> {
    let metadata = std::fs::metadata(file)
        .with_context(|| format!("Failed to read file metadata: {}", file.display()))?;

    let modified_at = metadata
        .modified()
        .ok()
        .map(chrono::DateTime::from)
        .unwrap_or_else(chrono::Utc::now);

    let ctx = ParseContext {
        path: file,
        checkpoint: &Checkpoint::None,
        file_size: metadata.len(),
        modified_at,
    };

    parser
        .parse(&ctx)
        .with_context(|| format!("Failed to parse: {}", file.display()))
}

fn build_output(args: &Args, file: &std::path::Path, result: &ParseResult) -> DebugOutput {
    // Extract checkpoint if requested
    let checkpoint = if args.show_checkpoint {
        match &result.new_checkpoint {
            Checkpoint::ByteOffset { offset } => Some(CheckpointOutput {
                checkpoint_type: "ByteOffset".to_string(),
                offset: *offset,
            }),
            _ => None,
        }
    } else {
        None
    };

    // Compute statistics
    let stats = compute_stats(&result.messages);

    // Convert messages (optionally filtering raw_data)
    let messages = if args.summary {
        vec![]
    } else {
        result
            .messages
            .iter()
            .map(|m| message_to_output(m, args.include_raw))
            .collect()
    };

    // Include warnings if verbose
    let warnings = if args.verbose {
        result.warnings.clone()
    } else {
        vec![]
    };

    // Extract slugs from session metadata
    let slugs = result
        .session
        .as_ref()
        .and_then(|s| s.metadata.get("slugs"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Convert plans to output format
    let plans = if args.show_plans {
        result.plans.iter().map(plan_to_output).collect()
    } else {
        vec![]
    };

    DebugOutput {
        file: file.display().to_string(),
        project: result.project.clone(),
        session: result.session.clone(),
        threads: result.threads.clone(),
        slugs,
        plans,
        messages,
        stats,
        agent_spawn_map: result.agent_spawn_map.clone(),
        warnings,
        checkpoint,
    }
}

fn plan_to_output(plan: &Plan) -> PlanOutput {
    // Create a content preview (first 200 chars)
    let content_preview = plan.content.as_ref().map(|c| {
        let preview: String = c.chars().take(200).collect();
        if c.len() > 200 {
            format!("{}...", preview)
        } else {
            preview
        }
    });

    // Extract content hash from metadata
    let content_hash = plan
        .metadata
        .get("content_hash")
        .and_then(|v| v.as_str())
        .map(|s| {
            // Take first 16 chars of hash (safely)
            let end = 16.min(s.len());
            s[..end].to_string()
        });

    PlanOutput {
        slug: plan.id.clone(),
        title: plan.title.clone(),
        path: plan.path.display().to_string(),
        content_preview,
        content_hash,
    }
}

fn message_to_output(msg: &Message, include_raw: bool) -> MessageOutput {
    MessageOutput {
        id: msg.id,
        session_id: msg.session_id.clone(),
        thread_id: msg.thread_id.clone(),
        seq: msg.seq,
        ts: msg.ts,
        author_role: msg.author_role.as_str().to_string(),
        author_name: msg.author_name.clone(),
        message_type: msg.message_type.as_str().to_string(),
        content: msg.content.clone(),
        content_type: msg.content_type.as_ref().map(|ct| format!("{:?}", ct)),
        tool_name: msg.tool_name.clone(),
        tool_input: msg.tool_input.clone(),
        tool_result: msg.tool_result.clone(),
        tokens_in: msg.tokens_in,
        tokens_out: msg.tokens_out,
        raw_data: if include_raw {
            Some(msg.raw_data.clone())
        } else {
            None
        },
    }
}

fn compute_stats(messages: &[Message]) -> Stats {
    let mut by_type: HashMap<String, usize> = HashMap::new();
    let mut by_author: HashMap<String, usize> = HashMap::new();
    let mut total_tokens_in: i32 = 0;
    let mut total_tokens_out: i32 = 0;
    let mut has_tokens = false;

    for msg in messages {
        *by_type
            .entry(msg.message_type.as_str().to_string())
            .or_insert(0) += 1;
        *by_author
            .entry(msg.author_role.as_str().to_string())
            .or_insert(0) += 1;

        if let Some(t) = msg.tokens_in {
            total_tokens_in += t;
            has_tokens = true;
        }
        if let Some(t) = msg.tokens_out {
            total_tokens_out += t;
            has_tokens = true;
        }
    }

    // Count unique threads
    let thread_ids: std::collections::HashSet<_> = messages.iter().map(|m| &m.thread_id).collect();

    Stats {
        message_count: messages.len(),
        thread_count: thread_ids.len(),
        by_type,
        by_author,
        total_tokens_in: if has_tokens {
            Some(total_tokens_in)
        } else {
            None
        },
        total_tokens_out: if has_tokens {
            Some(total_tokens_out)
        } else {
            None
        },
    }
}
