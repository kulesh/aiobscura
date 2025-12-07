# Claude Code Log Format Specification

This document describes the JSONL log format used by Claude Code, as understood by aiobscura for parsing and ingestion.

## Overview

Claude Code stores conversation logs as **append-only JSONL files**. Each line is a complete JSON object representing one event in the conversation. This format enables:

- Incremental parsing (seek to byte offset, read new lines)
- Lossless capture (complete JSON preserved in `raw_data`)
- Crash recovery (partial writes only affect last line)

## File Locations

```
~/.claude/
├── projects/
│   └── [encoded-project-path]/
│       ├── [session-uuid].jsonl       # Main session log
│       ├── agent-[agent-id].jsonl     # Agent/subagent logs
│       └── ...
├── plans/
│   └── [plan-slug].md                 # Plan files (markdown)
└── settings.json                       # User settings
```

### Path Encoding

Project paths are encoded by replacing `/` with `-`:

| Original Path | Encoded Folder Name |
|---------------|---------------------|
| `/Users/kulesh/dev/aiobscura` | `-Users-kulesh-dev-aiobscura` |
| `/home/user/projects/myapp` | `-home-user-projects-myapp` |

### File Naming

| Pattern | Description |
|---------|-------------|
| `{uuid}.jsonl` | Main session log (UUID format) |
| `agent-{id}.jsonl` | Agent/subagent log (spawned by Task tool) |

## JSONL Record Structure

### Common Fields

Every record contains these fields:

```json
{
  "uuid": "unique-message-id",
  "parentUuid": "parent-message-id-or-null",
  "sessionId": "session-uuid",
  "type": "assistant|user",
  "timestamp": "2025-12-06T18:04:55.986Z",
  "cwd": "/Users/kulesh/dev/aiobscura",
  "version": "2.0.59",
  "gitBranch": "main",
  "isSidechain": false,
  "userType": "external"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `uuid` | string | Unique identifier for this message |
| `parentUuid` | string? | Parent message ID (for threading) |
| `sessionId` | string | Session identifier |
| `type` | string | Record type: `"assistant"` or `"user"` |
| `timestamp` | string | ISO8601/RFC3339 timestamp |
| `cwd` | string | Current working directory |
| `version` | string | Claude Code version |
| `gitBranch` | string | Active git branch |
| `isSidechain` | boolean | True if this is an agent/sidechain message |
| `userType` | string | User type (e.g., "external") |

## Record Types

### 1. Assistant Message (`type: "assistant"`)

Assistant responses containing text and/or tool calls:

```json
{
  "type": "assistant",
  "message": {
    "model": "claude-opus-4-5-20251101",
    "id": "msg_01ABC...",
    "role": "assistant",
    "content": [
      {"type": "text", "text": "I'll help you with that."},
      {"type": "tool_use", "id": "toolu_01XYZ", "name": "Read", "input": {"file_path": "/path/to/file"}}
    ],
    "usage": {
      "input_tokens": 3582,
      "output_tokens": 125,
      "cache_creation_input_tokens": 0,
      "cache_read_input_tokens": 1500
    }
  },
  "requestId": "req_..."
}
```

### 2. User/Human Message (`type: "user"`)

#### Human Prompt (plain text)

```json
{
  "type": "user",
  "message": {
    "role": "user",
    "content": "Please read the file and explain what it does."
  }
}
```

#### Tool Result

```json
{
  "type": "user",
  "message": {
    "role": "user",
    "content": [
      {
        "type": "tool_result",
        "tool_use_id": "toolu_01XYZ",
        "content": "File contents here..."
      }
    ]
  },
  "toolUseResult": {
    "type": "tool_result",
    "name": "Read",
    "result": "File contents here...",
    "success": true
  }
}
```

### 3. Special Record Types

These record types should be **skipped** during parsing:

| Type | Description |
|------|-------------|
| `"file-history-snapshot"` | File state checkpoints for undo/redo |

Records with `"isSidechain": true` in the main session file are references to agent conversations stored in separate `agent-*.jsonl` files.

## Content Block Types

The `message.content` field can be:
- A **plain string** (simple text message)
- An **array of content blocks** (structured content)

### Content Block Schema

| Block Type | Fields | Description |
|------------|--------|-------------|
| `text` | `text: string` | Plain text content |
| `tool_use` | `id`, `name`, `input` | Tool invocation request |
| `tool_result` | `tool_use_id`, `content`, `is_error?` | Tool execution result |

#### Text Block

```json
{"type": "text", "text": "Here's my analysis of the code..."}
```

#### Tool Use Block

```json
{
  "type": "tool_use",
  "id": "toolu_01XYZ",
  "name": "Read",
  "input": {
    "file_path": "/Users/kulesh/dev/aiobscura/src/main.rs"
  }
}
```

#### Tool Result Block

```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_01XYZ",
  "content": "fn main() { ... }",
  "is_error": false
}
```

## Agent/Subagent Files

Agent files (`agent-[id].jsonl`) contain conversations from Task tool invocations. They have additional fields:

```json
{
  "agentId": "a1a93487",
  "slug": "warm-finding-russell",
  "sessionId": "parent-session-uuid",
  "isSidechain": true
}
```

| Field | Description |
|-------|-------------|
| `agentId` | Short identifier for this agent (matches filename) |
| `slug` | Human-readable slug (often matches plan file name) |
| `sessionId` | Links back to the parent session |
| `isSidechain` | Always `true` for agent files |

### Agent File Linkage

Agent files are linked to their spawning Task tool call through the `toolUseResult.agentId` field in the main session file.

#### Correlation Path

```
Main Session                              Agent File
────────────                              ──────────

