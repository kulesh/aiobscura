# OpenAI Codex CLI Log Format Specification

This document describes the JSONL log format used by OpenAI Codex CLI, as understood by aiobscura for parsing and ingestion.

## Overview

Codex CLI stores conversation logs as **append-only JSONL files**. Each line is a complete JSON object representing one event in the conversation. This format enables:

- Incremental parsing (seek to byte offset, read new lines)
- Lossless capture (complete JSON preserved in `raw_data`)
- Crash recovery (partial writes only affect last line)

## File Locations

```
~/.codex/
├── config.json                          # Configuration (model info)
├── version.json                         # Version tracking
└── sessions/
    └── YYYY/
        └── MM/
            └── DD/
                └── rollout-{TIMESTAMP}-{ULID}.jsonl
```

### File Naming

| Pattern | Description |
|---------|-------------|
| `rollout-{ISO8601}-{ULID}.jsonl` | Session log |

Example: `rollout-2025-11-24T19-33-35-019ab86e-1e83-75b0-b2d7-d335492e7026.jsonl`

**Notes:**
- Timestamp uses dashes instead of colons for filesystem compatibility
- ULID (Universally Unique Lexicographically Sortable Identifier) ensures sortable session IDs
- Session ID is the ULID portion: `019ab86e-1e83-75b0-b2d7-d335492e7026`

## JSONL Record Structure

### Top-Level Event Container

Every line in the JSONL file has this structure:

