//! Database schema and migrations
//!
//! Uses SQLite with embedded migrations managed via PRAGMA user_version.

use rusqlite::Connection;

/// Current schema version
pub const SCHEMA_VERSION: i32 = 3;

/// SQL migrations, indexed by version number
const MIGRATIONS: &[&str] = &[
    // Version 1: Initial schema (legacy)
    r#"
    -- ============================================
    -- LAYER 1: Canonical (lossless) - Legacy Schema
    -- ============================================

    CREATE TABLE IF NOT EXISTS sessions (
        id               TEXT PRIMARY KEY,
        agent            TEXT NOT NULL,
        session_type     TEXT NOT NULL,
        project_path     TEXT,
        started_at       DATETIME NOT NULL,
        last_activity_at DATETIME,
        status           TEXT,

        -- Lineage
        source_file      TEXT NOT NULL,

        -- Lossless capture
        raw_data         JSON,
        metadata         JSON
    );

    CREATE TABLE IF NOT EXISTS events (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id       TEXT NOT NULL REFERENCES sessions(id),
        seq              INTEGER NOT NULL,
        ts               DATETIME NOT NULL,
        event_type       TEXT NOT NULL,

        -- Common parsed fields
        tokens_in        INTEGER,
        tokens_out       INTEGER,
        tool_name        TEXT,
        tool_input       JSON,
        tool_result      TEXT,
        duration_ms      INTEGER,
        content          TEXT,

        -- Lineage
        source_file      TEXT NOT NULL,
        source_offset    INTEGER NOT NULL,
        source_line      INTEGER,

        -- Lossless capture
        raw_data         JSON NOT NULL,
        metadata         JSON
    );

    CREATE TABLE IF NOT EXISTS plans (
        id               TEXT PRIMARY KEY,
        agent            TEXT NOT NULL,
        project_path     TEXT,
        title            TEXT,
        content          TEXT NOT NULL,
        created_at       DATETIME NOT NULL,
        updated_at       DATETIME NOT NULL,

        -- Lineage
        source_file      TEXT NOT NULL,

        -- Lossless capture
        raw_data         JSON,
        metadata         JSON
    );

    CREATE TABLE IF NOT EXISTS checkpoints (
        source_path      TEXT PRIMARY KEY,
        agent            TEXT NOT NULL,
        entity_type      TEXT NOT NULL,
        file_hash        TEXT,
        byte_offset      INTEGER,
        last_rowid       INTEGER,
        last_event_ts    DATETIME,
        updated_at       DATETIME
    );

    -- ============================================
    -- LAYER 2: Derived (regenerable)
    -- ============================================

    CREATE TABLE IF NOT EXISTS session_metrics (
        session_id          TEXT PRIMARY KEY REFERENCES sessions(id),
        metric_version      INTEGER NOT NULL,
        computed_at         DATETIME NOT NULL,

        -- First-order aggregations
        total_tokens_in     INTEGER,
        total_tokens_out    INTEGER,
        total_tool_calls    INTEGER,
        tool_call_breakdown JSON,
        error_count         INTEGER,
        duration_ms         INTEGER,

        -- Higher-order derived
        tokens_per_minute   REAL,
        tool_success_rate   REAL,
        edit_churn_ratio    REAL
    );

    CREATE TABLE IF NOT EXISTS assessments (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id       TEXT NOT NULL REFERENCES sessions(id),
        assessor         TEXT NOT NULL,
        model            TEXT,
        assessed_at      DATETIME NOT NULL,
        scores           JSON NOT NULL,
        raw_response     TEXT,
        prompt_hash      TEXT
    );

    CREATE TABLE IF NOT EXISTS plugin_metrics (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        plugin_name      TEXT NOT NULL,
        entity_type      TEXT NOT NULL,
        entity_id        TEXT,
        metric_name      TEXT NOT NULL,
        metric_value     JSON NOT NULL,
        computed_at      DATETIME NOT NULL,

        UNIQUE(plugin_name, entity_type, entity_id, metric_name)
    );

    CREATE TABLE IF NOT EXISTS plugin_runs (
        id                INTEGER PRIMARY KEY AUTOINCREMENT,
        plugin_name       TEXT NOT NULL,
        session_id        TEXT,
        started_at        DATETIME NOT NULL,
        duration_ms       INTEGER NOT NULL,
        status            TEXT NOT NULL,
        error_message     TEXT,
        metrics_produced  INTEGER,
        input_event_count INTEGER,
        input_token_count INTEGER
    );

    -- ============================================
    -- Indexes
    -- ============================================

    CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id);
    CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts);
    CREATE INDEX IF NOT EXISTS idx_events_session_seq ON events(session_id, seq);
    CREATE INDEX IF NOT EXISTS idx_sessions_agent ON sessions(agent);
    CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
    CREATE INDEX IF NOT EXISTS idx_sessions_last_activity ON sessions(last_activity_at DESC);
    CREATE INDEX IF NOT EXISTS idx_plans_agent ON plans(agent);
    CREATE INDEX IF NOT EXISTS idx_assessments_session ON assessments(session_id);
    CREATE INDEX IF NOT EXISTS idx_plugin_runs_plugin ON plugin_runs(plugin_name, started_at);
    CREATE INDEX IF NOT EXISTS idx_plugin_runs_status ON plugin_runs(status) WHERE status != 'success';
    "#,
    // Version 2: New data model with Project, Thread, BackingModel, etc.
    r#"
    -- ============================================
    -- Drop legacy tables and recreate with new schema
    -- Note: This is a breaking migration. In production, we'd do a proper migration.
    -- For now, we drop and recreate since we're in early development.
    -- ============================================

    -- Drop old indexes first
    DROP INDEX IF EXISTS idx_events_session;
    DROP INDEX IF EXISTS idx_events_ts;
    DROP INDEX IF EXISTS idx_events_session_seq;
    DROP INDEX IF EXISTS idx_sessions_agent;
    DROP INDEX IF EXISTS idx_sessions_status;
    DROP INDEX IF EXISTS idx_sessions_last_activity;
    DROP INDEX IF EXISTS idx_plans_agent;
    DROP INDEX IF EXISTS idx_assessments_session;
    DROP INDEX IF EXISTS idx_plugin_runs_plugin;
    DROP INDEX IF EXISTS idx_plugin_runs_status;

    -- Drop old tables
    DROP TABLE IF EXISTS plugin_runs;
    DROP TABLE IF EXISTS plugin_metrics;
    DROP TABLE IF EXISTS assessments;
    DROP TABLE IF EXISTS session_metrics;
    DROP TABLE IF EXISTS checkpoints;
    DROP TABLE IF EXISTS plans;
    DROP TABLE IF EXISTS events;
    DROP TABLE IF EXISTS sessions;

    -- ============================================
    -- LAYER 1: Canonical (lossless) - New Schema
    -- ============================================

    -- Projects table (new)
    CREATE TABLE projects (
        id               TEXT PRIMARY KEY,
        path             TEXT NOT NULL UNIQUE,
        name             TEXT,
        created_at       DATETIME NOT NULL,
        last_activity_at DATETIME,
        metadata         JSON
    );

    CREATE INDEX idx_projects_path ON projects(path);

    -- Backing models table (new - separate for future enrichment)
    CREATE TABLE backing_models (
        id               TEXT PRIMARY KEY,   -- "provider:model_id"
        provider         TEXT NOT NULL,
        model_id         TEXT NOT NULL,
        display_name     TEXT,
        first_seen_at    DATETIME NOT NULL,
        metadata         JSON,

        UNIQUE(provider, model_id)
    );

    -- Source files table (checkpoint strategy is type-dependent)
    CREATE TABLE source_files (
        path             TEXT PRIMARY KEY,
        file_type        TEXT NOT NULL,      -- 'jsonl', 'json', 'markdown', 'sqlite'
        assistant        TEXT NOT NULL,
        created_at       DATETIME,
        modified_at      DATETIME,
        size_bytes       INTEGER,
        last_parsed_at   DATETIME,

        -- Checkpoint data (interpretation depends on file_type)
        checkpoint_type  TEXT,               -- 'byte_offset', 'content_hash', 'database_cursor', 'none'
        checkpoint_data  JSON                -- Type-specific
    );

    -- Sessions table (updated)
    CREATE TABLE sessions (
        id               TEXT PRIMARY KEY,
        assistant        TEXT NOT NULL,      -- 'claude_code', 'codex', etc.
        backing_model_id TEXT REFERENCES backing_models(id),
        project_id       TEXT REFERENCES projects(id),
        started_at       DATETIME NOT NULL,
        last_activity_at DATETIME,
        status           TEXT,               -- 'active', 'inactive', 'stale'
        source_file_path TEXT NOT NULL REFERENCES source_files(path),
        metadata         JSON                -- No raw_data: sessions are derived from messages
    );

    CREATE INDEX idx_sessions_project ON sessions(project_id);
    CREATE INDEX idx_sessions_assistant ON sessions(assistant);
    CREATE INDEX idx_sessions_backing_model ON sessions(backing_model_id);
    CREATE INDEX idx_sessions_status ON sessions(status);
    CREATE INDEX idx_sessions_last_activity ON sessions(last_activity_at DESC);

    -- Threads table (new)
    CREATE TABLE threads (
        id               TEXT PRIMARY KEY,
        session_id       TEXT NOT NULL REFERENCES sessions(id),
        thread_type      TEXT NOT NULL,      -- 'main', 'agent', 'background'
        parent_thread_id TEXT REFERENCES threads(id),
        spawned_by_message_id INTEGER,       -- FK to messages(id)
        started_at       DATETIME NOT NULL,
        ended_at         DATETIME,
        metadata         JSON
    );

    CREATE INDEX idx_threads_session ON threads(session_id);
    CREATE INDEX idx_threads_parent ON threads(parent_thread_id);

    -- Messages table (replaces events)
    CREATE TABLE messages (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id       TEXT NOT NULL REFERENCES sessions(id),
        thread_id        TEXT NOT NULL REFERENCES threads(id),
        seq              INTEGER NOT NULL,   -- Order within thread
        ts               DATETIME NOT NULL,

        -- Author
        author_role      TEXT NOT NULL,      -- 'human', 'assistant', 'agent', 'tool', 'system'
        author_name      TEXT,               -- 'Read', 'Bash', agent_id, etc.

        -- Message classification
        message_type     TEXT NOT NULL,      -- 'prompt', 'response', 'tool_call', etc.

        -- Content
        content          TEXT,
        tool_name        TEXT,
        tool_input       JSON,
        tool_result      TEXT,

        -- Metrics
        tokens_in        INTEGER,
        tokens_out       INTEGER,
        duration_ms      INTEGER,

        -- Lineage
        source_file_path TEXT NOT NULL REFERENCES source_files(path),
        source_offset    INTEGER NOT NULL,
        source_line      INTEGER,

        -- Lossless
        raw_data         JSON NOT NULL,
        metadata         JSON
    );

    CREATE INDEX idx_messages_session ON messages(session_id);
    CREATE INDEX idx_messages_thread ON messages(thread_id);
    CREATE INDEX idx_messages_ts ON messages(ts);

    -- Plans table (separate entity)
    CREATE TABLE plans (
        id               TEXT PRIMARY KEY,
        session_id       TEXT NOT NULL REFERENCES sessions(id),
        path             TEXT NOT NULL,
        title            TEXT,
        created_at       DATETIME NOT NULL,
        modified_at      DATETIME NOT NULL,
        status           TEXT,               -- 'active', 'completed', 'abandoned', 'unknown'
        content          TEXT,
        source_file_path TEXT NOT NULL REFERENCES source_files(path),
        raw_data         JSON,
        metadata         JSON
    );

    CREATE INDEX idx_plans_session ON plans(session_id);

    -- ============================================
    -- LAYER 2: Derived (regenerable)
    -- ============================================

    CREATE TABLE session_metrics (
        session_id          TEXT PRIMARY KEY REFERENCES sessions(id),
        metric_version      INTEGER NOT NULL,
        computed_at         DATETIME NOT NULL,

        -- First-order aggregations
        total_tokens_in     INTEGER,
        total_tokens_out    INTEGER,
        total_tool_calls    INTEGER,
        tool_call_breakdown JSON,
        error_count         INTEGER,
        duration_ms         INTEGER,

        -- Higher-order derived
        tokens_per_minute   REAL,
        tool_success_rate   REAL,
        edit_churn_ratio    REAL
    );

    CREATE TABLE assessments (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id       TEXT NOT NULL REFERENCES sessions(id),
        assessor         TEXT NOT NULL,
        model            TEXT,
        assessed_at      DATETIME NOT NULL,
        scores           JSON NOT NULL,
        raw_response     TEXT,
        prompt_hash      TEXT
    );

    CREATE INDEX idx_assessments_session ON assessments(session_id);

    CREATE TABLE plugin_metrics (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        plugin_name      TEXT NOT NULL,
        entity_type      TEXT NOT NULL,
        entity_id        TEXT,
        metric_name      TEXT NOT NULL,
        metric_value     JSON NOT NULL,
        computed_at      DATETIME NOT NULL,

        UNIQUE(plugin_name, entity_type, entity_id, metric_name)
    );

    CREATE TABLE plugin_runs (
        id                INTEGER PRIMARY KEY AUTOINCREMENT,
        plugin_name       TEXT NOT NULL,
        session_id        TEXT,
        started_at        DATETIME NOT NULL,
        duration_ms       INTEGER NOT NULL,
        status            TEXT NOT NULL,
        error_message     TEXT,
        metrics_produced  INTEGER,
        input_message_count INTEGER,
        input_token_count INTEGER
    );

    CREATE INDEX idx_plugin_runs_plugin ON plugin_runs(plugin_name, started_at);
    CREATE INDEX idx_plugin_runs_status ON plugin_runs(status) WHERE status != 'success';
    "#,
    // Version 3: Add agent_spawns table for incremental spawn linkage
    r#"
    -- Agent spawn mappings for linking agent threads to Task tool calls
    -- Persisted to survive incremental parses
    CREATE TABLE IF NOT EXISTS agent_spawns (
        agent_id             TEXT PRIMARY KEY,
        session_id           TEXT NOT NULL REFERENCES sessions(id),
        spawning_message_seq INTEGER NOT NULL,
        created_at           DATETIME NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_agent_spawns_session ON agent_spawns(session_id);
    "#,
];