Task tool_use message
  uuid: "5f41f821-..."
  message.content[].name: "Task"
           │
           │ (parentUuid points back)
           ▼
Task tool_result message          ───────►  agent-a4767a09.jsonl
  parentUuid: "5f41f821-..."                  agentId: "a4767a09"
  toolUseResult.agentId: "a4767a09"           sessionId: "b4749c81-..."
```

#### Task Tool Call (in main session)

When the assistant invokes the Task tool:

```json
{
  "uuid": "5f41f821-63c5-4d8e-a0c4-80a8fb673d21",
  "type": "assistant",
  "message": {
    "content": [{
      "type": "tool_use",
      "id": "toolu_01YYryTte3GmzGU3ZvMuZ22R",
      "name": "Task",
      "input": {
        "subagent_type": "Plan",
        "prompt": "Design the implementation..."
      }
    }]
  },
  "timestamp": "2025-12-06T18:21:29.464Z"
}
```

#### Task Tool Result (in main session)

When the agent completes, the result includes `agentId`:

```json
{
  "uuid": "76cb4f38-9514-4d64-bdc5-6febe9fad4c4",
  "parentUuid": "5f41f821-63c5-4d8e-a0c4-80a8fb673d21",
  "type": "user",
  "message": {
    "content": [{
      "type": "tool_result",
      "tool_use_id": "toolu_01YYryTte3GmzGU3ZvMuZ22R",
      "content": "Agent output..."
    }]
  },
  "toolUseResult": {
    "status": "completed",
    "agentId": "a4767a09",
    "prompt": "Design the implementation..."
  }
}
```

**Key insight**: The `toolUseResult.agentId` field provides the link between the spawning message and the agent file.

#### Linking Algorithm

To correlate agent files with their spawning messages:

1. Parse main session, find `tool_result` records where `toolUseResult.name == "Task"` (or has `agentId`)
2. Extract `toolUseResult.agentId` and `parentUuid`
3. Build map: `agentId → spawning_message_uuid`
4. When processing agent files, look up spawning message from map
5. Set `Thread.spawned_by_message_id` to reference the spawning message

#### Edge Cases

| Case | Handling |
|------|----------|
| Agent file without tool result | Log warning, `spawned_by_message_id = None` |
| Multiple agents with same slug | Use `agentId` for correlation (slugs can repeat) |
| Orphaned agent file | Agent exists but no Task call in session (truncated?) |

## Token Usage

The `usage` object in assistant messages tracks token consumption:

| Field | Description |
|-------|-------------|
| `input_tokens` | Tokens in the prompt sent to the model |
| `output_tokens` | Tokens in the model's response |
| `cache_creation_input_tokens` | Tokens added to prompt cache |
| `cache_read_input_tokens` | Tokens read from prompt cache |

**Total tokens for billing:** `input_tokens + output_tokens`
**Cache efficiency:** `cache_read_input_tokens / input_tokens`

## Mapping to aiobscura Types

### Author Role Mapping

| JSONL `type` | Content | `AuthorRole` |
|--------------|---------|--------------|
| `"assistant"` | any | `Assistant` |
| `"user"` | text | `Human` |
| `"user"` | tool_result | `Tool` |

### Message Type Mapping

| Content Block | Author | `MessageType` |
|---------------|--------|---------------|
| `text` | Assistant | `Response` |
| `text` | Human | `Prompt` |
| `tool_use` | Assistant | `ToolCall` |
| `tool_result` | Tool | `ToolResult` |

### Field Mapping

| JSONL Field | aiobscura Field | Notes |
|-------------|-----------------|-------|
| `sessionId` | `Session.id` | |
| `timestamp` | `Message.ts` | Parse as RFC3339 |
| `message.model` | `Session.backing_model_id` | Prefix with `"anthropic:"` |
| `message.usage.input_tokens` | `Message.tokens_in` | |
| `message.usage.output_tokens` | `Message.tokens_out` | |
| `cwd` | `Session.metadata.cwd` | Store in metadata |
| `gitBranch` | `Session.metadata.git_branch` | Store in metadata |

## Incremental Parsing

Claude Code JSONL files are append-only, enabling efficient incremental parsing:

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
    // ... other variants for other file types
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

A minimal session with one human prompt and one assistant response:

```jsonl
{"uuid":"msg-001","sessionId":"sess-123","type":"user","timestamp":"2025-12-06T10:00:00Z","message":{"role":"user","content":"Hello"}}
{"uuid":"msg-002","sessionId":"sess-123","type":"assistant","timestamp":"2025-12-06T10:00:05Z","message":{"model":"claude-opus-4-5-20251101","role":"assistant","content":[{"type":"text","text":"Hello! How can I help you today?"}],"usage":{"input_tokens":10,"output_tokens":15}}}
```

## References

- [Claude Code Documentation](https://docs.anthropic.com/claude-code)
- [aiobscura types.rs](../aiobscura-core/src/types.rs) - Domain type definitions
- [aiobscura architecture](./aiobscura-architecture.md) - System architecture
