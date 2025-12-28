# aiobscura: Ubiquitous Language & Type System Analysis

*A Domain-Driven Design Review*

---

## Executive Summary

This document analyzes aiobscura's type system from a Domain-Driven Design (DDD) perspective, examining how well the current types form a coherent "ubiquitous language" for describing:

1. **Coding Assistants** - AI products that help developers write code
2. **Human-Assistant Workflows** - The collaboration patterns between humans and assistants
3. **Self-Improvement Mechanics** - How humans, assistants, and workflows evolve over time

The analysis finds that aiobscura has a **strong foundational type system** with thoughtful terminology choices, but identifies several **gaps** in modeling the self-improvement and outcome-tracking aspects of the domain.

---

## Part 1: Current Type System Overview

### Domain Model Diagram

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           aiobscura DOMAIN MODEL                                │
└─────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────────┐
│                        LAYER 1: CANONICAL (Core Domain)                         │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│   ┌─────────────┐          ┌─────────────┐         ┌──────────────┐            │
│   │   Project   │          │  Assistant  │         │ BackingModel │            │
│   │             │          │             │         │              │            │
│   │ • id        │          │ ClaudeCode  │         │ • id         │            │
│   │ • path      │          │ Codex       │─────────│ • provider   │            │
│   │ • name      │          │ Aider       │  uses   │ • model_id   │            │
│   │ • metadata  │          │ Cursor      │         │ • metadata   │            │
│   └──────┬──────┘          └──────┬──────┘         └──────────────┘            │
│          │                        │                                             │
│          │ has many               │ creates                                     │
│          │                        │                                             │
│          ▼                        ▼                                             │
│   ┌──────────────────────────────────────────┐                                  │
│   │                 Session                   │                                  │
│   │                                          │                                  │
│   │  • id                  • project_id      │                                  │
│   │  • assistant           • backing_model_id│                                  │
│   │  • started_at          • status          │                                  │
│   │  • last_activity_at    • metadata        │                                  │
│   └─────────────────┬────────────────────────┘                                  │
│                     │                                                           │
│                     │ contains                                                  │
│                     ▼                                                           │
│   ┌──────────────────────────────────────────┐                                  │
│   │                 Thread                    │◄──────┐                         │
│   │                                          │       │ parent_thread_id         │
│   │  • id             • thread_type          │───────┘                         │
│   │  • session_id       (Main|Agent|Background)                                │
│   │  • started_at     • spawned_by_message_id│                                  │
│   │  • ended_at       • last_activity_at     │                                  │
│   └─────────────────┬────────────────────────┘                                  │
│                     │                                                           │
│                     │ contains (ordered by seq)                                 │
│                     ▼                                                           │
│   ┌──────────────────────────────────────────┐                                  │
│   │                Message                    │                                  │
│   │                                          │                                  │
│   │  • id               • author_role        │                                  │
│   │  • thread_id          (Human|Assistant|  │                                  │
│   │  • seq                 Agent|Tool|System)│                                  │
│   │  • emitted_at       • message_type       │                                  │
│   │  • observed_at        (Prompt|Response|  │                                  │
│   │  • content            ToolCall|etc.)     │                                  │
│   │  • tool_name        • raw_data (lossless)│                                  │
│   │  • tool_input       • metadata           │                                  │
│   └──────────────────────────────────────────┘                                  │
│                                                                                 │
│   ┌─────────────────────────────┐                                               │
│   │           Plan              │  (standalone artifact, linked to Session)     │
│   │                             │                                               │
│   │  • id        • status       │                                               │
│   │  • session_id  (Active|     │                                               │
│   │  • path        Completed|   │                                               │
│   │  • title       Abandoned)   │                                               │
│   │  • content                  │                                               │
│   └─────────────────────────────┘                                               │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────────┐
│                        LAYER 2: DERIVED (Analytics)                             │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│   │SessionMetrics│  │  Assessment  │  │PluginMetric  │  │  Personality │       │
│   │              │  │              │  │              │  │              │       │
│   │ tokens_in/out│  │ assessor     │  │ plugin_name  │  │ Archaeologist│       │
│   │ tool_calls   │  │ scores (LLM) │  │ metric_name  │  │ Delegator    │       │
│   │ edit_churn   │  │ raw_response │  │ metric_value │  │ Philosopher  │       │
│   │ ...          │  │              │  │              │  │ ...          │       │
│   └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘       │
│                                                                                 │
│   ┌───────────────────────────────┐  ┌───────────────────────────────┐         │
│   │       DashboardStats          │  │        WrappedStats           │         │
│   │                               │  │                               │         │
│   │ • project_count               │  │ • totals (tokens, sessions)   │         │
│   │ • session_count               │  │ • tool_rankings               │         │
│   │ • daily_activity[28]          │  │ • time_patterns               │         │
│   │ • current_streak              │  │ • personality                 │         │
│   │ • peak_hour                   │  │ • trends                      │         │
│   └───────────────────────────────┘  └───────────────────────────────┘         │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

