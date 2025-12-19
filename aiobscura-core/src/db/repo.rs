//! Database repository layer
//!
//! Provides query and insert operations for all entity types.

use crate::error::{Error, Result};
use crate::types::*;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Agent spawn info for linking threads to Task tool calls.
#[derive(Debug, Clone)]
pub struct AgentSpawnInfo {
    /// Agent ID (e.g., "a4767a09")
    pub agent_id: String,
    /// Session ID where the agent was spawned
    pub session_id: String,
    /// Seq number of the spawning Task tool_use message
    pub spawning_message_seq: i64,
}

/// Tool usage statistics for a thread.
#[derive(Debug, Clone, Default)]
pub struct ToolStats {
    /// Total number of tool calls
    pub total_calls: i64,
    /// Breakdown by tool name, sorted by count descending
    pub breakdown: Vec<(String, i64)>,
}

/// Stats for an assistant's source files: (assistant, file_count, total_size_bytes, last_parsed_at)
pub type AssistantSourceStats = (Assistant, i64, i64, Option<DateTime<Utc>>);

/// Token usage statistics.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    /// Total input tokens
    pub tokens_in: i64,
    /// Total output tokens
    pub tokens_out: i64,
}

/// File modification statistics for a thread.
#[derive(Debug, Clone, Default)]
pub struct FileStats {
    /// Total number of unique files modified
    pub total_files: i64,
    /// Breakdown by file path, sorted by edit count descending
    pub breakdown: Vec<(String, i64)>,
}

/// Session summary for list views in the TUI.
///
/// Contains pre-computed stats to avoid N+1 queries when rendering session lists.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    /// Session ID
    pub id: String,
    /// Which assistant this session is from
    pub assistant: Assistant,
    /// When the session started
    pub started_at: DateTime<Utc>,
    /// Most recent activity timestamp
    pub last_activity_at: Option<DateTime<Utc>>,
    /// Number of threads in this session
    pub thread_count: i64,
    /// Total message count across all threads
    pub message_count: i64,
    /// Model name (if known)
    pub model_name: Option<String>,
}

/// Thread summary with session and project context for list views.
#[derive(Debug, Clone)]
pub struct ThreadSummary {
    /// Thread record
    pub thread: Thread,
    /// Which assistant this thread belongs to
    pub assistant: Assistant,
    /// Project display name (if known)
    pub project_name: Option<String>,
    /// Total message count for the thread
    pub message_count: i64,
}

/// Metadata for a thread detail view.
#[derive(Debug, Clone)]
pub struct ThreadMetadata {
    /// Source file path
    pub source_path: Option<String>,
    /// Working directory
    pub cwd: Option<String>,
    /// Git branch
    pub git_branch: Option<String>,
    /// Model display name
    pub model_name: Option<String>,
    /// Session duration in seconds
    pub duration_secs: i64,
    /// Total message count
    pub message_count: i64,
    /// Total agent threads in the session
    pub agent_count: i64,
    /// Tool usage stats
    pub tool_stats: ToolStats,
    /// Plan count for the session
    pub plan_count: i64,
    /// File modification stats
    pub file_stats: FileStats,
}

/// Environment health statistics for a single assistant.
#[derive(Debug, Clone)]
pub struct AssistantHealth {
    /// The assistant type
    pub assistant: Assistant,
    /// Number of source files tracked
    pub file_count: i64,
    /// Total size of source files in bytes
    pub total_size_bytes: i64,
    /// Last time files were parsed/synced
    pub last_synced: Option<DateTime<Utc>>,
}

/// Overall environment health stats.
#[derive(Debug, Clone, Default)]
pub struct EnvironmentHealth {
    /// Database size in bytes
    pub database_size_bytes: u64,
    /// Per-assistant health stats
    pub assistants: Vec<AssistantHealth>,
    /// Total session count
    pub total_sessions: i64,
    /// Total message count
    pub total_messages: i64,
}

