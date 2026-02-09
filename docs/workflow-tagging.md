# Workflow Tagging (Lightweight Phase)

## Goal

Group related sessions without introducing a new `workflows` table yet.

## Current Approach

- Use `session.metadata.workflow_tag` as the grouping key.
- Read via `Session::workflow_tag()` in `aiobscura-core/src/types.rs`.
- Filter from CLI with:
  - `aiobscura-analyze --workflow <tag>`

This keeps the model simple while supporting immediate analysis queries by workflow.

## Tag Format

- Recommended format: `snake_case` short identifiers.
- Examples:
  - `feature_login`
  - `migration_pg15`
  - `incident_2026_02_09`

## Why Metadata First

- No schema/migration cost.
- Backwards-compatible with existing sessions.
- Lets us validate whether workflow grouping is heavily used before expanding the domain model.

## Promotion Criteria (Metadata -> First-Class Workflow)

Promote to a dedicated `Workflow` entity/table when at least one is true:

1. We need workflow-level lifecycle state (`planned`, `active`, `done`, `abandoned`).
2. We need workflow-level ownership/participants across assistants.
3. We need relationships between workflows (parent/child, dependency graph).
4. We need durable non-string attributes (priority, due date, outcome confidence).
5. Query performance degrades from repeated JSON metadata filtering at scale.

## Proposed Future Shape

- `workflows` table with stable ID and metadata.
- Join table `workflow_sessions(workflow_id, session_id)`.
- Optional workflow-scoped outcome record once outcome modeling matures.