```json
{
  "timestamp": "2025-11-25T00:33:35.897Z",
  "type": "session_meta|event_msg|response_item|turn_context",
  "payload": { ... }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | string | ISO8601/RFC3339 timestamp |
| `type` | string | Event type (see below) |
| `payload` | object | Type-specific data |

## Event Types

### 1. Session Meta (`type: "session_meta"`)

First record in the file, contains session initialization data:

```json
{
  "timestamp": "2025-11-25T00:33:35.897Z",
  "type": "session_meta",
  "payload": {
    "id": "019ab86e-1e83-75b0-b2d7-d335492e7026",
    "timestamp": "2025-11-25T00:33:35.875Z",
    "cwd": "/home/user/dev/project",
    "originator": "codex_cli_rs",
    "cli_version": "0.63.0",
    "instructions": "# Agent Guidelines\n...",
    "source": "cli",
    "model_provider": "openai",
    "git": {
      "commit_hash": "941533717dc6547231987459d769d4d141ee7f7e",
      "branch": "main",
      "repository_url": "git@github.com:user/repo.git"
    }
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Session ULID |
| `cwd` | string | Working directory |
| `originator` | string | Client type (e.g., `codex_cli_rs`) |
| `cli_version` | string | Codex CLI version |
| `instructions` | string | System prompt / AGENTS.md contents |
| `source` | string | Source: `cli`, `vscode`, etc. |
| `model_provider` | string | Provider: `openai` |
| `git` | object | Git repository info |

### 2. Event Message (`type: "event_msg"`)

User messages, token counts, and agent reasoning:

#### User Message

```json
{
  "timestamp": "2025-11-25T00:35:56.773Z",
  "type": "event_msg",
  "payload": {
    "type": "user_message",
    "message": "Please read the file and explain it.",
    "images": []
  }
}
```

#### Token Count

```json
{
  "timestamp": "2025-11-25T00:36:00.583Z",
  "type": "event_msg",
  "payload": {
    "type": "token_count",
    "info": {
      "total_token_usage": {
        "input_tokens": 4318,
        "cached_input_tokens": 3072,
        "output_tokens": 99,
        "reasoning_output_tokens": 64,
        "total_tokens": 4417
      },
      "last_token_usage": {
        "input_tokens": 4318,
        "cached_input_tokens": 3072,
        "output_tokens": 99,
        "reasoning_output_tokens": 64,
        "total_tokens": 4417
      },
      "model_context_window": 258400
    },
    "rate_limits": {
      "primary": {
        "used_percent": 1.0,
        "window_minutes": 300,
        "resets_at": 1764039758
      },
      "secondary": { ... },
      "credits": { ... }
    }
  }
}
```

#### Agent Reasoning

```json
{
  "type": "event_msg",
  "payload": {
    "type": "agent_reasoning",
    "text": "**Verifying issue tracking setup**"
  }
}
```

#### Agent Message

```json
{
  "type": "event_msg",
  "payload": {
    "type": "agent_message",
    "message": "**Design Critique**\n..."
  }
}
```

### 3. Response Item (`type: "response_item"`)

Structured responses from the model:

#### User/Assistant Message

```json
{
  "timestamp": "2025-11-25T00:33:35.897Z",
  "type": "response_item",
  "payload": {
    "type": "message",
    "role": "user",
    "content": [
      {
        "type": "input_text",
        "text": "Environment context..."
      }
    ]
  }
}
```

| Role | Description |
|------|-------------|
| `user` | User input or context injection |
| `assistant` | Model response |

#### Function Call (Tool Request)

```json
{
  "timestamp": "2025-11-25T00:36:05.711Z",
  "type": "response_item",
  "payload": {
    "type": "function_call",
    "name": "shell_command",
    "arguments": "{\"command\":\"ls -la\"}",
    "call_id": "call_c8Hb14zN8nJkIcW739lzXdPm"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Tool/function name |
| `arguments` | string | JSON-encoded arguments |
| `call_id` | string | Unique ID for linking to output |

#### Function Call Output (Tool Result)

```json
{
  "timestamp": "2025-11-25T00:36:05.711Z",
  "type": "response_item",
  "payload": {
    "type": "function_call_output",
    "call_id": "call_c8Hb14zN8nJkIcW739lzXdPm",
    "output": "Exit code: 0\nWall time: 5.1 seconds\nOutput:\n..."
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `call_id` | string | Links to originating function_call |
| `output` | string | Tool execution result |

#### Ghost Snapshot (Git State)

```json
{
  "timestamp": "2025-11-25T00:35:57.163Z",
  "type": "response_item",
  "payload": {
    "type": "ghost_snapshot",
    "ghost_commit": {
      "id": "368fa5c2acece5e8ef878406091b76b5f25c5b92",
      "parent": "941533717dc6547231987459d769d4d141ee7f7e",
      "preexisting_untracked_files": [...],
      "preexisting_untracked_dirs": [...]
    }
  }
}
```

Captures uncommitted git state for potential rollback.

#### Reasoning

```json
{
  "type": "response_item",
  "payload": {
    "type": "reasoning",
    "summary": [
      { "type": "summary_text", "text": "..." }
    ],
    "content": null,
    "encrypted_content": "gAAAAABpJPoGk4D..."
  }
}
```

Model thinking/reasoning (may be encrypted in production).

#### Custom Tool Call

```json
{
  "timestamp": "2025-11-25T00:50:09.953Z",
  "type": "response_item",
  "payload": {
    "type": "custom_tool_call",
    "status": "completed",
    "call_id": "call_QOfB5vNqL65vduFUzXvV1ZZP",
    "name": "apply_patch",
    "input": "*** Begin Patch\n*** Add File: src/example.py\n..."
  }
}
```

Custom tools (like `apply_patch`) that aren't standard shell commands.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Tool name (e.g., `apply_patch`) |
| `input` | string | Raw input (patch content, etc.) |
| `call_id` | string | Unique ID for linking to output |
| `status` | string | Execution status |

#### Custom Tool Call Output

```json
{
  "timestamp": "2025-11-25T00:50:09.953Z",
  "type": "response_item",
  "payload": {
    "type": "custom_tool_call_output",
    "call_id": "call_QOfB5vNqL65vduFUzXvV1ZZP",
    "output": "{\"output\":\"Success. Updated the following files:\\nA src/example.py\\n\",\"metadata\":{\"exit_code\":0,\"duration_seconds\":0.0}}"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `call_id` | string | Links to originating custom_tool_call |
| `output` | string | JSON-encoded result with metadata |

### 4. Turn Context (`type: "turn_context"`)

Environment snapshot at each conversation turn:

```json
{
  "timestamp": "2025-11-25T00:35:56.773Z",
  "type": "turn_context",
  "payload": {
    "cwd": "/home/user/dev/project",
    "approval_policy": "on-request",
    "sandbox_policy": {
      "type": "workspace-write",
      "network_access": false,
      "exclude_tmpdir_env_var": false,
      "exclude_slash_tmp": false
    },
    "model": "gpt-5.1-codex-max",
    "summary": "auto"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `cwd` | string | Current working directory |
| `approval_policy` | string | Tool approval policy |
| `sandbox_policy` | object | Sandbox configuration |
| `model` | string | Active model name |
| `summary` | string | Summary mode |

### 5. Compacted (`type: "compacted"`)

Context compaction marker, inserted when conversation history is summarized:

```json
{
  "timestamp": "2025-11-25T01:23:56.949Z",
  "type": "compacted",
  "payload": {
    "message": "",
    "replacement_history": [
      { "type": "message", "role": "user", "content": [...] },
      { "type": "compaction_summary", "encrypted_content": "gAAAAABpJQUs..." },
      { "type": "ghost_snapshot", "ghost_commit": {...} }
    ]
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `message` | string | Optional human-readable message |
| `replacement_history` | array | Summarized conversation history |

The `replacement_history` array contains the compacted context, including:
- Original user messages (preserved as-is)
- Encrypted compaction summaries
- Ghost snapshots of file state at compaction time

## Key Differences from Claude Code

| Aspect | Claude Code | Codex |
|--------|-------------|-------|
| **Session ID** | UUID (file stem) | ULID (in filename) |
| **Structure** | `type: user\|assistant` with embedded content | Separated: `event_msg` + `response_item` |
| **Tool Calls** | Embedded in `message.content[]` | Explicit `function_call` / `function_call_output` |
| **Tool Linking** | `tool_use_id` in content blocks | `call_id` field |
| **Token Tracking** | Per-message in `usage` object | Periodic `token_count` events |
| **Threading** | Implicit via `parentUuid` | No explicit threading (single stream) |
| **Agent Files** | Separate `agent-*.jsonl` files | Not applicable |
| **Git Tracking** | Per-session `gitBranch` | Per-turn + `ghost_snapshot` commits |

## Mapping to aiobscura Types

### Author Role Mapping

| Codex Event | `AuthorRole` |
|-------------|--------------|
| `event_msg.user_message` | `Human` |
| `event_msg.agent_message` | `Agent` |
| `response_item.message (role=user)` | `Human` |
| `response_item.message (role=assistant)` | `Assistant` |
| `response_item.function_call` | `Assistant` |
| `response_item.function_call_output` | `Tool` |
| `response_item.custom_tool_call` | `Assistant` |
| `response_item.custom_tool_call_output` | `Tool` |

### Message Type Mapping

| Codex Event | `MessageType` |
|-------------|---------------|
| `event_msg.user_message` | `Prompt` |
| `response_item.message (role=assistant)` | `Response` |
| `response_item.function_call` | `ToolCall` |
| `response_item.function_call_output` | `ToolResult` |
| `response_item.custom_tool_call` | `ToolCall` |
| `response_item.custom_tool_call_output` | `ToolResult` |
| `response_item.reasoning` | `Context` |
| `event_msg.agent_reasoning` | `Context` |
| `response_item.ghost_snapshot` | `Context` |
| `compacted` | `Context` |

### Field Mapping

| Codex Field | aiobscura Field | Notes |
|-------------|-----------------|-------|
| `session_meta.id` | `Session.id` | ULID from filename |
| `timestamp` | `Message.ts` | Parse as RFC3339 |
| `turn_context.model` | `Session.backing_model_id` | Prefix with `"openai:"` |
| `token_count.info.last_token_usage.input_tokens` | `Message.tokens_in` | |
| `token_count.info.last_token_usage.output_tokens` | `Message.tokens_out` | |
| `session_meta.cwd` | `Session.metadata.cwd` | |
| `session_meta.git` | `Session.metadata.git` | |
| `function_call.name` | `Message.tool_name` | |
| `function_call.arguments` | `Message.tool_input` | Parse JSON string |
| `function_call_output.output` | `Message.tool_result` | |

## Tool Call Linking

Codex uses `call_id` to link tool calls and their results:

```
function_call (call_id: "call_abc123")
     │
     │ (match by call_id)
     ▼
function_call_output (call_id: "call_abc123")
```

### Linking Algorithm

1. Parse `function_call` records, store `call_id → Message.id` mapping
2. When parsing `function_call_output`, look up `call_id` to find originating message
3. Store output in `Message.tool_result` of the tool call message

## Incremental Parsing

Codex JSONL files are append-only, enabling efficient incremental parsing:

### Algorithm

1. **Load checkpoint**: Get last parsed byte offset from `SourceFile.checkpoint`
2. **Validate**: If `offset > file_size`, file was truncated → reset to 0
3. **Seek**: Position file reader at byte offset
4. **Parse**: Read lines until EOF, tracking byte positions
5. **Store**: Save new checkpoint with final byte offset

### Checkpoint Storage

```rust
pub enum Checkpoint {
    ByteOffset { offset: u64 },  // For JSONL files
}
```

### Edge Cases

| Case | Handling |
|------|----------|
| File truncated | Reset offset to 0, log warning |
| Partial line at EOF | Stop before incomplete line |
| Invalid JSON line | Log warning, skip line, continue |
| Missing required field | Use defaults, store in `raw_data` |

## Example Session

A minimal session with user prompt and tool call:

```jsonl
{"timestamp":"2025-11-25T00:33:35.897Z","type":"session_meta","payload":{"id":"019ab86e-1e83-75b0-b2d7-d335492e7026","cwd":"/home/user/dev/project","originator":"codex_cli_rs","cli_version":"0.63.0","source":"cli","model_provider":"openai","git":{"branch":"main"}}}
{"timestamp":"2025-11-25T00:35:56.773Z","type":"event_msg","payload":{"type":"user_message","message":"List the files","images":[]}}
{"timestamp":"2025-11-25T00:35:56.773Z","type":"turn_context","payload":{"cwd":"/home/user/dev/project","model":"gpt-5.1-codex-max"}}
{"timestamp":"2025-11-25T00:36:05.711Z","type":"response_item","payload":{"type":"function_call","name":"shell_command","arguments":"{\"command\":\"ls -la\"}","call_id":"call_abc123"}}
{"timestamp":"2025-11-25T00:36:05.711Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call_abc123","output":"Exit code: 0\nOutput:\ntotal 8\ndrwxr-xr-x 3 user staff 96 Nov 25 00:33 ."}}
```

## References

- [OpenAI Codex CLI Documentation](https://developers.openai.com/codex/cli/reference/)
- [OpenAI Codex GitHub Repository](https://github.com/openai/codex)
- [aiobscura types.rs](../aiobscura-core/src/types.rs) - Domain type definitions
- [aiobscura architecture](./aiobscura-architecture.md) - System architecture