## Part 2: Ubiquitous Language Glossary

The following terms form aiobscura's ubiquitous language. Consistency in using these terms across code, documentation, and conversation is essential.

### Core Entities

| Term | Definition | Notes |
|------|------------|-------|
| **Project** | A codebase/directory that multiple Sessions and Assistants can work on | Enables cross-assistant analytics |
| **Assistant** | A coding assistant product (Claude Code, Codex, Aider, Cursor) | NOT an "agent" - products, not subprocesses |
| **BackingModel** | The LLM powering an assistant (opus-4.5, gpt-5, sonnet-4) | Separate entity for cost/capability tracking |
| **Session** | A period of activity by an Assistant on a Project | The main unit of work/analysis |
| **Thread** | A conversation flow within a Session | Main thread is implicit; agents spawn sub-threads |
| **Message** | The atomic unit of activity within a Thread | Replaces "Event" from earlier versions |
| **Plan** | A planning document associated with Sessions | Tracked separately; may span sessions |

### Roles

| Term | Definition | Notes |
|------|------------|-------|
| **Human** | Always a real person | Never ambiguous |
| **Caller** | CLI or parent assistant invoking a session/agent | The entity that initiated the interaction |
| **Assistant** (role) | The coding assistant responding to humans | Claude Code answering in main thread |
| **Agent** | A subprocess spawned by an Assistant | Task agents, explorers - never interact directly with Human |
| **Tool** | An executable capability (Bash, Read, Edit) | Has its own messages (tool_call, tool_result) |
| **System** | Internal events (snapshots, context loading) | Not user-facing |

### Why "Human" not "User"?

"User" is **deliberately avoided** because it's ambiguous:
- From an Agent's view: its "user" is the Assistant that spawned it
- From an Assistant's view: its "user" is the Human

By using "Human" consistently, we maintain clarity about who the real person is in any interaction context.

### Message Types

| Type | Author | Description |
|------|--------|-------------|
| `Prompt` | Human/Caller | Request/instruction to assistant |
| `Response` | Assistant/Agent | Reply from the assistant |
| `ToolCall` | Assistant/Agent | Request to invoke a tool |
| `ToolResult` | Tool | Result of tool execution |
| `Plan` | Assistant | Planning/reasoning output |
| `Summary` | System | Summarization of context |
| `Context` | System | Context loading |
| `Error` | Any | Error or exception |

---

## Part 3: Relationship Diagram

```
                                    ┌──────────────────┐
                                    │      Human       │
                                    │   (Developer)    │
                                    └────────┬─────────┘
                                             │
                                             │ initiates
                                             ▼
┌──────────────────┐              ┌──────────────────┐
│     Project      │◄─────────────│     Session      │
│                  │   works on   │                  │
│ ~/my-app         │              │ • 2 hours        │
│ ~/api-server     │              │ • 45 tool calls  │
└──────────────────┘              └────────┬─────────┘
                                           │
                                           │ runs within
                                           ▼
                                  ┌──────────────────┐
                                  │    Assistant     │──────► BackingModel
                                  │                  │        (opus-4.5)
                                  │  Claude Code     │
                                  │  Codex           │
                                  └────────┬─────────┘
                                           │
                                           │ orchestrates
                                           ▼
                    ┌──────────────────────────────────────────┐
                    │                 Threads                   │
                    │                                          │
                    │  ┌──────────────┐    ┌──────────────┐   │
                    │  │ Main Thread  │    │ Agent Thread │   │
                    │  │              │───►│              │   │
                    │  │ Human ←→     │    │ (Explore)    │   │
                    │  │ Assistant    │    │              │   │
                    │  └──────┬───────┘    └──────────────┘   │
                    │         │                               │
                    │         │                               │
                    └─────────┼───────────────────────────────┘
                              │
                              │ composed of
                              ▼
                    ┌──────────────────┐
                    │     Messages     │
                    │                  │
                    │ Prompt → Response│
                    │ ToolCall →       │
                    │ ToolResult       │
                    └──────────────────┘
```