/// Database handle with connection pooling (single connection for now)
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open or create a database at the given path
    pub fn open(path: &PathBuf) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Enable foreign keys and WAL mode for better concurrency
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = -64000;  -- 64MB cache
            ",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database (for testing)
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Run migrations on this database
    pub fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        super::schema::run_migrations(&conn)
    }

    /// Get the underlying connection (for advanced use)
    pub fn connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    // ============================================
    // Project operations
    // ============================================

    /// Insert or update a project
    pub fn upsert_project(&self, project: &Project) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO projects (id, path, name, created_at, last_activity_at, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                last_activity_at = excluded.last_activity_at,
                metadata = excluded.metadata
            "#,
            params![
                project.id,
                project.path.to_string_lossy().to_string(),
                project.name,
                project.created_at.to_rfc3339(),
                project.last_activity_at.map(|t| t.to_rfc3339()),
                project.metadata.to_string(),
            ],
        )?;
        Ok(())
    }

    /// Get a project by ID
    pub fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT * FROM projects WHERE id = ?", [id], |row| {
            Self::row_to_project(row)
        })
        .optional()
        .map_err(Error::from)
    }

    /// Get a project by path
    pub fn get_project_by_path(&self, path: &Path) -> Result<Option<Project>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT * FROM projects WHERE path = ?",
            [path.to_string_lossy().to_string()],
            Self::row_to_project,
        )
        .optional()
        .map_err(Error::from)
    }

    fn row_to_project(row: &Row) -> rusqlite::Result<Project> {
        let path_str: String = row.get("path")?;
        let created_at_str: String = row.get("created_at")?;
        let last_activity_str: Option<String> = row.get("last_activity_at")?;
        let metadata_str: String = row.get("metadata")?;

        Ok(Project {
            id: row.get("id")?,
            path: PathBuf::from(path_str),
            name: row.get("name")?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            last_activity_at: last_activity_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            metadata: serde_json::from_str(&metadata_str).unwrap_or(serde_json::json!({})),
        })
    }

    // ============================================
    // BackingModel operations
    // ============================================

    /// Insert or update a backing model
    pub fn upsert_backing_model(&self, model: &BackingModel) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO backing_models (id, provider, model_id, display_name, first_seen_at, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                metadata = excluded.metadata
            "#,
            params![
                model.id,
                model.provider,
                model.model_id,
                model.display_name,
                model.first_seen_at.to_rfc3339(),
                model.metadata.to_string(),
            ],
        )?;
        Ok(())
    }

    /// Get a backing model by ID
    pub fn get_backing_model(&self, id: &str) -> Result<Option<BackingModel>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT * FROM backing_models WHERE id = ?",
            [id],
            Self::row_to_backing_model,
        )
        .optional()
        .map_err(Error::from)
    }

    fn row_to_backing_model(row: &Row) -> rusqlite::Result<BackingModel> {
        let first_seen_str: String = row.get("first_seen_at")?;
        let metadata_str: String = row.get("metadata")?;

        Ok(BackingModel {
            id: row.get("id")?,
            provider: row.get("provider")?,
            model_id: row.get("model_id")?,
            display_name: row.get("display_name")?,
            first_seen_at: DateTime::parse_from_rfc3339(&first_seen_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            metadata: serde_json::from_str(&metadata_str).unwrap_or(serde_json::json!({})),
        })
    }

    // ============================================
    // SourceFile operations
    // ============================================

    /// Insert or update a source file
    pub fn upsert_source_file(&self, file: &SourceFile) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        let (checkpoint_type, checkpoint_data) = match &file.checkpoint {
            Checkpoint::ByteOffset { offset } => (
                "byte_offset".to_string(),
                serde_json::json!({"offset": offset}),
            ),
            Checkpoint::ContentHash { hash } => (
                "content_hash".to_string(),
                serde_json::json!({"hash": hash}),
            ),
            Checkpoint::DatabaseCursor {
                table,
                cursor_column,
                cursor_value,
            } => (
                "database_cursor".to_string(),
                serde_json::json!({
                    "table": table,
                    "cursor_column": cursor_column,
                    "cursor_value": cursor_value,
                }),
            ),
            Checkpoint::None => ("none".to_string(), serde_json::json!(null)),
        };

        conn.execute(
            r#"
            INSERT INTO source_files (path, file_type, assistant, created_at, modified_at,
                                       size_bytes, last_parsed_at, checkpoint_type, checkpoint_data)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(path) DO UPDATE SET
                modified_at = excluded.modified_at,
                size_bytes = excluded.size_bytes,
                last_parsed_at = excluded.last_parsed_at,
                checkpoint_type = excluded.checkpoint_type,
                checkpoint_data = excluded.checkpoint_data
            "#,
            params![
                file.path.to_string_lossy().to_string(),
                file.file_type.as_str(),
                file.assistant.as_str(),
                file.created_at.to_rfc3339(),
                file.modified_at.to_rfc3339(),
                file.size_bytes as i64,
                file.last_parsed_at.map(|t| t.to_rfc3339()),
                checkpoint_type,
                checkpoint_data.to_string(),
            ],
        )?;
        Ok(())
    }

    /// Get a source file by path
    pub fn get_source_file(&self, path: &str) -> Result<Option<SourceFile>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT * FROM source_files WHERE path = ?",
            [path],
            Self::row_to_source_file,
        )
        .optional()
        .map_err(Error::from)
    }

    fn row_to_source_file(row: &Row) -> rusqlite::Result<SourceFile> {
        let path_str: String = row.get("path")?;
        let file_type_str: String = row.get("file_type")?;
        let assistant_str: String = row.get("assistant")?;
        let created_at_str: String = row.get("created_at")?;
        let modified_at_str: String = row.get("modified_at")?;
        let size_bytes: i64 = row.get("size_bytes")?;
        let last_parsed_str: Option<String> = row.get("last_parsed_at")?;
        let checkpoint_type: Option<String> = row.get("checkpoint_type")?;
        let checkpoint_data_str: Option<String> = row.get("checkpoint_data")?;

        let checkpoint = match checkpoint_type.as_deref() {
            Some("byte_offset") => {
                let data: serde_json::Value = checkpoint_data_str
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(serde_json::json!({}));
                Checkpoint::ByteOffset {
                    offset: data["offset"].as_u64().unwrap_or(0),
                }
            }
            Some("content_hash") => {
                let data: serde_json::Value = checkpoint_data_str
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(serde_json::json!({}));
                Checkpoint::ContentHash {
                    hash: data["hash"].as_str().unwrap_or("").to_string(),
                }
            }
            Some("database_cursor") => {
                let data: serde_json::Value = checkpoint_data_str
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(serde_json::json!({}));
                Checkpoint::DatabaseCursor {
                    table: data["table"].as_str().unwrap_or("").to_string(),
                    cursor_column: data["cursor_column"].as_str().unwrap_or("").to_string(),
                    cursor_value: data["cursor_value"].as_str().unwrap_or("").to_string(),
                }
            }
            _ => Checkpoint::None,
        };

        Ok(SourceFile {
            path: PathBuf::from(path_str),
            file_type: file_type_str.parse().unwrap_or(FileType::Jsonl),
            assistant: assistant_str.parse().unwrap_or(Assistant::ClaudeCode),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            modified_at: DateTime::parse_from_rfc3339(&modified_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            size_bytes: size_bytes as u64,
            last_parsed_at: last_parsed_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            checkpoint,
        })
    }

    // ============================================
    // Session operations
    // ============================================

    /// Insert or update a session
    pub fn upsert_session(&self, session: &Session) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO sessions (id, assistant, backing_model_id, project_id, started_at,
                                  last_activity_at, status, source_file_path, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                backing_model_id = excluded.backing_model_id,
                project_id = COALESCE(excluded.project_id, sessions.project_id),
                last_activity_at = excluded.last_activity_at,
                status = excluded.status,
                metadata = excluded.metadata
            "#,
            params![
                session.id,
                session.assistant.as_str(),
                session.backing_model_id,
                session.project_id,
                session.started_at.to_rfc3339(),
                session.last_activity_at.map(|t| t.to_rfc3339()),
                session.status.as_str(),
                session.source_file_path,
                session.metadata.to_string(),
            ],
        )?;
        Ok(())
    }

    /// Get a session by ID
    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT * FROM sessions WHERE id = ?", [id], |row| {
            Self::row_to_session(row)
        })
        .optional()
        .map_err(Error::from)
    }

    /// List sessions with optional filtering
    pub fn list_sessions(&self, filter: &SessionFilter) -> Result<Vec<Session>> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from("SELECT * FROM sessions WHERE 1=1");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

        if let Some(assistant) = &filter.assistant {
            sql.push_str(" AND assistant = ?");
            params.push(Box::new(assistant.as_str().to_string()));
        }

        if let Some(status) = &filter.status {
            sql.push_str(" AND status = ?");
            params.push(Box::new(status.as_str().to_string()));
        }

        if let Some(project_id) = &filter.project_id {
            sql.push_str(" AND project_id = ?");
            params.push(Box::new(project_id.clone()));
        }

        if let Some(since) = &filter.since {
            sql.push_str(" AND started_at >= ?");
            params.push(Box::new(since.to_rfc3339()));
        }

        sql.push_str(" ORDER BY last_activity_at DESC NULLS LAST");

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let sessions = stmt
            .query_map(params_refs.as_slice(), Self::row_to_session)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(sessions)
    }

    /// List sessions for a project with summary stats for TUI display.
    ///
    /// Returns sessions with pre-computed thread count, message count, and model name
    /// to avoid N+1 queries when rendering session lists.
    pub fn list_project_sessions(&self, project_id: &str) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock().unwrap();

        // Join sessions with aggregated stats from threads and messages
        let mut stmt = conn.prepare(
            r#"
            SELECT
                s.id,
                s.assistant,
                s.started_at,
                s.last_activity_at,
                COUNT(DISTINCT t.id) as thread_count,
                COUNT(m.id) as message_count,
                bm.display_name as model_name
            FROM sessions s
            LEFT JOIN threads t ON t.session_id = s.id
            LEFT JOIN messages m ON m.session_id = s.id
            LEFT JOIN backing_models bm ON bm.id = s.backing_model_id
            WHERE s.project_id = ?
            GROUP BY s.id
            ORDER BY s.last_activity_at DESC NULLS LAST
            "#,
        )?;

        let summaries = stmt
            .query_map([project_id], |row| {
                let assistant_str: String = row.get(1)?;
                let started_at_str: String = row.get(2)?;
                let last_activity_str: Option<String> = row.get(3)?;

                Ok(SessionSummary {
                    id: row.get(0)?,
                    assistant: assistant_str.parse().unwrap_or(Assistant::ClaudeCode),
                    started_at: DateTime::parse_from_rfc3339(&started_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    last_activity_at: last_activity_str
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    thread_count: row.get(4)?,
                    message_count: row.get(5)?,
                    model_name: row.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(summaries)
    }

    fn row_to_session(row: &Row) -> rusqlite::Result<Session> {
        let assistant_str: String = row.get("assistant")?;
        let status_str: String = row.get("status")?;
        let started_at_str: String = row.get("started_at")?;
        let last_activity_str: Option<String> = row.get("last_activity_at")?;
        let metadata_str: String = row.get("metadata")?;

        Ok(Session {
            id: row.get("id")?,
            assistant: assistant_str.parse().unwrap_or(Assistant::ClaudeCode),
            backing_model_id: row.get("backing_model_id")?,
            project_id: row.get("project_id")?,
            started_at: DateTime::parse_from_rfc3339(&started_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            last_activity_at: last_activity_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            status: status_str.parse().unwrap_or(SessionStatus::Stale),
            source_file_path: row.get("source_file_path")?,
            metadata: serde_json::from_str(&metadata_str).unwrap_or(serde_json::json!({})),
        })
    }

    // ============================================
    // Thread operations
    // ============================================

    /// Insert a thread
    pub fn insert_thread(&self, thread: &Thread) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO threads (id, session_id, thread_type, parent_thread_id,
                                spawned_by_message_id, started_at, ended_at, last_activity_at, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                thread.id,
                thread.session_id,
                thread.thread_type.as_str(),
                thread.parent_thread_id,
                thread.spawned_by_message_id,
                thread.started_at.to_rfc3339(),
                thread.ended_at.map(|t| t.to_rfc3339()),
                thread.last_activity_at.map(|t| t.to_rfc3339()),
                thread.metadata.to_string(),
            ],
        )?;
        Ok(())
    }

    /// Get threads for a session
    pub fn get_session_threads(&self, session_id: &str) -> Result<Vec<Thread>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT * FROM threads WHERE session_id = ? ORDER BY started_at ASC")?;

        let threads = stmt
            .query_map([session_id], Self::row_to_thread)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(threads)
    }

    /// List all threads with message counts and session/project context.
    pub fn list_threads_with_counts(&self) -> Result<Vec<ThreadSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT
                t.*,
                s.assistant,
                p.name as project_name,
                COALESCE(mc.message_count, 0) as message_count
            FROM threads t
            JOIN sessions s ON s.id = t.session_id
            LEFT JOIN projects p ON p.id = s.project_id
            LEFT JOIN (
                SELECT thread_id, COUNT(*) as message_count
                FROM messages
                GROUP BY thread_id
            ) mc ON mc.thread_id = t.id
            ORDER BY t.started_at ASC
            "#,
        )?;

        let threads = stmt
            .query_map([], |row| {
                let assistant_str: String = row.get("assistant")?;
                Ok(ThreadSummary {
                    thread: Self::row_to_thread(row)?,
                    assistant: assistant_str.parse().unwrap_or(Assistant::ClaudeCode),
                    project_name: row.get("project_name")?,
                    message_count: row.get("message_count")?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(threads)
    }

    /// Get a single thread by ID
    pub fn get_thread(&self, thread_id: &str) -> Result<Option<Thread>> {
        let conn = self.conn.lock().unwrap();
        let thread = conn
            .query_row("SELECT * FROM threads WHERE id = ?", [thread_id], |row| {
                Self::row_to_thread(row)
            })
            .optional()?;

        Ok(thread)
    }

    fn row_to_thread(row: &Row) -> rusqlite::Result<Thread> {
        let thread_type_str: String = row.get("thread_type")?;
        let started_at_str: String = row.get("started_at")?;
        let ended_at_str: Option<String> = row.get("ended_at")?;
        let last_activity_at_str: Option<String> = row.get("last_activity_at")?;
        let metadata_str: String = row.get("metadata")?;

        Ok(Thread {
            id: row.get("id")?,
            session_id: row.get("session_id")?,
            thread_type: thread_type_str.parse().unwrap_or(ThreadType::Main),
            parent_thread_id: row.get("parent_thread_id")?,
            spawned_by_message_id: row.get("spawned_by_message_id")?,
            started_at: DateTime::parse_from_rfc3339(&started_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            ended_at: ended_at_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            last_activity_at: last_activity_at_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            metadata: serde_json::from_str(&metadata_str).unwrap_or(serde_json::json!({})),
        })
    }

    /// Update thread spawn info (for linking agent threads to spawning Task calls).
    ///
    /// Sets `spawned_by_message_id` and `parent_thread_id` for the given thread.
    pub fn update_thread_spawn_info(
        &self,
        thread_id: &str,
        spawned_by_message_id: i64,
        parent_thread_id: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            UPDATE threads
            SET spawned_by_message_id = ?1, parent_thread_id = ?2
            WHERE id = ?3
            "#,
            params![spawned_by_message_id, parent_thread_id, thread_id],
        )?;
        Ok(())
    }

    /// Update thread metadata JSON.
    pub fn update_thread_metadata(
        &self,
        thread_id: &str,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            UPDATE threads
            SET metadata = ?1
            WHERE id = ?2
            "#,
            params![metadata.to_string(), thread_id],
        )?;
        Ok(())
    }

    /// Update the last_activity_at timestamp for a thread.
    pub fn update_thread_last_activity(
        &self,
        thread_id: &str,
        last_activity_at: DateTime<Utc>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            UPDATE threads
            SET last_activity_at = ?1
            WHERE id = ?2
            "#,
            params![last_activity_at.to_rfc3339(), thread_id],
        )?;
        Ok(())
    }

    // ============================================
    // Agent spawn operations
    // ============================================

    /// Insert or update agent spawn mapping.
    ///
    /// Used to persist spawn info for incremental parsing - survives across syncs.
    pub fn upsert_agent_spawn(
        &self,
        agent_id: &str,
        session_id: &str,
        spawning_seq: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO agent_spawns (agent_id, session_id, spawning_message_seq, created_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(agent_id) DO UPDATE SET
                session_id = excluded.session_id,
                spawning_message_seq = excluded.spawning_message_seq
            "#,
            params![agent_id, session_id, spawning_seq, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Get spawn info for an agent.
    ///
    /// Returns the session ID and spawning message seq for linking agent threads.
    pub fn get_agent_spawn(&self, agent_id: &str) -> Result<Option<AgentSpawnInfo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT agent_id, session_id, spawning_message_seq FROM agent_spawns WHERE agent_id = ?",
        )?;

        let result = stmt
            .query_row([agent_id], |row| {
                Ok(AgentSpawnInfo {
                    agent_id: row.get(0)?,
                    session_id: row.get(1)?,
                    spawning_message_seq: row.get(2)?,
                })
            })
            .optional()?;

        Ok(result)
    }

    // ============================================
    // Message operations
    // ============================================

    /// Insert a message
    pub fn insert_message(&self, message: &Message) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO messages (session_id, thread_id, seq, emitted_at, observed_at, author_role, author_name,
                                  message_type, content, content_type, tool_name, tool_input, tool_result,
                                  tokens_in, tokens_out, duration_ms, source_file_path,
                                  source_offset, source_line, raw_data, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
            "#,
            params![
                message.session_id,
                message.thread_id,
                message.seq,
                message.emitted_at.to_rfc3339(),
                message.observed_at.to_rfc3339(),
                message.author_role.as_str(),
                message.author_name,
                message.message_type.as_str(),
                message.content,
                message.content_type.as_ref().map(|ct| ct.to_string()),
                message.tool_name,
                message.tool_input.as_ref().map(|v| v.to_string()),
                message.tool_result,
                message.tokens_in,
                message.tokens_out,
                message.duration_ms,
                message.source_file_path,
                message.source_offset,
                message.source_line,
                message.raw_data.to_string(),
                message.metadata.to_string(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Insert multiple messages in a transaction
    pub fn insert_messages(&self, messages: &[Message]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        for message in messages {
            tx.execute(
                r#"
                INSERT INTO messages (session_id, thread_id, seq, emitted_at, observed_at, author_role, author_name,
                                      message_type, content, content_type, tool_name, tool_input, tool_result,
                                      tokens_in, tokens_out, duration_ms, source_file_path,
                                      source_offset, source_line, raw_data, metadata)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
                "#,
                params![
                    message.session_id,
                    message.thread_id,
                    message.seq,
                    message.emitted_at.to_rfc3339(),
                    message.observed_at.to_rfc3339(),
                    message.author_role.as_str(),
                    message.author_name,
                    message.message_type.as_str(),
                    message.content,
                    message.content_type.as_ref().map(|ct| ct.to_string()),
                    message.tool_name,
                    message.tool_input.as_ref().map(|v| v.to_string()),
                    message.tool_result,
                    message.tokens_in,
                    message.tokens_out,
                    message.duration_ms,
                    message.source_file_path,
                    message.source_offset,
                    message.source_line,
                    message.raw_data.to_string(),
                    message.metadata.to_string(),
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Get messages for a session
    pub fn get_session_messages(&self, session_id: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM messages WHERE session_id = ? ORDER BY emitted_at ASC LIMIT ?",
        )?;

        let messages = stmt
            .query_map(params![session_id, limit as i64], |row| {
                Self::row_to_message(row)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(messages)
    }

    /// Count messages for a session
    pub fn count_session_messages(&self, session_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id = ?",
            [session_id],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Count messages for a thread
    pub fn count_thread_messages(&self, thread_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id = ?",
            [thread_id],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Get messages for a thread
    pub fn get_thread_messages(&self, thread_id: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT * FROM messages WHERE thread_id = ? ORDER BY seq ASC LIMIT ?")?;

        let messages = stmt
            .query_map(params![thread_id, limit as i64], |row| {
                Self::row_to_message(row)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(messages)
    }

    /// Get a message from the main thread by session and sequence number.
    pub fn get_main_thread_message_by_seq(
        &self,
        session_id: &str,
        seq: i64,
    ) -> Result<Option<Message>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                r#"
            SELECT m.*
            FROM messages m
            JOIN threads t ON t.id = m.thread_id
            WHERE m.session_id = ?1
              AND m.seq = ?2
              AND t.thread_type = 'main'
            ORDER BY m.id ASC
            LIMIT 1
            "#,
                params![session_id, seq],
                Self::row_to_message,
            )
            .optional()?)
    }

    /// Get the last sequence number for a thread
    pub fn get_last_message_seq(&self, thread_id: &str) -> Result<Option<i32>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT MAX(seq) FROM messages WHERE thread_id = ?",
            [thread_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Error::from)
        .map(|opt| opt.flatten())
    }

    /// Get the last activity timestamp for a thread.
    pub fn get_thread_last_activity(&self, thread_id: &str) -> Result<Option<DateTime<Utc>>> {
        let conn = self.conn.lock().unwrap();
        let result: Option<String> = conn
            .query_row(
                "SELECT last_activity_at FROM threads WHERE id = ?",
                [thread_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::from)?
            .flatten();

        Ok(result.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }))
    }

    /// Get the latest message timestamp in the database.
    /// Used by TUI to detect when new data has been synced.
    /// Note: Uses observed_at since we want to detect new ingestions, not event times.
    pub fn get_latest_message_ts(&self) -> Result<Option<DateTime<Utc>>> {
        let conn = self.conn.lock().unwrap();
        let result: Option<String> = conn
            .query_row("SELECT MAX(observed_at) FROM messages", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(Error::from)?
            .flatten();

        Ok(result.and_then(|observed_at_str| {
            DateTime::parse_from_rfc3339(&observed_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }))
    }

    /// Get the latest message timestamp for a specific session.
    /// Used for analytics freshness checking.
    pub fn get_session_last_message_ts(&self, session_id: &str) -> Result<Option<DateTime<Utc>>> {
        let conn = self.conn.lock().unwrap();
        let result: Option<String> = conn
            .query_row(
                "SELECT MAX(emitted_at) FROM messages WHERE session_id = ?",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::from)?
            .flatten();

        Ok(result.and_then(|emitted_at_str| {
            DateTime::parse_from_rfc3339(&emitted_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }))
    }

    /// Get pre-computed session analytics from plugin_metrics table.
    /// Returns None if no analytics have been computed for this session.
    pub fn get_session_analytics(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::analytics::SessionAnalytics>> {
        let metrics = self.get_session_plugin_metrics(session_id)?;

        // Filter to edit_churn plugin metrics
        let edit_churn_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.plugin_name == "core.edit_churn")
            .collect();

        if edit_churn_metrics.is_empty() {
            return Ok(None);
        }

        // Extract each metric value
        let mut edit_count: i64 = 0;
        let mut unique_files: i64 = 0;
        let mut churn_ratio: f64 = 0.0;
        let mut high_churn_files: Vec<String> = Vec::new();
        let mut computed_at = chrono::Utc::now();

        for metric in &edit_churn_metrics {
            match metric.metric_name.as_str() {
                "edit_count" => {
                    edit_count = metric.metric_value.as_i64().unwrap_or(0);
                }
                "unique_files" => {
                    unique_files = metric.metric_value.as_i64().unwrap_or(0);
                }
                "churn_ratio" => {
                    churn_ratio = metric.metric_value.as_f64().unwrap_or(0.0);
                }
                "high_churn_files" => {
                    if let Some(arr) = metric.metric_value.as_array() {
                        high_churn_files = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                    }
                }
                _ => {}
            }
            // Use the computed_at from any metric (they should all be the same)
            computed_at = metric.computed_at;
        }

        Ok(Some(crate::analytics::SessionAnalytics {
            edit_count,
            unique_files,
            churn_ratio,
            high_churn_files,
            computed_at,
        }))
    }

    /// Get all metrics for a thread across all plugins.
    pub fn get_thread_plugin_metrics(
        &self,
        thread_id: &str,
    ) -> Result<Vec<crate::types::PluginMetric>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            r#"
            SELECT id, plugin_name, entity_type, entity_id, metric_name, metric_value, computed_at
            FROM plugin_metrics
            WHERE entity_type = 'thread' AND entity_id = ?
            ORDER BY plugin_name, metric_name
            "#,
        )?;

        let metrics = stmt
            .query_map([thread_id], |row| {
                let computed_at_str: String = row.get(6)?;
                let metric_value = Self::parse_metric_value(row.get_ref(5)?);
                Ok(crate::types::PluginMetric {
                    id: row.get(0)?,
                    plugin_name: row.get(1)?,
                    entity_type: row.get(2)?,
                    entity_id: row.get(3)?,
                    metric_name: row.get(4)?,
                    metric_value,
                    computed_at: DateTime::parse_from_rfc3339(&computed_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(metrics)
    }

    /// Get pre-computed thread analytics from plugin_metrics table.
    /// Returns None if no analytics have been computed for this thread.
    pub fn get_thread_analytics(
        &self,
        thread_id: &str,
    ) -> Result<Option<crate::analytics::ThreadAnalytics>> {
        let metrics = self.get_thread_plugin_metrics(thread_id)?;

        // Filter to edit_churn plugin metrics
        let edit_churn_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.plugin_name == "core.edit_churn")
            .collect();

        if edit_churn_metrics.is_empty() {
            return Ok(None);
        }

        // Extract each metric value
        let mut edit_count: i64 = 0;
        let mut unique_files: i64 = 0;
        let mut churn_ratio: f64 = 0.0;
        let mut high_churn_files: Vec<String> = Vec::new();
        let mut high_churn_threshold: f64 = 0.0;
        let mut burst_edit_files: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        let mut burst_edit_count: i64 = 0;
        let mut lines_changed: i64 = 0;
        let mut first_try_rate: f64 = 0.0;
        let mut computed_at = chrono::Utc::now();

        for metric in &edit_churn_metrics {
            match metric.metric_name.as_str() {
                "edit_count" => {
                    edit_count = metric.metric_value.as_i64().unwrap_or(0);
                }
                "unique_files" => {
                    unique_files = metric.metric_value.as_i64().unwrap_or(0);
                }
                "churn_ratio" => {
                    churn_ratio = metric.metric_value.as_f64().unwrap_or(0.0);
                }
                "high_churn_files" => {
                    if let Some(arr) = metric.metric_value.as_array() {
                        high_churn_files = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                    }
                }
                "high_churn_threshold" => {
                    high_churn_threshold = metric.metric_value.as_f64().unwrap_or(0.0);
                }
                "burst_edit_files" => {
                    if let Some(obj) = metric.metric_value.as_object() {
                        burst_edit_files = obj
                            .iter()
                            .map(|(k, v)| (k.clone(), v.as_i64().unwrap_or(0)))
                            .collect();
                    }
                }
                "burst_edit_count" => {
                    burst_edit_count = metric.metric_value.as_i64().unwrap_or(0);
                }
                "lines_changed" => {
                    lines_changed = metric.metric_value.as_i64().unwrap_or(0);
                }
                "first_try_rate" => {
                    first_try_rate = metric.metric_value.as_f64().unwrap_or(0.0);
                }
                _ => {}
            }
            // Use the computed_at from any metric (they should all be the same)
            computed_at = metric.computed_at;
        }

        Ok(Some(crate::analytics::ThreadAnalytics {
            edit_count,
            unique_files,
            churn_ratio,
            high_churn_files,
            high_churn_threshold,
            burst_edit_files,
            burst_edit_count,
            lines_changed,
            first_try_rate,
            computed_at,
        }))
    }

    fn row_to_message(row: &Row) -> rusqlite::Result<Message> {
        let author_role_str: String = row.get("author_role")?;
        let message_type_str: String = row.get("message_type")?;
        let emitted_at_str: String = row.get("emitted_at")?;
        let observed_at_str: String = row.get("observed_at")?;
        let content_type_str: Option<String> = row.get("content_type")?;
        let tool_input_str: Option<String> = row.get("tool_input")?;
        let raw_data_str: String = row.get("raw_data")?;
        let metadata_str: String = row.get("metadata")?;

        Ok(Message {
            id: row.get("id")?,
            session_id: row.get("session_id")?,
            thread_id: row.get("thread_id")?,
            seq: row.get("seq")?,
            emitted_at: DateTime::parse_from_rfc3339(&emitted_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            observed_at: DateTime::parse_from_rfc3339(&observed_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            author_role: author_role_str.parse().unwrap_or(AuthorRole::System),
            author_name: row.get("author_name")?,
            message_type: message_type_str.parse().unwrap_or(MessageType::Response),
            content: row.get("content")?,
            content_type: content_type_str.and_then(|s| s.parse().ok()),
            tool_name: row.get("tool_name")?,
            tool_input: tool_input_str.and_then(|s| serde_json::from_str(&s).ok()),
            tool_result: row.get("tool_result")?,
            tokens_in: row.get("tokens_in")?,
            tokens_out: row.get("tokens_out")?,
            duration_ms: row.get("duration_ms")?,
            source_file_path: row.get("source_file_path")?,
            source_offset: row.get("source_offset")?,
            source_line: row.get("source_line")?,
            raw_data: serde_json::from_str(&raw_data_str).unwrap_or(serde_json::json!({})),
            metadata: serde_json::from_str(&metadata_str).unwrap_or(serde_json::json!({})),
        })
    }

    // ============================================
    // Plan operations
    // ============================================

    /// Insert a plan version if content has changed (deduplicated by content hash)
    ///
    /// Returns true if a new version was inserted, false if this content already exists.
    pub fn upsert_plan_version(&self, plan: &Plan) -> Result<bool> {
        let conn = self.conn.lock().unwrap();

        // Extract content hash from metadata
        let content_hash = plan
            .metadata
            .get("content_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Try to insert - will fail silently if (slug, hash) already exists
        let result = conn.execute(
            r#"
            INSERT OR IGNORE INTO plan_versions
                (plan_slug, content_hash, title, content, captured_at, source_file)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                plan.id, // slug is the plan ID
                content_hash,
                plan.title,
                plan.content,
                Utc::now().to_rfc3339(),
                plan.source_file_path,
            ],
        )?;

        Ok(result > 0)
    }

    /// Link a session to a plan slug
    ///
    /// Uses INSERT OR IGNORE to handle duplicates gracefully.
    pub fn link_session_plan(
        &self,
        session_id: &str,
        plan_slug: &str,
        first_used_at: DateTime<Utc>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT OR IGNORE INTO session_plans (session_id, plan_slug, first_used_at)
            VALUES (?1, ?2, ?3)
            "#,
            params![session_id, plan_slug, first_used_at.to_rfc3339(),],
        )?;
        Ok(())
    }

    /// Get the latest version of a plan by slug
    pub fn get_plan_by_slug(&self, slug: &str) -> Result<Option<Plan>> {
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row(
                r#"
                SELECT plan_slug, title, content, captured_at, source_file
                FROM plan_versions
                WHERE plan_slug = ?
                ORDER BY captured_at DESC
                LIMIT 1
                "#,
                [slug],
                |row| {
                    let captured_at_str: String = row.get("captured_at")?;
                    Ok(Plan {
                        id: row.get("plan_slug")?,
                        session_id: String::new(), // Not stored in plan_versions
                        path: PathBuf::from(row.get::<_, String>("source_file")?),
                        title: row.get("title")?,
                        created_at: DateTime::parse_from_rfc3339(&captured_at_str)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                        modified_at: DateTime::parse_from_rfc3339(&captured_at_str)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                        status: PlanStatus::Unknown,
                        content: row.get("content")?,
                        source_file_path: row.get("source_file")?,
                        raw_data: serde_json::json!({}),
                        metadata: serde_json::json!({}),
                    })
                },
            )
            .optional()?;

        Ok(result)
    }

    /// Get all plan slugs for a session
    pub fn get_plan_slugs_for_session(&self, session_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT plan_slug FROM session_plans WHERE session_id = ? ORDER BY first_used_at",
        )?;

        let slugs: Vec<String> = stmt
            .query_map([session_id], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;

        Ok(slugs)
    }

    /// Get all plans for a session (latest version of each)
    pub fn get_plans_for_session(&self, session_id: &str) -> Result<Vec<Plan>> {
        let slugs = self.get_plan_slugs_for_session(session_id)?;
        let mut plans = Vec::new();

        for slug in slugs {
            if let Some(plan) = self.get_plan_by_slug(&slug)? {
                plans.push(plan);
            }
        }

        Ok(plans)
    }

    /// List plans for a project (latest version per plan slug).
    pub fn list_project_plans(&self, project_id: &str) -> Result<Vec<Plan>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT
                sp.session_id,
                pv.plan_slug,
                pv.title,
                pv.content,
                pv.captured_at,
                pv.source_file
            FROM session_plans sp
            JOIN sessions s ON s.id = sp.session_id
            JOIN (
                SELECT plan_slug, MAX(captured_at) as max_captured_at
                FROM plan_versions
                GROUP BY plan_slug
            ) latest ON latest.plan_slug = sp.plan_slug
            JOIN plan_versions pv
              ON pv.plan_slug = latest.plan_slug
             AND pv.captured_at = latest.max_captured_at
            WHERE s.project_id = ?
            ORDER BY pv.captured_at DESC
            "#,
        )?;

        let plans = stmt
            .query_map([project_id], |row| {
                let captured_at_str: String = row.get("captured_at")?;
                let captured_at = DateTime::parse_from_rfc3339(&captured_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let source_file: String = row.get("source_file")?;

                Ok(Plan {
                    id: row.get("plan_slug")?,
                    session_id: row.get("session_id")?,
                    path: PathBuf::from(&source_file),
                    title: row.get("title")?,
                    created_at: captured_at,
                    modified_at: captured_at,
                    status: PlanStatus::Unknown,
                    content: row.get("content")?,
                    source_file_path: source_file,
                    raw_data: serde_json::json!({}),
                    metadata: serde_json::json!({}),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(plans)
    }

    // ============================================
    // Statistics
    // ============================================

    /// Count sessions by status
    pub fn count_sessions_by_status(&self) -> Result<std::collections::HashMap<String, i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT status, COUNT(*) FROM sessions GROUP BY status")?;

        let counts: std::collections::HashMap<String, i64> = stmt
            .query_map([], |row| {
                let status: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((status, count))
            })?
            .collect::<std::result::Result<_, _>>()?;

        Ok(counts)
    }

    /// Count total messages
    pub fn count_messages(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
        Ok(count)
    }

    // Backward compatibility alias
    pub fn count_events(&self) -> Result<i64> {
        self.count_messages()
    }

    // ============================================
    // Plugin Metrics operations
    // ============================================

    /// Insert or update a plugin metric.
    ///
    /// Metrics are upserted based on (plugin_name, entity_type, entity_id, metric_name).
    /// This makes plugin runs idempotent.
    pub fn insert_plugin_metric(
        &self,
        plugin_name: &str,
        entity_type: &str,
        entity_id: Option<&str>,
        metric_name: &str,
        metric_value: &serde_json::Value,
        metric_version: i32,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO plugin_metrics (plugin_name, entity_type, entity_id, metric_name, metric_value, metric_version, computed_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(plugin_name, entity_type, entity_id, metric_name) DO UPDATE SET
                metric_value = excluded.metric_value,
                metric_version = excluded.metric_version,
                computed_at = excluded.computed_at
            "#,
            params![
                plugin_name,
                entity_type,
                entity_id,
                metric_name,
                // Use serde_json::to_string to ensure valid JSON text representation
                // This wraps strings in quotes and ensures consistent TEXT storage
                serde_json::to_string(metric_value).unwrap_or_else(|_| "null".to_string()),
                metric_version,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Get all metrics for a specific entity from a plugin.
    pub fn get_plugin_metrics(
        &self,
        plugin_name: &str,
        entity_type: &str,
        entity_id: Option<&str>,
    ) -> Result<Vec<crate::types::PluginMetric>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            r#"
            SELECT id, plugin_name, entity_type, entity_id, metric_name, metric_value, computed_at
            FROM plugin_metrics
            WHERE plugin_name = ?1
              AND entity_type = ?2
              AND ((?3 IS NULL AND entity_id IS NULL) OR entity_id = ?3)
            "#,
        )?;

        let metrics = stmt
            .query_map(params![plugin_name, entity_type, entity_id], |row| {
                let computed_at_str: String = row.get(6)?;
                // Handle different SQLite storage types for metric_value
                let metric_value = Self::parse_metric_value(row.get_ref(5)?);
                Ok(crate::types::PluginMetric {
                    id: row.get(0)?,
                    plugin_name: row.get(1)?,
                    entity_type: row.get(2)?,
                    entity_id: row.get(3)?,
                    metric_name: row.get(4)?,
                    metric_value,
                    computed_at: DateTime::parse_from_rfc3339(&computed_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(metrics)
    }

    /// Parse a metric value from SQLite's dynamic type into JSON.
    ///
    /// SQLite may store JSON values as INTEGER, REAL, or TEXT depending on the value.
    /// This function handles all cases and returns a serde_json::Value.
    fn parse_metric_value(value_ref: rusqlite::types::ValueRef<'_>) -> serde_json::Value {
        match value_ref {
            rusqlite::types::ValueRef::Null => serde_json::json!(null),
            rusqlite::types::ValueRef::Integer(i) => serde_json::json!(i),
            rusqlite::types::ValueRef::Real(f) => serde_json::json!(f),
            rusqlite::types::ValueRef::Text(s) => {
                let s = std::str::from_utf8(s).unwrap_or("null");
                serde_json::from_str(s).unwrap_or(serde_json::json!(null))
            }
            rusqlite::types::ValueRef::Blob(_) => serde_json::json!(null),
        }
    }

    /// Get all metrics for a session across all plugins.
    pub fn get_session_plugin_metrics(
        &self,
        session_id: &str,
    ) -> Result<Vec<crate::types::PluginMetric>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            r#"
            SELECT id, plugin_name, entity_type, entity_id, metric_name, metric_value, computed_at
            FROM plugin_metrics
            WHERE entity_type = 'session' AND entity_id = ?
            ORDER BY plugin_name, metric_name
            "#,
        )?;

        let metrics = stmt
            .query_map([session_id], |row| {
                let computed_at_str: String = row.get(6)?;
                let metric_value = Self::parse_metric_value(row.get_ref(5)?);
                Ok(crate::types::PluginMetric {
                    id: row.get(0)?,
                    plugin_name: row.get(1)?,
                    entity_type: row.get(2)?,
                    entity_id: row.get(3)?,
                    metric_name: row.get(4)?,
                    metric_value,
                    computed_at: DateTime::parse_from_rfc3339(&computed_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(metrics)
    }

    /// Insert a plugin run record for observability.
    ///
    /// Returns the ID of the inserted record.
    pub fn insert_plugin_run(&self, run: &crate::analytics::PluginRunResult) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO plugin_runs (plugin_name, session_id, started_at, duration_ms, status, error_message, metrics_produced, input_message_count, input_token_count)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                run.plugin_name,
                run.session_id,
                run.started_at.to_rfc3339(),
                run.duration_ms,
                run.status.as_str(),
                run.error_message,
                run.metrics_produced as i64,
                run.input_message_count as i64,
                run.input_token_count,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get recent plugin runs for a specific plugin.
    pub fn get_plugin_runs(
        &self,
        plugin_name: &str,
        limit: usize,
    ) -> Result<Vec<crate::analytics::PluginRunResult>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            r#"
            SELECT plugin_name, session_id, started_at, duration_ms, status, error_message, metrics_produced, input_message_count, input_token_count
            FROM plugin_runs
            WHERE plugin_name = ?
            ORDER BY started_at DESC
            LIMIT ?
            "#,
        )?;

        let runs = stmt
            .query_map(params![plugin_name, limit as i64], |row| {
                let started_at_str: String = row.get(2)?;
                let status_str: String = row.get(4)?;
                Ok(crate::analytics::PluginRunResult {
                    plugin_name: row.get(0)?,
                    session_id: row.get(1)?,
                    started_at: DateTime::parse_from_rfc3339(&started_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    duration_ms: row.get(3)?,
                    status: if status_str == "success" {
                        crate::analytics::PluginRunStatus::Success
                    } else {
                        crate::analytics::PluginRunStatus::Error
                    },
                    error_message: row.get(5)?,
                    metrics_produced: row.get::<_, i64>(6)? as usize,
                    input_message_count: row.get::<_, i64>(7)? as usize,
                    input_token_count: row.get(8)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(runs)
    }

    /// Get plugin run statistics for observability.
    ///
    /// Returns (success_count, error_count, avg_duration_ms) for each plugin.
    pub fn get_plugin_stats(&self) -> Result<Vec<(String, i64, i64, f64)>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            r#"
            SELECT
                plugin_name,
                SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) as success_count,
                SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) as error_count,
                AVG(duration_ms) as avg_duration
            FROM plugin_runs
            GROUP BY plugin_name
            ORDER BY plugin_name
            "#,
        )?;

        let stats = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(stats)
    }

    // ============================================
    // Metadata/Stats operations (for TUI)
    // ============================================

    /// Get the source file path for a session
    pub fn get_session_source_path(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let path: Option<String> = conn
            .query_row(
                "SELECT source_file_path FROM sessions WHERE id = ?",
                [session_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(path)
    }

    /// Get the backing model display name for a session
    pub fn get_session_model_name(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let name: Option<String> = conn
            .query_row(
                r#"
                SELECT bm.display_name
                FROM sessions s
                JOIN backing_models bm ON s.backing_model_id = bm.id
                WHERE s.id = ?
                "#,
                [session_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(name)
    }

    /// Get session metadata (cwd, git_branch, etc.)
    pub fn get_session_metadata(&self, session_id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let metadata: Option<String> = conn
            .query_row(
                "SELECT metadata FROM sessions WHERE id = ?",
                [session_id],
                |r| r.get(0),
            )
            .optional()?;
        match metadata {
            Some(s) => Ok(Some(serde_json::from_str(&s).unwrap_or_default())),
            None => Ok(None),
        }
    }

    /// Count agent threads for a session
    pub fn count_session_agents(&self, session_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM threads WHERE session_id = ? AND thread_type = 'agent'",
            [session_id],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Count plans for a session
    pub fn count_session_plans(&self, session_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_plans WHERE session_id = ?",
            [session_id],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Get tool usage statistics for a thread
    pub fn get_thread_tool_stats(&self, thread_id: &str) -> Result<ToolStats> {
        let conn = self.conn.lock().unwrap();

        // Get total tool calls
        let total_calls: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id = ? AND message_type = 'tool_call'",
            [thread_id],
            |r| r.get(0),
        )?;

        // Get breakdown by tool name
        let mut stmt = conn.prepare(
            r#"
            SELECT tool_name, COUNT(*) as cnt
            FROM messages
            WHERE thread_id = ? AND message_type = 'tool_call' AND tool_name IS NOT NULL
            GROUP BY tool_name
            ORDER BY cnt DESC
            "#,
        )?;
        let breakdown: Vec<(String, i64)> = stmt
            .query_map([thread_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(ToolStats {
            total_calls,
            breakdown,
        })
    }

    /// Get file modification statistics for a thread (from Edit/Write tool_input)
    pub fn get_thread_file_stats(&self, thread_id: &str) -> Result<FileStats> {
        let conn = self.conn.lock().unwrap();

        // Query all Edit/Write tool calls and extract file_path from JSON
        let mut stmt = conn.prepare(
            r#"
            SELECT json_extract(tool_input, '$.file_path') as file_path
            FROM messages
            WHERE thread_id = ?
              AND message_type = 'tool_call'
              AND tool_name IN ('Edit', 'Write', 'MultiEdit')
              AND tool_input IS NOT NULL
            "#,
        )?;

        // Count files - use HashMap to aggregate
        let mut file_counts: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();

        let rows = stmt.query_map([thread_id], |row| row.get::<_, Option<String>>(0))?;
        for row in rows {
            if let Ok(Some(path)) = row {
                *file_counts.entry(path).or_insert(0) += 1;
            }
        }

        let total_files = file_counts.len() as i64;

        // Sort by count descending
        let mut breakdown: Vec<(String, i64)> = file_counts.into_iter().collect();
        breakdown.sort_by(|a, b| b.1.cmp(&a.1));

        Ok(FileStats {
            total_files,
            breakdown,
        })
    }

    /// Get thread metadata for detail views.
    pub fn get_thread_metadata(&self, thread_id: &str) -> Result<Option<ThreadMetadata>> {
        struct ThreadMetadataRow {
            session_id: String,
            source_path: String,
            model_name: Option<String>,
            metadata: Option<String>,
            started_at: String,
            last_activity_at: Option<String>,
        }

        let conn = self.conn.lock().unwrap();
        let result: Option<ThreadMetadataRow> = conn
            .query_row(
                r#"
                SELECT
                    t.session_id,
                    s.source_file_path,
                    bm.display_name as model_name,
                    s.metadata,
                    s.started_at,
                    s.last_activity_at
                FROM threads t
                JOIN sessions s ON s.id = t.session_id
                LEFT JOIN backing_models bm ON bm.id = s.backing_model_id
                WHERE t.id = ?
                "#,
                [thread_id],
                |row| {
                    Ok(ThreadMetadataRow {
                        session_id: row.get(0)?,
                        source_path: row.get(1)?,
                        model_name: row.get(2)?,
                        metadata: row.get(3)?,
                        started_at: row.get(4)?,
                        last_activity_at: row.get(5)?,
                    })
                },
            )
            .optional()?;

        let Some(row) = result else {
            return Ok(None);
        };

        let metadata: serde_json::Value = row
            .metadata
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::json!({}));
        let cwd = metadata
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(String::from);
        let git_branch = metadata
            .get("git_branch")
            .and_then(|v| v.as_str())
            .map(String::from);

        let started_at = DateTime::parse_from_rfc3339(&row.started_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let last_activity = row.last_activity_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        });
        let duration_secs = last_activity
            .map(|last| last.signed_duration_since(started_at).num_seconds().max(0))
            .unwrap_or(0);

        let message_count = self.count_thread_messages(thread_id).unwrap_or(0);
        let agent_count = self.count_session_agents(&row.session_id).unwrap_or(0);
        let tool_stats = self.get_thread_tool_stats(thread_id).unwrap_or_default();
        let plan_count = self.count_session_plans(&row.session_id).unwrap_or(0);
        let file_stats = self.get_thread_file_stats(thread_id).unwrap_or_default();

        Ok(Some(ThreadMetadata {
            source_path: Some(row.source_path),
            cwd,
            git_branch,
            model_name: row.model_name,
            duration_secs,
            message_count,
            agent_count,
            tool_stats,
            plan_count,
            file_stats,
        }))
    }

    /// Get session timestamps for duration calculation
    #[allow(clippy::type_complexity)]
    pub fn get_session_timestamps(
        &self,
        session_id: &str,
    ) -> Result<Option<(DateTime<Utc>, Option<DateTime<Utc>>)>> {
        let conn = self.conn.lock().unwrap();
        let result: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT started_at, last_activity_at FROM sessions WHERE id = ?",
                [session_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;

        match result {
            Some((started_str, last_str)) => {
                let started = DateTime::parse_from_rfc3339(&started_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| Error::Parse {
                        agent: "session".to_string(),
                        message: format!("Invalid timestamp: {}", e),
                    })?;
                let last = last_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                });
                Ok(Some((started, last)))
            }
            None => Ok(None),
        }
    }

    // ============================================
    // Wrapped Analytics Queries
    // ============================================

    /// Get aggregate totals for a time period (for Wrapped).
    pub fn get_wrapped_totals(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<crate::analytics::TotalStats> {
        let conn = self.conn.lock().unwrap();
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();

        // Sessions and duration
        let (sessions, total_duration_secs): (i64, i64) = conn
            .query_row(
                r#"
                SELECT
                    COUNT(*),
                    COALESCE(SUM(
                        CASE WHEN last_activity_at IS NOT NULL
                        THEN (julianday(last_activity_at) - julianday(started_at)) * 86400
                        ELSE 0 END
                    ), 0)
                FROM sessions
                WHERE started_at >= ? AND started_at < ?
                "#,
                [&start_str, &end_str],
                |r| Ok((r.get(0)?, r.get::<_, f64>(1)? as i64)),
            )
            .unwrap_or((0, 0));

        // Tokens and tool calls from messages
        let (tokens_in, tokens_out, tool_calls): (i64, i64, i64) = conn
            .query_row(
                r#"
                SELECT
                    COALESCE(SUM(tokens_in), 0),
                    COALESCE(SUM(tokens_out), 0),
                    COALESCE(SUM(CASE WHEN message_type = 'tool_call' THEN 1 ELSE 0 END), 0)
                FROM messages m
                JOIN threads t ON m.thread_id = t.id
                JOIN sessions s ON t.session_id = s.id
                WHERE s.started_at >= ? AND s.started_at < ?
                "#,
                [&start_str, &end_str],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        // Plans
        let plans: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(DISTINCT sp.plan_id)
                FROM session_plans sp
                JOIN sessions s ON sp.session_id = s.id
                WHERE s.started_at >= ? AND s.started_at < ?
                "#,
                [&start_str, &end_str],
                |r| r.get(0),
            )
            .unwrap_or(0);

        // Agents spawned
        let agents_spawned: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM threads t
                JOIN sessions s ON t.session_id = s.id
                WHERE t.thread_type = 'agent'
                  AND s.started_at >= ? AND s.started_at < ?
                "#,
                [&start_str, &end_str],
                |r| r.get(0),
            )
            .unwrap_or(0);

        // Files modified (from Edit/Write tool_input)
        let files_modified: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(DISTINCT json_extract(tool_input, '$.file_path'))
                FROM messages m
                JOIN threads t ON m.thread_id = t.id
                JOIN sessions s ON t.session_id = s.id
                WHERE s.started_at >= ? AND s.started_at < ?
                  AND m.message_type = 'tool_call'
                  AND m.tool_name IN ('Edit', 'Write', 'MultiEdit')
                  AND json_extract(tool_input, '$.file_path') IS NOT NULL
                "#,
                [&start_str, &end_str],
                |r| r.get(0),
            )
            .unwrap_or(0);

        // Unique projects
        let unique_projects: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(DISTINCT project_id)
                FROM sessions
                WHERE started_at >= ? AND started_at < ?
                  AND project_id IS NOT NULL
                "#,
                [&start_str, &end_str],
                |r| r.get(0),
            )
            .unwrap_or(0);

        Ok(crate::analytics::TotalStats {
            sessions,
            total_duration_secs,
            tokens_in,
            tokens_out,
            tool_calls,
            plans,
            agents_spawned,
            files_modified,
            unique_projects,
        })
    }

    /// Get tool usage rankings for a time period.
    pub fn get_wrapped_tool_rankings(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();

        let mut stmt = conn.prepare(
            r#"
            SELECT m.tool_name, COUNT(*) as cnt
            FROM messages m
            JOIN threads t ON m.thread_id = t.id
            JOIN sessions s ON t.session_id = s.id
            WHERE s.started_at >= ? AND s.started_at < ?
              AND m.message_type = 'tool_call'
              AND m.tool_name IS NOT NULL
            GROUP BY m.tool_name
            ORDER BY cnt DESC
            LIMIT ?
            "#,
        )?;

        let rows = stmt
            .query_map(params![&start_str, &end_str, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// Get hourly activity distribution for a time period.
    pub fn get_wrapped_hourly_distribution(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<[i64; 24]> {
        let conn = self.conn.lock().unwrap();
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();

        let mut distribution = [0i64; 24];

        let mut stmt = conn.prepare(
            r#"
            SELECT CAST(strftime('%H', emitted_at) AS INTEGER) as hour, COUNT(*) as cnt
            FROM messages m
            JOIN threads t ON m.thread_id = t.id
            JOIN sessions s ON t.session_id = s.id
            WHERE s.started_at >= ? AND s.started_at < ?
            GROUP BY hour
            "#,
        )?;

        let rows = stmt.query_map([&start_str, &end_str], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;

        for row in rows.flatten() {
            let (hour, count) = row;
            if (0..24).contains(&hour) {
                distribution[hour as usize] = count;
            }
        }

        Ok(distribution)
    }

    /// Get daily activity distribution for a time period (0=Sunday).
    pub fn get_wrapped_daily_distribution(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<[i64; 7]> {
        let conn = self.conn.lock().unwrap();
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();

        let mut distribution = [0i64; 7];

        let mut stmt = conn.prepare(
            r#"
            SELECT CAST(strftime('%w', emitted_at) AS INTEGER) as dow, COUNT(*) as cnt
            FROM messages m
            JOIN threads t ON m.thread_id = t.id
            JOIN sessions s ON t.session_id = s.id
            WHERE s.started_at >= ? AND s.started_at < ?
            GROUP BY dow
            "#,
        )?;

        let rows = stmt.query_map([&start_str, &end_str], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;

        for row in rows.flatten() {
            let (dow, count) = row;
            if (0..7).contains(&dow) {
                distribution[dow as usize] = count;
            }
        }

        Ok(distribution)
    }

    /// Get project rankings for a time period.
    pub fn get_wrapped_project_rankings(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<crate::analytics::ProjectRanking>> {
        let conn = self.conn.lock().unwrap();
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();

        let mut stmt = conn.prepare(
            r#"
            SELECT
                COALESCE(p.name, '(no project)') as name,
                COUNT(DISTINCT s.id) as sessions,
                COALESCE(SUM(m.tokens_in + m.tokens_out), 0) as tokens,
                COALESCE(SUM(
                    CASE WHEN s.last_activity_at IS NOT NULL
                    THEN (julianday(s.last_activity_at) - julianday(s.started_at)) * 86400
                    ELSE 0 END
                ), 0) as duration,
                MIN(s.started_at) as first_session
            FROM sessions s
            LEFT JOIN projects p ON s.project_id = p.id
            LEFT JOIN threads t ON t.session_id = s.id
            LEFT JOIN messages m ON m.thread_id = t.id
            WHERE s.started_at >= ? AND s.started_at < ?
            GROUP BY COALESCE(p.id, 'none')
            ORDER BY tokens DESC
            LIMIT ?
            "#,
        )?;

        let rows: Vec<crate::analytics::ProjectRanking> = stmt
            .query_map(params![&start_str, &end_str, limit as i64], |row| {
                let first_session_str: Option<String> = row.get(4)?;
                let first_session = first_session_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                });
                Ok(crate::analytics::ProjectRanking {
                    name: row.get(0)?,
                    sessions: row.get(1)?,
                    tokens: row.get(2)?,
                    duration_secs: row.get::<_, f64>(3)? as i64,
                    files_modified: 0, // Would need a subquery
                    first_session,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// Get the longest (marathon) session in a time period.
    ///
    /// This calculates the longest single-day coding session by measuring the
    /// time span from first to last message on each day, rather than the full
    /// session duration which can span multiple days.
    pub fn get_wrapped_marathon_session(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Option<crate::analytics::MarathonSession>> {
        let conn = self.conn.lock().unwrap();
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();

        // Find the longest single-day coding session by grouping messages by day
        // and calculating the span from first to last message within that day
        let result: Option<(String, f64, String, Option<String>, i64, i64)> = conn
            .query_row(
                r#"
                WITH daily_sessions AS (
                    SELECT
                        s.id as session_id,
                        p.name as project_name,
                        date(m.emitted_at) as session_date,
                        MIN(m.emitted_at) as first_msg,
                        MAX(m.emitted_at) as last_msg,
                        (julianday(MAX(m.emitted_at)) - julianday(MIN(m.emitted_at))) * 86400 as duration_secs,
                        COUNT(CASE WHEN m.message_type = 'tool_call' THEN 1 END) as tool_calls,
                        COALESCE(SUM(m.tokens_in + m.tokens_out), 0) as tokens
                    FROM messages m
                    JOIN threads t ON m.thread_id = t.id
                    JOIN sessions s ON t.session_id = s.id
                    LEFT JOIN projects p ON s.project_id = p.id
                    WHERE m.emitted_at >= ? AND m.emitted_at < ?
                    GROUP BY s.id, date(m.emitted_at)
                    HAVING COUNT(*) > 1
                )
                SELECT
                    session_id,
                    duration_secs,
                    first_msg,
                    project_name,
                    tool_calls,
                    tokens
                FROM daily_sessions
                ORDER BY duration_secs DESC
                LIMIT 1
                "#,
                [&start_str, &end_str],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()?;

        match result {
            Some((session_id, duration, date_str, project_name, tool_calls, tokens)) => {
                let date = DateTime::parse_from_rfc3339(&date_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(Some(crate::analytics::MarathonSession {
                    session_id,
                    duration_secs: duration as i64,
                    date,
                    project_name,
                    tool_calls,
                    tokens,
                    files_modified: 0, // Would need additional query
                }))
            }
            None => Ok(None),
        }
    }

    /// Get streak statistics for a time period.
    pub fn get_wrapped_streak_stats(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<crate::analytics::StreakStats> {
        let conn = self.conn.lock().unwrap();
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();

        // Get all unique dates with activity
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT date(started_at) as activity_date
            FROM sessions
            WHERE started_at >= ? AND started_at < ?
            ORDER BY activity_date
            "#,
        )?;

        let dates: Vec<String> = stmt
            .query_map([&start_str, &end_str], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let active_days = dates.len() as i64;
        let total_days = (end - start).num_days();

        // Calculate longest streak
        let mut longest_streak = 0i64;
        let mut longest_start: Option<DateTime<Utc>> = None;
        let mut longest_end: Option<DateTime<Utc>> = None;
        let mut current_streak = 0i64;
        let mut current_start: Option<chrono::NaiveDate> = None;
        let mut prev_date: Option<chrono::NaiveDate> = None;

        for date_str in &dates {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                match prev_date {
                    Some(prev) if (date - prev).num_days() == 1 => {
                        // Consecutive day
                        current_streak += 1;
                    }
                    _ => {
                        // New streak
                        if current_streak > longest_streak {
                            longest_streak = current_streak;
                            longest_start =
                                current_start.map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc());
                            longest_end =
                                prev_date.map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc());
                        }
                        current_streak = 1;
                        current_start = Some(date);
                    }
                }
                prev_date = Some(date);
            }
        }

        // Check final streak
        if current_streak > longest_streak {
            longest_streak = current_streak;
            longest_start = current_start.map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc());
            longest_end = prev_date.map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc());
        }

        // Calculate current streak (days from today going backwards)
        let today = Utc::now().date_naive();
        let mut current_streak_days = 0i64;

        for date_str in dates.iter().rev() {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                let days_ago = (today - date).num_days();
                if days_ago == current_streak_days {
                    current_streak_days += 1;
                } else {
                    break;
                }
            }
        }

        Ok(crate::analytics::StreakStats {
            current_streak_days,
            longest_streak_days: longest_streak,
            longest_streak_start: longest_start,
            longest_streak_end: longest_end,
            active_days,
            total_days,
        })
    }

    // ============================================
    // Project Analytics Queries (for TUI Project View)
    // ============================================

    /// List all projects with summary stats for the project list view.
    pub fn list_projects_with_stats(&self) -> Result<Vec<crate::analytics::ProjectRow>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            r#"
            SELECT
                p.id,
                p.name,
                p.path,
                COUNT(DISTINCT s.id) as session_count,
                MAX(s.last_activity_at) as last_activity,
                COALESCE(SUM(m.tokens_in + m.tokens_out), 0) as total_tokens
            FROM projects p
            LEFT JOIN sessions s ON s.project_id = p.id
            LEFT JOIN threads t ON t.session_id = s.id
            LEFT JOIN messages m ON m.thread_id = t.id
            GROUP BY p.id
            ORDER BY last_activity DESC NULLS LAST
            "#,
        )?;

        let rows: Vec<crate::analytics::ProjectRow> = stmt
            .query_map([], |row| {
                let last_activity_str: Option<String> = row.get(4)?;
                let last_activity = last_activity_str
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                Ok(crate::analytics::ProjectRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    session_count: row.get(3)?,
                    last_activity,
                    total_tokens: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// Get detailed stats for a single project.
    pub fn get_project_stats(
        &self,
        project_id: &str,
    ) -> Result<Option<crate::analytics::ProjectStats>> {
        let conn = self.conn.lock().unwrap();

        // First, get the project info
        let project_info: Option<(String, String, String)> = conn
            .query_row(
                "SELECT id, name, path FROM projects WHERE id = ?",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let (id, name, path) = match project_info {
            Some(info) => info,
            None => return Ok(None),
        };

        // Activity summary
        let (session_count, total_duration_secs, first_session_str, last_activity_str): (
            i64,
            f64,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                r#"
                SELECT
                    COUNT(*),
                    COALESCE(SUM(
                        CASE WHEN last_activity_at IS NOT NULL AND started_at IS NOT NULL
                        THEN MAX(0, (julianday(last_activity_at) - julianday(started_at)) * 86400)
                        ELSE 0 END
                    ), 0),
                    MIN(started_at),
                    MAX(last_activity_at)
                FROM sessions
                WHERE project_id = ?
                "#,
                [project_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap_or((0, 0.0, None, None));

        let first_session = first_session_str
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let last_activity = last_activity_str
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        // Thread and message counts
        let (thread_count, message_count): (i64, i64) = conn
            .query_row(
                r#"
                SELECT
                    COUNT(DISTINCT t.id),
                    COUNT(m.id)
                FROM threads t
                JOIN sessions s ON t.session_id = s.id
                LEFT JOIN messages m ON m.thread_id = t.id
                WHERE s.project_id = ?
                "#,
                [project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));

        // Token usage
        let (tokens_in, tokens_out): (i64, i64) = conn
            .query_row(
                r#"
                SELECT
                    COALESCE(SUM(m.tokens_in), 0),
                    COALESCE(SUM(m.tokens_out), 0)
                FROM messages m
                JOIN threads t ON m.thread_id = t.id
                JOIN sessions s ON t.session_id = s.id
                WHERE s.project_id = ?
                "#,
                [project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));

        // Tool stats
        let total_calls: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM messages m
                JOIN threads t ON m.thread_id = t.id
                JOIN sessions s ON t.session_id = s.id
                WHERE s.project_id = ? AND m.message_type = 'tool_call'
                "#,
                [project_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let mut tool_stmt = conn.prepare(
            r#"
            SELECT m.tool_name, COUNT(*) as cnt
            FROM messages m
            JOIN threads t ON m.thread_id = t.id
            JOIN sessions s ON t.session_id = s.id
            WHERE s.project_id = ? AND m.message_type = 'tool_call' AND m.tool_name IS NOT NULL
            GROUP BY m.tool_name
            ORDER BY cnt DESC
            "#,
        )?;
        let tool_breakdown: Vec<(String, i64)> = tool_stmt
            .query_map([project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let tool_stats = ToolStats {
            total_calls,
            breakdown: tool_breakdown,
        };

        // File stats
        let mut file_stmt = conn.prepare(
            r#"
            SELECT json_extract(m.tool_input, '$.file_path') as file_path
            FROM messages m
            JOIN threads t ON m.thread_id = t.id
            JOIN sessions s ON t.session_id = s.id
            WHERE s.project_id = ?
              AND m.message_type = 'tool_call'
              AND m.tool_name IN ('Edit', 'Write', 'MultiEdit')
              AND m.tool_input IS NOT NULL
            "#,
        )?;

        let mut file_counts: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        let rows = file_stmt.query_map([project_id], |row| row.get::<_, Option<String>>(0))?;
        for row in rows {
            if let Ok(Some(path)) = row {
                *file_counts.entry(path).or_insert(0) += 1;
            }
        }

        let total_files = file_counts.len() as i64;
        let mut file_breakdown: Vec<(String, i64)> = file_counts.into_iter().collect();
        file_breakdown.sort_by(|a, b| b.1.cmp(&a.1));

        let file_stats = FileStats {
            total_files,
            breakdown: file_breakdown,
        };

        // Agents spawned
        let agents_spawned: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM threads t
                JOIN sessions s ON t.session_id = s.id
                WHERE s.project_id = ? AND t.thread_type = 'agent'
                "#,
                [project_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        // Plans created
        let plans_created: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(DISTINCT sp.plan_slug)
                FROM session_plans sp
                JOIN sessions s ON sp.session_id = s.id
                WHERE s.project_id = ?
                "#,
                [project_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        // Hourly distribution
        let mut hourly = [0i64; 24];
        let mut hourly_stmt = conn.prepare(
            r#"
            SELECT CAST(strftime('%H', m.emitted_at) AS INTEGER) as hour, COUNT(*) as cnt
            FROM messages m
            JOIN threads t ON m.thread_id = t.id
            JOIN sessions s ON t.session_id = s.id
            WHERE s.project_id = ?
            GROUP BY hour
            "#,
        )?;
        let hourly_rows = hourly_stmt.query_map([project_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in hourly_rows.flatten() {
            let (hour, count) = row;
            if (0..24).contains(&hour) {
                hourly[hour as usize] = count;
            }
        }

        // Daily distribution
        let mut daily = [0i64; 7];
        let mut daily_stmt = conn.prepare(
            r#"
            SELECT CAST(strftime('%w', m.emitted_at) AS INTEGER) as dow, COUNT(*) as cnt
            FROM messages m
            JOIN threads t ON m.thread_id = t.id
            JOIN sessions s ON t.session_id = s.id
            WHERE s.project_id = ?
            GROUP BY dow
            "#,
        )?;
        let daily_rows = daily_stmt.query_map([project_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in daily_rows.flatten() {
            let (dow, count) = row;
            if (0..7).contains(&dow) {
                daily[dow as usize] = count;
            }
        }

        // Sessions by assistant
        let mut assistant_stmt = conn.prepare(
            r#"
            SELECT assistant, COUNT(*) as cnt
            FROM sessions
            WHERE project_id = ?
            GROUP BY assistant
            ORDER BY cnt DESC
            "#,
        )?;
        let sessions_by_assistant: Vec<(String, i64)> = assistant_stmt
            .query_map([project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(Some(crate::analytics::ProjectStats {
            id,
            name,
            path,
            session_count,
            thread_count,
            message_count,
            total_duration_secs: total_duration_secs as i64,
            tokens_in,
            tokens_out,
            tool_stats,
            file_stats,
            agents_spawned,
            plans_created,
            hourly_distribution: hourly,
            daily_distribution: daily,
            first_session,
            last_activity,
            sessions_by_assistant,
        }))
    }

    /// Get usage profile for personality classification.
    pub fn get_wrapped_usage_profile(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<crate::analytics::personality::UsageProfile> {
        let conn = self.conn.lock().unwrap();
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();

        // Tool counts
        let (read_count, edit_count, bash_count, total_tools): (i64, i64, i64, i64) = conn
            .query_row(
                r#"
                SELECT
                    SUM(CASE WHEN tool_name = 'Read' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN tool_name IN ('Edit', 'MultiEdit', 'Write') THEN 1 ELSE 0 END),
                    SUM(CASE WHEN tool_name = 'Bash' THEN 1 ELSE 0 END),
                    COUNT(*)
                FROM messages m
                JOIN threads t ON m.thread_id = t.id
                JOIN sessions s ON t.session_id = s.id
                WHERE s.started_at >= ? AND s.started_at < ?
                  AND m.message_type = 'tool_call'
                "#,
                [&start_str, &end_str],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap_or((0, 0, 0, 0));

        // Sessions and agents
        let (sessions, agents): (i64, i64) = conn
            .query_row(
                r#"
                SELECT
                    (SELECT COUNT(*) FROM sessions WHERE started_at >= ? AND started_at < ?),
                    (SELECT COUNT(*) FROM threads t
                     JOIN sessions s ON t.session_id = s.id
                     WHERE t.thread_type = 'agent'
                       AND s.started_at >= ? AND s.started_at < ?)
                "#,
                [&start_str, &end_str, &start_str, &end_str],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));

        // Average session duration
        let avg_duration: f64 = conn
            .query_row(
                r#"
                SELECT AVG((julianday(last_activity_at) - julianday(started_at)) * 86400)
                FROM sessions
                WHERE started_at >= ? AND started_at < ?
                  AND last_activity_at IS NOT NULL
                "#,
                [&start_str, &end_str],
                |r| r.get(0),
            )
            .unwrap_or(0.0);

        // Plans
        let plans: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(DISTINCT sp.plan_id)
                FROM session_plans sp
                JOIN sessions s ON sp.session_id = s.id
                WHERE s.started_at >= ? AND s.started_at < ?
                "#,
                [&start_str, &end_str],
                |r| r.get(0),
            )
            .unwrap_or(0);

        // Time distribution for night owl / early bird
        // Inline the query to avoid deadlock (can't call get_wrapped_hourly_distribution while holding lock)
        let mut hourly = [0i64; 24];
        {
            let mut stmt = conn.prepare(
                r#"
                SELECT CAST(strftime('%H', emitted_at) AS INTEGER) as hour, COUNT(*) as cnt
                FROM messages m
                JOIN threads t ON m.thread_id = t.id
                JOIN sessions s ON t.session_id = s.id
                WHERE s.started_at >= ? AND s.started_at < ?
                GROUP BY hour
                "#,
            )?;
            let rows = stmt.query_map([&start_str, &end_str], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in rows.flatten() {
                let (hour, count) = row;
                if (0..24).contains(&hour) {
                    hourly[hour as usize] = count;
                }
            }
        }
        let total_activity: i64 = hourly.iter().sum();
        let late_night: i64 = hourly[22..24].iter().sum::<i64>() + hourly[0..4].iter().sum::<i64>();
        let early_morning: i64 = hourly[5..9].iter().sum();

        // Projects
        let (unique_projects, top_project_sessions): (i64, i64) = conn
            .query_row(
                r#"
                SELECT
                    COUNT(DISTINCT project_id),
                    (SELECT COUNT(*) FROM sessions s2
                     WHERE s2.project_id = (
                         SELECT project_id FROM sessions
                         WHERE started_at >= ? AND started_at < ? AND project_id IS NOT NULL
                         GROUP BY project_id ORDER BY COUNT(*) DESC LIMIT 1
                     ) AND s2.started_at >= ? AND s2.started_at < ?)
                FROM sessions
                WHERE started_at >= ? AND started_at < ?
                  AND project_id IS NOT NULL
                "#,
                params![&start_str, &end_str, &start_str, &end_str, &start_str, &end_str],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));

        let sessions_f = sessions.max(1) as f64;
        let total_tools_f = total_tools.max(1) as f64;
        let total_activity_f = total_activity.max(1) as f64;

        Ok(crate::analytics::personality::UsageProfile {
            read_to_edit_ratio: if edit_count > 0 {
                read_count as f64 / edit_count as f64
            } else {
                read_count as f64
            },
            agent_spawn_rate: agents as f64 / sessions_f,
            avg_session_duration_secs: avg_duration,
            edits_per_session: edit_count as f64 / sessions_f,
            bash_percentage: bash_count as f64 / total_tools_f,
            plans_per_session: plans as f64 / sessions_f,
            late_night_percentage: late_night as f64 / total_activity_f,
            early_morning_percentage: early_morning as f64 / total_activity_f,
            project_diversity: unique_projects as f64 / sessions_f,
            top_project_concentration: top_project_sessions as f64 / sessions_f,
        })
    }

    /// Get dashboard statistics for the Projects view header.
    ///
    /// Returns aggregate stats, activity heatmap (last 28 days), streaks, and patterns.
    pub fn get_dashboard_stats(&self) -> Result<crate::analytics::DashboardStats> {
        let conn = self.conn.lock().unwrap();

        // 1. Get aggregate totals
        let (project_count, session_count, total_tokens): (i64, i64, i64) = conn
            .query_row(
                r#"
                SELECT
                    (SELECT COUNT(*) FROM projects),
                    (SELECT COUNT(*) FROM sessions),
                    COALESCE((SELECT SUM(tokens_in + tokens_out) FROM messages), 0)
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        // 2. Get total duration (sum of session durations)
        // Note: SQLite returns REAL for julianday calculations, so we get as f64 and cast
        let total_duration_secs: i64 = conn
            .query_row(
                r#"
                SELECT COALESCE(SUM(
                    CASE WHEN last_activity_at IS NOT NULL AND started_at IS NOT NULL
                    THEN MAX(0, (julianday(last_activity_at) - julianday(started_at)) * 86400)
                    ELSE 0 END
                ), 0) as duration
                FROM sessions
                "#,
                [],
                |row| row.get::<_, f64>(0),
            )
            .map(|f| f as i64)
            .unwrap_or(0);

        // 3. Get daily activity for last 28 days
        // Use localtime to match user's timezone for "today"
        let mut daily_activity = [0i64; 28];
        {
            let mut stmt = conn.prepare(
                r#"
                SELECT
                    CAST(julianday(date('now', 'localtime')) - julianday(DATE(emitted_at)) AS INTEGER) as days_ago,
                    COUNT(*) as count
                FROM messages
                WHERE emitted_at >= datetime('now', '-28 days')
                GROUP BY DATE(emitted_at)
                "#,
            )?;
            let rows =
                stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
            for row in rows.flatten() {
                let (days_ago, count) = row;
                if (0..28).contains(&days_ago) {
                    // Index 27 = today (0 days ago), index 0 = 27 days ago
                    let idx = 27 - days_ago as usize;
                    daily_activity[idx] = count;
                }
            }
        }

        // 4. Calculate streaks
        let (current_streak, longest_streak) =
            crate::analytics::DashboardStats::calculate_streaks(&daily_activity);

        // 5. Get peak hour (most active hour of day)
        let peak_hour: u8 = conn
            .query_row(
                r#"
                SELECT CAST(strftime('%H', emitted_at) AS INTEGER) as hour
                FROM messages
                GROUP BY hour
                ORDER BY COUNT(*) DESC
                LIMIT 1
                "#,
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|h| h as u8)
            .unwrap_or(12);

        // 6. Get busiest day of week (0=Sunday, 1=Monday, ..., 6=Saturday)
        let busiest_day: u8 = conn
            .query_row(
                r#"
                SELECT CAST(strftime('%w', emitted_at) AS INTEGER) as dow
                FROM messages
                GROUP BY dow
                ORDER BY COUNT(*) DESC
                LIMIT 1
                "#,
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|d| d as u8)
            .unwrap_or(1);

        Ok(crate::analytics::DashboardStats {
            project_count,
            session_count,
            total_tokens,
            total_duration_secs,
            daily_activity,
            current_streak,
            longest_streak,
            peak_hour,
            busiest_day,
        })
    }

    // ============================================
    // Live View Queries
    // ============================================

    /// Get active sessions (threads with recent activity) for the live view.
    ///
    /// Returns threads that have had message activity within the last `since_minutes` minutes,
    /// ordered by last activity descending (most recent first).
    pub fn get_active_sessions(
        &self,
        since_minutes: i64,
    ) -> Result<Vec<crate::types::ActiveSession>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            r#"
            SELECT
                s.id as session_id,
                t.id as thread_id,
                COALESCE(p.name, '(no project)') as project_name,
                t.thread_type,
                s.assistant,
                MAX(m.emitted_at) as last_activity,
                COUNT(m.id) as message_count,
                t.parent_thread_id
            FROM threads t
            JOIN sessions s ON t.session_id = s.id
            LEFT JOIN projects p ON s.project_id = p.id
            JOIN messages m ON m.thread_id = t.id
            WHERE m.emitted_at >= datetime('now', ? || ' minutes')
            GROUP BY t.id
            ORDER BY last_activity DESC
            "#,
        )?;

        let since_param = format!("-{}", since_minutes);
        let sessions: Vec<crate::types::ActiveSession> = stmt
            .query_map([&since_param], |row| {
                let thread_type_str: String = row.get(3)?;
                let assistant_str: String = row.get(4)?;
                let last_activity_str: String = row.get(5)?;

                Ok(crate::types::ActiveSession {
                    session_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    project_name: row.get(2)?,
                    thread_type: thread_type_str
                        .parse()
                        .unwrap_or(crate::types::ThreadType::Main),
                    assistant: assistant_str
                        .parse()
                        .unwrap_or(crate::types::Assistant::ClaudeCode),
                    last_activity: chrono::DateTime::parse_from_rfc3339(&last_activity_str)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    message_count: row.get(6)?,
                    parent_thread_id: row.get(7)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(sessions)
    }

    /// Get aggregate statistics for the live view's stats toolbar.
    ///
    /// Returns message count, token totals, agent count, and tool call count
    /// for messages within the specified time window.
    pub fn get_live_stats(&self, since_minutes: i64) -> Result<crate::types::LiveStats> {
        let conn = self.conn.lock().unwrap();
        let since_param = format!("-{}", since_minutes);

        // Get message count, token totals, and tool call count
        let mut msg_stmt = conn.prepare(
            r#"
            SELECT
                COUNT(*) as total_messages,
                COALESCE(SUM(COALESCE(tokens_in, 0) + COALESCE(tokens_out, 0)), 0) as total_tokens,
                COALESCE(SUM(CASE WHEN message_type = 'tool_call' THEN 1 ELSE 0 END), 0) as total_tool_calls
            FROM messages
            WHERE emitted_at >= datetime('now', ? || ' minutes')
            "#,
        )?;

        let (total_messages, total_tokens, total_tool_calls): (i64, i64, i64) = msg_stmt
            .query_row([&since_param], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;

        // Get count of agent threads with activity in the window
        let mut agent_stmt = conn.prepare(
            r#"
            SELECT COUNT(DISTINCT t.id)
            FROM threads t
            JOIN messages m ON m.thread_id = t.id
            WHERE t.thread_type = 'agent'
              AND m.emitted_at >= datetime('now', ? || ' minutes')
            "#,
        )?;

        let total_agents: i64 = agent_stmt.query_row([&since_param], |row| row.get(0))?;

        Ok(crate::types::LiveStats {
            total_messages,
            total_tokens,
            total_agents,
            total_tool_calls,
        })
    }

    /// Get recent messages across all sessions for the live stream view.
    ///
    /// Returns messages with project/thread context, ordered by timestamp descending.
    /// The `limit` parameter controls how many messages to return.
    pub fn get_recent_messages(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::types::MessageWithContext>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            r#"
            SELECT
                m.id,
                m.emitted_at,
                s.assistant,
                COALESCE(p.name, '(no project)') as project_name,
                CASE
                    WHEN t.thread_type = 'main' THEN 'main'
                    ELSE substr(t.id, 1, 8)
                END as thread_name,
                m.author_role,
                m.message_type,
                COALESCE(
                    CASE
                        WHEN m.tool_name IS NOT NULL THEN m.tool_name
                        ELSE substr(COALESCE(m.content, ''), 1, 60)
                    END,
                    ''
                ) as preview,
                m.tool_name
            FROM messages m
            JOIN threads t ON m.thread_id = t.id
            JOIN sessions s ON t.session_id = s.id
            LEFT JOIN projects p ON s.project_id = p.id
            ORDER BY m.emitted_at DESC
            LIMIT ?
            "#,
        )?;

        let messages: Vec<crate::types::MessageWithContext> = stmt
            .query_map([limit as i64], |row| {
                let emitted_at_str: String = row.get(1)?;
                let assistant_str: String = row.get(2)?;
                let author_role_str: String = row.get(5)?;
                let message_type_str: String = row.get(6)?;

                Ok(crate::types::MessageWithContext {
                    id: row.get(0)?,
                    emitted_at: chrono::DateTime::parse_from_rfc3339(&emitted_at_str)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    assistant: assistant_str
                        .parse()
                        .unwrap_or(crate::types::Assistant::ClaudeCode),
                    project_name: row.get(3)?,
                    thread_name: row.get(4)?,
                    author_role: author_role_str
                        .parse()
                        .unwrap_or(crate::types::AuthorRole::System),
                    message_type: message_type_str
                        .parse()
                        .unwrap_or(crate::types::MessageType::Response),
                    preview: row.get(7)?,
                    tool_name: row.get(8)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(messages)
    }

    // ========== Environment Health ==========

    /// Get the database file size in bytes.
    pub fn get_database_size(&self) -> Result<u64> {
        let conn = self.conn.lock().unwrap();

        let page_count: u64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: u64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;

        Ok(page_count * page_size)
    }

    /// Get environment health stats for each assistant.
    /// Returns a list of (assistant, file_count, total_size_bytes, last_parsed_at).
    pub fn get_assistant_source_stats(&self) -> Result<Vec<AssistantSourceStats>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            r#"
            SELECT
                assistant,
                COUNT(*) as file_count,
                COALESCE(SUM(size_bytes), 0) as total_size,
                MAX(last_parsed_at) as last_parsed
            FROM source_files
            GROUP BY assistant
            ORDER BY assistant
            "#,
        )?;

        let rows: Vec<(Assistant, i64, i64, Option<DateTime<Utc>>)> = stmt
            .query_map([], |row| {
                let assistant_str: String = row.get(0)?;
                let last_parsed_str: Option<String> = row.get(3)?;
                let last_parsed = last_parsed_str
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                Ok((
                    assistant_str
                        .parse()
                        .unwrap_or(crate::types::Assistant::ClaudeCode),
                    row.get(1)?,
                    row.get(2)?,
                    last_parsed,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// Get total counts for environment overview.
    pub fn get_total_counts(&self) -> Result<(i64, i64, i64)> {
        let conn = self.conn.lock().unwrap();

        let session_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
        let message_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
        let source_file_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM source_files", [], |row| row.get(0))?;

        Ok((session_count, message_count, source_file_count))
    }

    /// Get aggregated environment health stats.
    pub fn get_environment_health(&self) -> Result<EnvironmentHealth> {
        let database_size_bytes = self.get_database_size()?;
        let assistant_stats = self.get_assistant_source_stats()?;
        let assistants = assistant_stats
            .into_iter()
            .map(
                |(assistant, file_count, total_size_bytes, last_synced)| AssistantHealth {
                    assistant,
                    file_count,
                    total_size_bytes,
                    last_synced,
                },
            )
            .collect();

        let (total_sessions, total_messages, _) = self.get_total_counts()?;

        Ok(EnvironmentHealth {
            database_size_bytes,
            assistants,
            total_sessions,
            total_messages,
        })
    }
}

/// Filter for listing sessions
#[derive(Debug, Default)]
pub struct SessionFilter {
    /// Filter by assistant type
    pub assistant: Option<Assistant>,
    /// Filter by status
    pub status: Option<SessionStatus>,
    /// Filter by project ID
    pub project_id: Option<String>,
    /// Filter sessions started after this time
    pub since: Option<DateTime<Utc>>,
    /// Maximum number of sessions to return
    pub limit: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_source_file() -> SourceFile {
        SourceFile {
            path: PathBuf::from("/path/to/source.jsonl"),
            file_type: FileType::Jsonl,
            assistant: Assistant::ClaudeCode,
            created_at: Utc::now(),
            modified_at: Utc::now(),
            size_bytes: 1024,
            last_parsed_at: None,
            checkpoint: Checkpoint::ByteOffset { offset: 0 },
        }
    }

    fn create_test_thread(session_id: &str) -> Thread {
        Thread {
            id: format!("{}-main", session_id),
            session_id: session_id.to_string(),
            thread_type: ThreadType::Main,
            parent_thread_id: None,
            spawned_by_message_id: None,
            started_at: Utc::now(),
            ended_at: None,
            last_activity_at: Some(Utc::now()),
            metadata: serde_json::json!({}),
        }
    }

    fn create_test_session() -> Session {
        Session {
            id: "test-session-1".to_string(),
            assistant: Assistant::ClaudeCode,
            backing_model_id: None,
            project_id: None,
            started_at: Utc::now(),
            last_activity_at: Some(Utc::now()),
            status: SessionStatus::Active,
            source_file_path: "/path/to/source.jsonl".to_string(),
            metadata: serde_json::json!({}),
        }
    }

    fn create_test_message(session_id: &str, thread_id: &str, seq: i32) -> Message {
        Message {
            id: 0,
            session_id: session_id.to_string(),
            thread_id: thread_id.to_string(),
            seq,
            emitted_at: Utc::now(),
            observed_at: Utc::now(),
            author_role: AuthorRole::Human,
            author_name: None,
            message_type: MessageType::Prompt,
            content: Some("Hello".to_string()),
            content_type: None,
            tool_name: None,
            tool_input: None,
            tool_result: None,
            tokens_in: Some(100),
            tokens_out: None,
            duration_ms: None,
            source_file_path: "/path/to/source.jsonl".to_string(),
            source_offset: 0,
            source_line: Some(1),
            raw_data: serde_json::json!({"type": "prompt"}),
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn test_session_crud() {
        let db = Database::open_in_memory().unwrap();
        db.migrate().unwrap();

        // Create source file first (for FK)
        let source_file = create_test_source_file();
        db.upsert_source_file(&source_file).unwrap();

        let session = create_test_session();

        // Insert
        db.upsert_session(&session).unwrap();

        // Read
        let retrieved = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(retrieved.id, session.id);
        assert_eq!(retrieved.assistant, Assistant::ClaudeCode);

        // List
        let sessions = db.list_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn test_message_insert_and_query() {
        let db = Database::open_in_memory().unwrap();
        db.migrate().unwrap();

        // Create source file first (for FK)
        let source_file = create_test_source_file();
        db.upsert_source_file(&source_file).unwrap();

        let session = create_test_session();
        db.upsert_session(&session).unwrap();

        // Create thread
        let thread = create_test_thread(&session.id);
        db.insert_thread(&thread).unwrap();

        // Insert messages
        let messages = vec![
            create_test_message(&session.id, &thread.id, 1),
            create_test_message(&session.id, &thread.id, 2),
            create_test_message(&session.id, &thread.id, 3),
        ];
        db.insert_messages(&messages).unwrap();

        // Query
        let retrieved = db.get_session_messages(&session.id, 10).unwrap();
        assert_eq!(retrieved.len(), 3);

        // Query by thread
        let thread_messages = db.get_thread_messages(&thread.id, 10).unwrap();
        assert_eq!(thread_messages.len(), 3);
        assert_eq!(thread_messages[0].seq, 1);
        assert_eq!(thread_messages[2].seq, 3);
    }

    #[test]
    fn test_source_file_checkpoint() {
        let db = Database::open_in_memory().unwrap();
        db.migrate().unwrap();

        let mut source_file = create_test_source_file();
        source_file.checkpoint = Checkpoint::ByteOffset { offset: 1024 };

        // Insert
        db.upsert_source_file(&source_file).unwrap();

        // Read
        let retrieved = db
            .get_source_file(&source_file.path.to_string_lossy())
            .unwrap()
            .unwrap();
        match retrieved.checkpoint {
            Checkpoint::ByteOffset { offset } => assert_eq!(offset, 1024),
            _ => panic!("Expected ByteOffset checkpoint"),
        }

        // Update with new checkpoint
        source_file.checkpoint = Checkpoint::ByteOffset { offset: 2048 };
        db.upsert_source_file(&source_file).unwrap();

        let retrieved = db
            .get_source_file(&source_file.path.to_string_lossy())
            .unwrap()
            .unwrap();
        match retrieved.checkpoint {
            Checkpoint::ByteOffset { offset } => assert_eq!(offset, 2048),
            _ => panic!("Expected ByteOffset checkpoint"),
        }
    }
}
