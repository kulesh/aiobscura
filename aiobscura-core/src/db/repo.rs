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
                project_id = excluded.project_id,
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
                                spawned_by_message_id, started_at, ended_at, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                thread.id,
                thread.session_id,
                thread.thread_type.as_str(),
                thread.parent_thread_id,
                thread.spawned_by_message_id,
                thread.started_at.to_rfc3339(),
                thread.ended_at.map(|t| t.to_rfc3339()),
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

    fn row_to_thread(row: &Row) -> rusqlite::Result<Thread> {
        let thread_type_str: String = row.get("thread_type")?;
        let started_at_str: String = row.get("started_at")?;
        let ended_at_str: Option<String> = row.get("ended_at")?;
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
            INSERT INTO messages (session_id, thread_id, seq, ts, author_role, author_name,
                                  message_type, content, tool_name, tool_input, tool_result,
                                  tokens_in, tokens_out, duration_ms, source_file_path,
                                  source_offset, source_line, raw_data, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            "#,
            params![
                message.session_id,
                message.thread_id,
                message.seq,
                message.ts.to_rfc3339(),
                message.author_role.as_str(),
                message.author_name,
                message.message_type.as_str(),
                message.content,
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
                INSERT INTO messages (session_id, thread_id, seq, ts, author_role, author_name,
                                      message_type, content, tool_name, tool_input, tool_result,
                                      tokens_in, tokens_out, duration_ms, source_file_path,
                                      source_offset, source_line, raw_data, metadata)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
                "#,
                params![
                    message.session_id,
                    message.thread_id,
                    message.seq,
                    message.ts.to_rfc3339(),
                    message.author_role.as_str(),
                    message.author_name,
                    message.message_type.as_str(),
                    message.content,
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
        let mut stmt =
            conn.prepare("SELECT * FROM messages WHERE session_id = ? ORDER BY ts ASC LIMIT ?")?;

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

    fn row_to_message(row: &Row) -> rusqlite::Result<Message> {
        let author_role_str: String = row.get("author_role")?;
        let message_type_str: String = row.get("message_type")?;
        let ts_str: String = row.get("ts")?;
        let tool_input_str: Option<String> = row.get("tool_input")?;
        let raw_data_str: String = row.get("raw_data")?;
        let metadata_str: String = row.get("metadata")?;

        Ok(Message {
            id: row.get("id")?,
            session_id: row.get("session_id")?,
            thread_id: row.get("thread_id")?,
            seq: row.get("seq")?,
            ts: DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            author_role: author_role_str.parse().unwrap_or(AuthorRole::System),
            author_name: row.get("author_name")?,
            message_type: message_type_str.parse().unwrap_or(MessageType::Response),
            content: row.get("content")?,
            content_type: None, // TODO: Add content_type column to database schema
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

    /// Get session timestamps for duration calculation
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
                let last = last_str
                    .map(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    })
                    .flatten();
                Ok(Some((started, last)))
            }
            None => Ok(None),
        }
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
            ts: Utc::now(),
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