---

## Part 4: Strengths of the Current Model

### 1. Clear Human vs Agent Distinction

The explicit `AuthorRole` enum eliminates the "user" ambiguity:

```rust
pub enum AuthorRole {
    Human,      // Always a real person
    Caller,     // CLI or parent assistant
    Assistant,  // The coding assistant product
    Agent,      // Subprocess spawned by assistant
    Tool,       // Tool execution
    System,     // Internal events
}
```

This is excellent DDD practice - the language is precise about *who* is acting.

### 2. Separation of Assistant from BackingModel

```rust
pub enum Assistant {        // Product
    ClaudeCode,
    Codex,
    Aider,
    Cursor,
}

pub struct BackingModel {   // LLM powering the product
    pub provider: String,   // "anthropic", "openai"
    pub model_id: String,   // "claude-opus-4-5-20251101"
}
```

This allows tracking that the same Assistant (e.g., Claude Code) may use different models over time, enabling cost analysis and capability comparison.

### 3. Thread Hierarchy for Agent Conversations

The `Thread` type properly models the hierarchical nature of agent spawning:

```rust
pub struct Thread {
    pub thread_type: ThreadType,        // Main | Agent | Background
    pub parent_thread_id: Option<String>,
    pub spawned_by_message_id: Option<i64>,
}
```

### 4. Dual Timestamp Model

Messages have both:
- `emitted_at` - when the event actually happened
- `observed_at` - when we ingested it

This is crucial for accurate timeline reconstruction when parsing logs post-hoc.

### 5. Lossless Data Capture

Every Message preserves `raw_data`:

```rust
pub struct Message {
    // ... parsed fields ...
    pub raw_data: serde_json::Value,  // Complete original record
    pub metadata: serde_json::Value,  // Parsed agent-specific fields
}
```

This ensures analytics can always access fields not in the common schema.

---

## Part 5: Identified Gaps

### Gap 1: No Human Developer Entity

**Problem:** The Human is implicit - only referenced via `AuthorRole::Human` in messages. There's no entity to track:
- Developer preferences
- Learning patterns over time
- Cross-session behavior
- Skill progression

**Impact:** Cannot answer "How has this developer's usage patterns evolved?" or "What tools has this human mastered vs struggled with?"

**Current State:**
```
Human ──(implicit)──► Session
```

**Proposed Addition:**
```
Developer ──(has many)──► Session
    │
    └── preferences, skill_profile, active_since
```

### Gap 2: No Workflow/Task Concept

**Problem:** There's no concept above Session for tracking a coherent piece of work:
- A feature implementation might span multiple sessions
- A bug investigation might pause and resume
- A refactoring effort might take weeks

**Impact:** Sessions are analyzed in isolation. Cannot answer "How long did this feature take across all related sessions?"

**Current State:**
```
Session ── (standalone, no higher grouping)
```

**Proposed Addition:**
```
Workflow ──(spans)──► Session[]
    │
    └── goal, status, started_at, completed_at
```

### Gap 3: No Outcome/Success Tracking

**Problem:** No types for capturing whether work was successful:
- Was the code accepted?
- Did tests pass?
- Was the PR merged?
- Did the human express satisfaction?

**Impact:** The `Personality` and `Wrapped` analytics describe *activity* but cannot measure *effectiveness*.

**Current State:**
```
Session → SessionMetrics (activity metrics only)
```

**Proposed Addition:**
```
Session → Outcome
    │
    └── success: bool
    └── evidence_type: (tests_pass | pr_merged | human_approval)
    └── notes: Option<String>
```

### Gap 4: No Learning/Improvement Tracking

**Problem:** The requirements mention "self-improvement mechanics" but no types support this:
- How is the Human improving in their prompting?
- How effectively is the Assistant handling this Project?
- Are repeated sessions on the same Project getting more efficient?

**Impact:** Cannot generate insights like "Your sessions are 30% more efficient than last month" or "Consider trying tool X which similar developers find useful."

### Gap 5: Agent Subtype Not Captured

**Problem:** When an Assistant spawns an Agent, we know it's ThreadType::Agent but not what *kind* of agent:
- Explore agent
- Plan agent
- Code review agent
- Test runner agent

**Impact:** Cannot analyze which agent types are most useful or which fail frequently.

**Current State:**
```rust
pub enum ThreadType {
    Main,
    Agent,      // No subtype information
    Background,
}
```

**Proposed Addition:**
```rust
pub enum ThreadType {
    Main,
    Agent {
        subtype: AgentSubtype  // Explore, Plan, CodeReview, etc.
    },
    Background,
}
```