/// Run all pending migrations
pub fn run_migrations(conn: &Connection) -> crate::error::Result<()> {
    let current_version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);

    tracing::info!(
        current_version,
        target_version = SCHEMA_VERSION,
        "Checking database migrations"
    );

    for (i, migration) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i32;
        if version > current_version {
            tracing::info!(version, "Running migration");
            conn.execute_batch(migration)?;
            conn.execute(&format!("PRAGMA user_version = {}", version), [])?;
        }
    }

    if current_version < SCHEMA_VERSION {
        tracing::info!(
            from = current_version,
            to = SCHEMA_VERSION,
            "Migrations complete"
        );
    }

    Ok(())
}

/// Get the current schema version from the database
pub fn get_schema_version(conn: &Connection) -> crate::error::Result<i32> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations_idempotent() {
        let conn = Connection::open_in_memory().unwrap();

        // Run migrations twice - should be idempotent
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        // Check version
        let version = get_schema_version(&conn).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_tables_created() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Check that all tables exist (new schema)
        let tables = [
            "projects",
            "backing_models",
            "source_files",
            "sessions",
            "threads",
            "messages",
            "plans",
            "session_metrics",
            "assessments",
            "plugin_metrics",
            "plugin_runs",
            "agent_spawns",
        ];

        for table in tables {
            let exists: i32 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "Table {} should exist", table);
        }
    }

    #[test]
    fn test_foreign_keys() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        run_migrations(&conn).unwrap();

        // Verify foreign key constraints are set up correctly by checking pragma
        let fk_list: Vec<(String, String)> = conn
            .prepare("PRAGMA foreign_key_list(sessions)")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(2)?, row.get::<_, String>(3)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        // sessions should reference backing_models and projects
        assert!(
            fk_list.iter().any(|(table, _)| table == "backing_models"),
            "sessions should reference backing_models"
        );
        assert!(
            fk_list.iter().any(|(table, _)| table == "projects"),
            "sessions should reference projects"
        );
    }
}