### Gap 6: Tool Taxonomy Incomplete

**Problem:** Tools are just strings (`tool_name: Option<String>`). No formal model for:
- Tool categories (file operations, shell, search, etc.)
- Tool capabilities
- Tool success/failure patterns

**Impact:** Tool analytics are limited to counting. Cannot answer "What percentage of file searches lead to successful edits?"

---

## Part 6: Proposed Type Additions

### 6.1 Developer Entity (Optional - for future)

```rust
/// A human developer using AI assistants.
///
/// Tracks preferences and behavior over time.
pub struct Developer {
    /// Unique identifier (could be derived from system user)
    pub id: String,
    /// Display name
    pub name: Option<String>,
    /// When this developer was first seen
    pub first_seen_at: DateTime<Utc>,
    /// Preferences (default assistant, favorite tools, etc.)
    pub preferences: serde_json::Value,
    /// Computed skill profile (updated by analytics)
    pub skill_profile: Option<SkillProfile>,
}

pub struct SkillProfile {
    /// Tools the developer uses effectively
    pub proficient_tools: Vec<String>,
    /// Prompting style (verbose, terse, example-heavy)
    pub prompting_style: String,
    /// Average session efficiency (tokens per successful outcome)
    pub efficiency_score: f64,
}
```

### 6.2 Workflow/Task Entity (Recommended)

```rust
/// A coherent unit of work that may span multiple sessions.
///
/// Examples: "Implement OAuth", "Fix memory leak", "Refactor auth module"
pub struct Workflow {
    /// Unique identifier
    pub id: String,
    /// Human-readable goal
    pub goal: String,
    /// Current status
    pub status: WorkflowStatus,
    /// When work began
    pub started_at: DateTime<Utc>,
    /// When work was completed (if applicable)
    pub completed_at: Option<DateTime<Utc>>,
    /// Related sessions (M:N relationship)
    pub session_ids: Vec<String>,
    /// Tags for categorization
    pub tags: Vec<String>,
}

pub enum WorkflowStatus {
    Active,
    Paused,
    Completed,
    Abandoned,
}
```

### 6.3 Outcome Entity (Recommended)

```rust
/// The outcome of a Session or Workflow.
///
/// Captures whether the work achieved its goal.
pub struct Outcome {
    /// Database ID
    pub id: i64,
    /// Associated session (or workflow_id for workflow outcomes)
    pub session_id: Option<String>,
    pub workflow_id: Option<String>,
    /// Overall success assessment
    pub success: OutcomeSuccess,
    /// Type of evidence supporting the assessment
    pub evidence_type: EvidenceType,
    /// When the outcome was recorded
    pub recorded_at: DateTime<Utc>,
    /// Optional notes
    pub notes: Option<String>,
}

pub enum OutcomeSuccess {
    /// Clear success (tests pass, PR merged)
    Success,
    /// Partial success (some goals met)
    Partial,
    /// Not successful (abandoned, failed)
    Failure,
    /// Unknown/not yet determined
    Unknown,
}

pub enum EvidenceType {
    /// Automated (tests passed, build succeeded)
    Automated,
    /// Human confirmed success
    HumanApproval,
    /// PR/commit merged
    Merged,
    /// Inferred from lack of follow-up sessions
    Inferred,
    /// Manual assessment
    Manual,
}
```

### 6.4 Enhanced ThreadType (Recommended)

```rust
pub enum ThreadType {
    /// Implicit main conversation thread
    Main,
    /// Spawned by Task tool
    Agent(AgentSubtype),
    /// Background operations
    Background(BackgroundKind),
}

pub enum AgentSubtype {
    Explore,        // Codebase exploration
    Plan,           // Implementation planning
    CodeReview,     // Code review
    Test,           // Test running
    Research,       // Web research
    Custom(String), // User-defined
}

pub enum BackgroundKind {
    Summarization,
    Backup,
    Sync,
}
```

---

## Part 7: Recommended Priority

| Gap | Impact | Effort | Priority |
|-----|--------|--------|----------|
| Agent Subtype | Medium | Low | **P1** - Easy win, valuable for debugging |
| Outcome Entity | High | Medium | **P2** - Enables effectiveness metrics |
| Workflow Entity | High | Medium | **P2** - Enables cross-session analysis |
| Tool Taxonomy | Medium | Medium | **P3** - Nice to have |
| Developer Entity | Low | High | **P4** - Future consideration |
| Learning Tracking | High | High | **P5** - Requires analytics infrastructure |

---

## Part 7B: Schema-Type-Documentation Parity Analysis

This section identifies concrete discrepancies between the three sources of truth:
1. **types.rs** - Rust type definitions
2. **schema.rs** - SQLite database schema
3. **aiobscura-requirements.md** - Original specification

### Terminology Drift Map

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                        TERMINOLOGY EVOLUTION                                  │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                               │
│  Requirements Doc (v1.3)    →    types.rs (Current)    →    Schema (v7)      │
│  ──────────────────────          ─────────────────          ────────────     │
│                                                                               │
│  "agent" (column)           →    assistant             →    assistant        │
│  "events" (table)           →    Message               →    messages         │
│  "event_type"               →    MessageType           →    message_type     │
│  "ts" (timestamp)           →    emitted_at            →    emitted_at       │
│  "session_type"             →    (removed)             →    (removed)        │
│  —                          →    Thread                →    threads          │
│  —                          →    BackingModel          →    backing_models   │
│  —                          →    Project               →    projects         │
│                                                                               │
│  Legend: ✓ = aligned, ⚠ = drift, ✗ = missing                                │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Specific Discrepancies

#### 1. Requirements Doc Uses "agent" for Assistant Column

**Requirements (line 183):**
```sql
CREATE TABLE sessions (
    agent            TEXT NOT NULL,  -- "agent" column name
    ...
```

**Schema & Types (current):**
```sql
assistant        TEXT NOT NULL,      -- "assistant" column name
```

**Status:** ✓ Fixed in code, requirements doc is stale

#### 2. Requirements Doc Still References "events" Table

**Requirements (line 207-234):**
```sql
CREATE TABLE events (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    ...
);
```

**Current Code:** Uses `messages` table and `Message` type

**Status:** ⚠ Requirements doc needs update to match current terminology

#### 3. Session Type Removed Without Documentation

**Requirements (line 200-204):**
```
Session types:
- **agent_task** — Full agent coding session (human + AI + tools)
- **conversation** — Pure human-AI conversation (no tool use)
- **file_operation** — Batch file operations
```

**Current Code:** `session_type` field removed entirely from both types.rs and schema.rs

**Status:** ⚠ Requirements doc describes a feature that was removed. Either:
- Update requirements to remove session_type
- Or reintroduce it if still needed

#### 4. Thread Concept Not in Requirements

The `Thread` entity (with ThreadType::Main, Agent, Background) is a significant architectural addition not reflected in requirements.

**Impact:** The requirements describe a flat Session→Events model, but actual implementation has Session→Thread→Message hierarchy.

**Status:** ⚠ Requirements doc needs Thread documentation

#### 5. AuthorRole::Agent Marked as Unused

**types.rs (line 445):**
```rust
/// Subprocess spawned by assistant (Task agent, etc.)
/// Note: Currently unused by parsers; reserved for unified timeline views
Agent,
```

**Observation:** The parser uses `ThreadType::Agent` to detect agent files, but assigns `AuthorRole::Assistant` to their messages (not `AuthorRole::Agent`).

**Question:** Should agent thread messages have `author_role: Agent` instead of `Assistant`?

### Schema Tables Without Rust Types

The schema defines several tables that have no corresponding Rust types:

| Table | Purpose | Rust Type Needed? |
|-------|---------|-------------------|
| `agent_spawns` | Links agent_id → spawning_message_seq | Yes - currently uses HashMap in ParseResult |
| `session_plans` | M:N join table | Optional - could be derived |
| `plan_versions` | Content versioning | Yes - for plan history tracking |
| `source_files` | Checkpoint tracking | Partial - SourceFile exists but Checkpoint in types.rs |

### Parser-Type Usage Analysis

The Claude parser demonstrates correct usage of the type system:

```
┌────────────────────────────────────────────────────────────────┐
│                    PARSER TYPE MAPPING                          │
├────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Raw Log Field        Parser Decision           Assigned Type   │
│  ──────────────       ─────────────────         ─────────────   │
│                                                                 │
│  type: "user"    ──►  In main thread?                          │
│                       ├── Yes ──────────► AuthorRole::Human     │
│                       └── No (agent) ───► AuthorRole::Caller    │
│                                                                 │
│  type: "assistant" ─────────────────────► AuthorRole::Assistant │
│                                                                 │
│  content: tool_use ─────────────────────► MessageType::ToolCall │
│                                                                 │
│  content: tool_result ──────────────────► MessageType::ToolResult│
│                              or if is_error: MessageType::Error │
│                                                                 │
│  File: agent-*.jsonl ───────────────────► ThreadType::Agent     │
│  File: *.jsonl ─────────────────────────► ThreadType::Main      │
│                                                                 │
└────────────────────────────────────────────────────────────────┘
```

**Key Finding:** The `AuthorRole::Caller` is correctly used for "user" messages in agent threads, maintaining the Human vs Caller distinction.

---

## Part 7C: Concrete Fixes Checklist

### Documentation Fixes (No Code Changes)

- [ ] **Update requirements doc:** Replace "events" with "messages" throughout
- [ ] **Update requirements doc:** Replace "agent" column with "assistant"
- [ ] **Update requirements doc:** Remove session_type or document its removal
- [ ] **Add to requirements:** Thread concept and ThreadType enum
- [ ] **Add to requirements:** Dual timestamp model (emitted_at/observed_at)
- [ ] **Add to requirements:** Project entity and project_id foreign key

### Code Considerations

- [ ] **Decide:** Should agent thread messages use `AuthorRole::Agent`?
- [ ] **Add type:** `AgentSpawn` struct for the agent_spawns table
- [ ] **Add type:** `PlanVersion` struct for plan_versions table
- [ ] **Consider:** Removing the deprecated type aliases after migration period

---

## Part 8: Terminology Alignment Check

### Correct Usage Examples

| Context | Correct | Incorrect |
|---------|---------|-----------|
| Claude Code analyzing code | "The **Assistant** invoked the Read tool" | "The agent invoked..." |
| A Task subprocess exploring | "The **Agent** found 3 matching files" | "The assistant found..." |
| Person typing a prompt | "The **Human** asked for a refactor" | "The user asked..." |
| Subprocess spawning | "The Assistant spawned an **Agent**" | "Claude spawned a thread" |
| Opus 4.5 powering Claude Code | "The **BackingModel** is opus-4.5" | "The assistant model is..." |
| A coding session | "The **Session** lasted 2 hours" | "The conversation lasted..." |

### Deprecated Terms

| Deprecated | Use Instead | Reason |
|------------|-------------|--------|
| `Event` | `Message` | Events implies logging; Message captures communication |
| `AgentType` | `Assistant` | AgentType confused product with subprocess |
| `User` | `Human` or specific role | Ambiguous depending on perspective |
| `Conversation` | `Thread` or `Session` | Vague; use precise terms |

---

## Part 9: Summary

### Current Strengths

1. **Clear role taxonomy** (Human/Caller/Assistant/Agent/Tool/System)
2. **Product vs LLM separation** (Assistant vs BackingModel)
3. **Hierarchical threads** supporting agent conversations
4. **Lossless data capture** for future-proofing
5. **Dual timestamp model** for accurate timeline reconstruction
6. **Caller role for agents** - Parser correctly distinguishes Human from Caller in agent threads

### Documentation Drift (Requires Sync)

1. **Requirements doc stale** - Still uses "events", "agent" column, session_type
2. **Thread concept undocumented** - Major architectural feature not in requirements
3. **AuthorRole::Agent unused** - Defined but never assigned by parsers

### Schema-Type Gaps

1. **Missing types for tables:** agent_spawns, session_plans, plan_versions
2. **SourceFile partial:** Checkpoint enum exists but not fully integrated

### Conceptual Gaps (Future Work)

1. **No Developer entity** - Human is implicit
2. **No Workflow concept** - Sessions are isolated
3. **No Outcome tracking** - Activity without effectiveness
4. **Missing Agent subtypes** - Agents are undifferentiated
5. **No formal Tool taxonomy** - Tools are opaque strings

### Recommended Next Steps

#### Immediate (Documentation Sync)
1. Update requirements doc to match current terminology (messages, assistant, Thread)
2. Remove or document session_type decision
3. Add Thread hierarchy to requirements

#### Short-Term (Code)
1. Add `AgentSubtype` to `ThreadType::Agent` (minimal change, high value)
2. Decide: Should agent thread responses use `AuthorRole::Agent`?
3. Add missing types: `AgentSpawn`, `PlanVersion`

#### Medium-Term (Features)
1. Introduce `Outcome` entity for success tracking
2. Add `Workflow` entity for cross-session analysis

#### Long-Term (Enhancements)
1. Consider `Developer` entity for personalization
2. Formal Tool taxonomy for deeper analytics

---

*Document generated by aiobscura type system analysis*
*Last updated: December 2024*
