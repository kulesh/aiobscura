//! Application state for the TUI.

use std::collections::HashMap;

use aiobscura_core::analytics::{
    generate_wrapped, DashboardStats, ProjectRow, ProjectStats, SessionAnalytics, ThreadAnalytics,
    WrappedConfig, WrappedPeriod, WrappedStats,
};
use aiobscura_core::db::{FileStats, ToolStats};
use aiobscura_core::{
    ActiveSession, Database, LiveStats, Message, MessageWithContext, Plan, SessionFilter, Thread,
    ThreadType,
};
use anyhow::Result;
use chrono::Datelike;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use crate::thread_row::{SessionRow, ThreadRow};

/// Sub-tab within Project detail view.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum ProjectSubTab {
    #[default]
    Overview,
    Sessions,
    Plans,
    Files,
}

/// Analytics panel view mode (session-level or thread-level).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum AnalyticsViewMode {
    /// Show session-level analytics (aggregated across all threads)
    #[default]
    Session,
    /// Show thread-level analytics (for the current thread only)
    Thread,
}

/// Current view mode
#[derive(Debug, Clone, Default)]
pub enum ViewMode {
    /// Thread list view
    List,
    /// Thread detail view showing messages
    Detail {
        #[allow(dead_code)]
        thread_id: String,
        thread_name: String,
    },
    /// Plan list view showing plans for a session
    PlanList {
        #[allow(dead_code)]
        session_id: String,
        session_name: String,
        /// True if we came from Detail view, false if from List view
        came_from_detail: bool,
        /// Thread info for returning to Detail view
        return_thread_id: Option<String>,
        return_thread_name: Option<String>,
    },
    /// Plan detail view showing plan content
    PlanDetail {
        #[allow(dead_code)]
        plan_slug: String,
        plan_title: String,
    },
    /// Wrapped view showing year/month in review
    Wrapped,
    /// Live activity view showing message stream across all sessions
    Live,
    /// Project list view (default)
    #[default]
    ProjectList,
    /// Project detail view showing stats
    ProjectDetail {
        project_id: String,
        project_name: String,
        sub_tab: ProjectSubTab,
    },
    /// Session detail view showing merged messages across all threads
    SessionDetail {
        #[allow(dead_code)]
        session_id: String,
        session_name: String,
    },
}

/// Metadata for the detail view header.
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
    /// Agent thread count
    pub agent_count: i64,
    /// Tool usage stats
    pub tool_stats: ToolStats,
    /// Plan count
    pub plan_count: i64,
    /// File modification stats
    pub file_stats: FileStats,
}

/// Main application state.
pub struct App {
    /// Database connection
    db: Database,
    /// Current view mode
    pub view_mode: ViewMode,
    /// Threads loaded for display
    pub threads: Vec<ThreadRow>,
    /// Table selection state
    pub table_state: TableState,
    /// Messages for detail view
    pub messages: Vec<Message>,
    /// Scroll offset for detail view
    pub scroll_offset: usize,
    /// Plans for current session
    pub plans: Vec<Plan>,
    /// Plan table selection state
    pub plan_table_state: TableState,
    /// Selected plan for detail view
    pub selected_plan: Option<Plan>,
    /// Scroll offset for plan detail view
    pub plan_scroll_offset: usize,
    /// Metadata for current thread (detail view)
    pub thread_metadata: Option<ThreadMetadata>,
    /// Analytics for current session (detail view)
    pub session_analytics: Option<SessionAnalytics>,
    /// Error message if session analytics computation failed
    pub session_analytics_error: Option<String>,
    /// Analytics for current thread (detail view)
    pub thread_analytics: Option<ThreadAnalytics>,
    /// Error message if thread analytics computation failed
    pub thread_analytics_error: Option<String>,
    /// Current analytics view mode (session vs thread)
    pub analytics_view_mode: AnalyticsViewMode,
    /// Scroll offset for analytics panel (for scrolling long content)
    pub analytics_scroll_offset: usize,
    /// Wrapped stats for the wrapped view
    pub wrapped_stats: Option<WrappedStats>,
    /// Current wrapped period
    pub wrapped_period: WrappedPeriod,
    /// Current wrapped card index (0-based)
    pub wrapped_card_index: usize,
    /// Cache for wrapped stats by period (avoids recomputation)
    wrapped_cache: HashMap<WrappedPeriod, WrappedStats>,
    /// Animation frame counter (increments each render)
    pub animation_frame: u64,
    /// Snowflake positions for holiday animation (x, y, speed)
    pub snowflakes: Vec<(u16, u16, u8)>,
    /// Whether the app should exit
    pub should_quit: bool,

    // ========== Project View State ==========
    /// Projects loaded for display
    pub projects: Vec<ProjectRow>,
    /// Project table selection state
    pub project_table_state: TableState,
    /// Selected project stats (for detail view)
    pub project_stats: Option<ProjectStats>,
    /// Dashboard stats for the header panel
    pub dashboard_stats: Option<DashboardStats>,

    // ========== Project Sub-Tab State ==========
    /// Sessions for current project
    pub project_sessions: Vec<SessionRow>,
    /// Project sessions table selection state
    pub project_sessions_table_state: TableState,
    /// Plans for current project
    pub project_plans: Vec<Plan>,
    /// Project plans table selection state
    pub project_plans_table_state: TableState,
    /// Files for current project (full list: path, edit_count)
    pub project_files: Vec<(String, i64)>,
    /// Project files table selection state
    pub project_files_table_state: TableState,

    // ========== Session Detail View State ==========
    /// Messages for session detail view (merged across all threads)
    pub session_messages: Vec<Message>,
    /// Threads for current session (for reference/drill-down)
    pub session_threads: Vec<Thread>,
    /// Scroll offset for session detail view
    pub session_scroll_offset: usize,

    // ========== Navigation Return State ==========
    /// Return destination when exiting Detail/PlanDetail views
    /// (project_id, project_name, sub_tab) - if set, return to project sub-tab
    pub return_to_project: Option<(String, String, ProjectSubTab)>,

    // ========== Live Refresh State ==========
    /// Last known latest message timestamp (for change detection)
    last_known_ts: Option<chrono::DateTime<chrono::Utc>>,
    /// Tick count when last update was detected (for live indicator flash)
    pub live_update_tick: Option<u32>,
    /// Current tick count (incremented each render)
    pub tick_count: u32,

    // ========== Live View State ==========
    /// Messages for the live stream view (newest first from DB, displayed newest at top)
    pub live_messages: Vec<MessageWithContext>,
    /// Scroll offset for live view (0 = top/newest, increasing shows older)
    pub live_scroll_offset: usize,
    /// Whether auto-scroll is enabled (stays at top showing newest)
    pub live_auto_scroll: bool,
    /// Active sessions for the live view (threads with recent activity)
    pub active_sessions: Vec<ActiveSession>,
    /// Aggregate stats for the live view's toolbar
    pub live_stats: LiveStats,
}

impl App {
    /// Create a new App with the given database connection.
    pub fn new(db: Database) -> Self {
        Self {
            db,
            view_mode: ViewMode::default(),
            threads: Vec::new(),
            table_state: TableState::default(),
            messages: Vec::new(),
            scroll_offset: 0,
            plans: Vec::new(),
            plan_table_state: TableState::default(),
            selected_plan: None,
            plan_scroll_offset: 0,
            thread_metadata: None,
            session_analytics: None,
            session_analytics_error: None,
            thread_analytics: None,
            thread_analytics_error: None,
            analytics_view_mode: AnalyticsViewMode::default(),
            analytics_scroll_offset: 0,
            wrapped_stats: None,
            wrapped_period: WrappedPeriod::current_year(),
            wrapped_card_index: 0,
            wrapped_cache: HashMap::new(),
            animation_frame: 0,
            snowflakes: Vec::new(),
            should_quit: false,
            // Project view state
            projects: Vec::new(),
            project_table_state: TableState::default(),
            project_stats: None,
            dashboard_stats: None,
            // Project sub-tab state
            project_sessions: Vec::new(),
            project_sessions_table_state: TableState::default(),
            project_plans: Vec::new(),
            project_plans_table_state: TableState::default(),
            project_files: Vec::new(),
            project_files_table_state: TableState::default(),
            // Session detail view state
            session_messages: Vec::new(),
            session_threads: Vec::new(),
            session_scroll_offset: 0,
            // Navigation return state
            return_to_project: None,
            // Live refresh state
            last_known_ts: None,
            live_update_tick: None,
            tick_count: 0,
            // Live view state
            live_messages: Vec::new(),
            live_scroll_offset: 0,
            live_auto_scroll: true,
            active_sessions: Vec::new(),
            live_stats: LiveStats::default(),
        }
    }

    /// Tick the animation state (call each frame).
    pub fn tick_animation(&mut self, width: u16, height: u16) {
        self.animation_frame = self.animation_frame.wrapping_add(1);
        self.tick_count = self.tick_count.wrapping_add(1);

        // Initialize snowflakes if empty and in wrapped view
        if matches!(self.view_mode, ViewMode::Wrapped) {
            if self.snowflakes.is_empty() {
                self.init_snowflakes(width, height);
            }
            self.update_snowflakes(height);
        }
    }

    /// Initialize snowflakes with random positions.
    fn init_snowflakes(&mut self, width: u16, height: u16) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Create ~30 snowflakes with pseudo-random positions
        let count = 30;
        for i in 0..count {
            // Use a simple hash for pseudo-randomness
            let mut hasher = DefaultHasher::new();
            (i as u64 * 7919 + self.animation_frame).hash(&mut hasher);
            let hash = hasher.finish();

            let x = (hash % width as u64) as u16;
            let y = ((hash / width as u64) % height as u64) as u16;
            let speed = ((hash / (width as u64 * height as u64)) % 3 + 1) as u8;

            self.snowflakes.push((x, y, speed));
        }
    }

    /// Update snowflake positions (falling animation).
    fn update_snowflakes(&mut self, height: u16) {
        for (x, y, speed) in &mut self.snowflakes {
            // Only update every few frames based on speed
            if self.animation_frame.is_multiple_of(*speed as u64 * 2) {
                *y = (*y + 1) % height;

                // Add slight horizontal drift
                if self.animation_frame.is_multiple_of(7) {
                    if *x > 0 && self.animation_frame.is_multiple_of(2) {
                        *x -= 1;
                    } else if *x < 200 {
                        *x += 1;
                    }
                }
            }
        }
    }

    /// Load threads from the database with hierarchy.
    pub fn load_threads(&mut self) -> Result<()> {
        let sessions = self.db.list_sessions(&SessionFilter::default())?;
        self.threads.clear();

        // Collect all threads with their session info
        struct ThreadInfo {
            thread: Thread,
            session_id: String,
            project_name: String,
            assistant_name: String,
        }

        let mut all_threads: Vec<ThreadInfo> = Vec::new();

        for session in sessions {
            // Get project name
            let project_name = session
                .project_id
                .as_ref()
                .and_then(|id| self.db.get_project(id).ok().flatten())
                .and_then(|p| p.name)
                .unwrap_or_else(|| "(no project)".to_string());

            let assistant_name = session.assistant.display_name().to_string();

            // Get all threads for this session
            let threads = self.db.get_session_threads(&session.id).unwrap_or_default();

            for thread in threads {
                all_threads.push(ThreadInfo {
                    thread,
                    session_id: session.id.clone(),
                    project_name: project_name.clone(),
                    assistant_name: assistant_name.clone(),
                });
            }
        }

        // Build a map of thread_id -> children for hierarchy
        let mut children_map: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, info) in all_threads.iter().enumerate() {
            if let Some(parent_id) = &info.thread.parent_thread_id {
                children_map.entry(parent_id.clone()).or_default().push(idx);
            }
        }

        // Group threads by project, then build hierarchy
        let mut project_threads: HashMap<String, Vec<&ThreadInfo>> = HashMap::new();
        for info in &all_threads {
            project_threads
                .entry(info.project_name.clone())
                .or_default()
                .push(info);
        }

        // Sort project names
        let mut project_names: Vec<_> = project_threads.keys().cloned().collect();
        project_names.sort();

        // Build final thread list with hierarchy
        for project_name in project_names {
            let threads = project_threads.get(&project_name).unwrap();

            // Separate main threads and orphan agents
            let mut main_threads: Vec<&ThreadInfo> = Vec::new();
            let mut orphan_agents: Vec<&ThreadInfo> = Vec::new();

            for info in threads {
                if info.thread.parent_thread_id.is_none() {
                    if info.thread.thread_type == ThreadType::Main {
                        main_threads.push(info);
                    } else {
                        // Agent/Background with no parent = orphan
                        orphan_agents.push(info);
                    }
                }
            }

            // Sort main threads by last activity (most recent first)
            main_threads.sort_by(|a, b| b.thread.started_at.cmp(&a.thread.started_at));

            // Add main threads with their children
            for main_info in main_threads {
                // Add main thread
                let message_count = self
                    .db
                    .count_thread_messages(&main_info.thread.id)
                    .unwrap_or(0);

                // Get actual last message timestamp, fall back to thread timestamps
                let last_activity = self
                    .db
                    .get_thread_last_activity(&main_info.thread.id)
                    .ok()
                    .flatten()
                    .or(main_info.thread.ended_at)
                    .or(Some(main_info.thread.started_at));

                self.threads.push(ThreadRow {
                    id: main_info.thread.id.clone(),
                    session_id: main_info.session_id.clone(),
                    thread_type: main_info.thread.thread_type,
                    parent_thread_id: None,
                    last_activity,
                    project_name: main_info.project_name.clone(),
                    assistant_name: main_info.assistant_name.clone(),
                    message_count,
                    indent_level: 0,
                    is_last_child: false,
                });

                // Add child threads (agents spawned by this main thread)
                if let Some(child_indices) = children_map.get(&main_info.thread.id) {
                    let mut children: Vec<&ThreadInfo> =
                        child_indices.iter().map(|&idx| &all_threads[idx]).collect();
                    // Sort children by started_at
                    children.sort_by(|a, b| a.thread.started_at.cmp(&b.thread.started_at));

                    let child_count = children.len();
                    for (child_idx, child_info) in children.into_iter().enumerate() {
                        let message_count = self
                            .db
                            .count_thread_messages(&child_info.thread.id)
                            .unwrap_or(0);

                        // Get actual last message timestamp, fall back to thread timestamps
                        let last_activity = self
                            .db
                            .get_thread_last_activity(&child_info.thread.id)
                            .ok()
                            .flatten()
                            .or(child_info.thread.ended_at)
                            .or(Some(child_info.thread.started_at));

                        self.threads.push(ThreadRow {
                            id: child_info.thread.id.clone(),
                            session_id: child_info.session_id.clone(),
                            thread_type: child_info.thread.thread_type,
                            parent_thread_id: child_info.thread.parent_thread_id.clone(),
                            last_activity,
                            project_name: child_info.project_name.clone(),
                            assistant_name: child_info.assistant_name.clone(),
                            message_count,
                            indent_level: 1,
                            is_last_child: child_idx == child_count - 1,
                        });
                    }
                }
            }

            // Add orphan agents at the end of this project group
            orphan_agents.sort_by(|a, b| b.thread.started_at.cmp(&a.thread.started_at));
            for orphan_info in orphan_agents {
                let message_count = self
                    .db
                    .count_thread_messages(&orphan_info.thread.id)
                    .unwrap_or(0);

                // Get actual last message timestamp, fall back to thread timestamps
                let last_activity = self
                    .db
                    .get_thread_last_activity(&orphan_info.thread.id)
                    .ok()
                    .flatten()
                    .or(orphan_info.thread.ended_at)
                    .or(Some(orphan_info.thread.started_at));

                self.threads.push(ThreadRow {
                    id: orphan_info.thread.id.clone(),
                    session_id: orphan_info.session_id.clone(),
                    thread_type: orphan_info.thread.thread_type,
                    parent_thread_id: None,
                    last_activity,
                    project_name: orphan_info.project_name.clone(),
                    assistant_name: orphan_info.assistant_name.clone(),
                    message_count,
                    indent_level: 0,
                    is_last_child: false,
                });
            }
        }

        // Sort all threads by last_activity descending (most recent first)
        self.threads.sort_by(|a, b| {
            let a_time = a.last_activity;
            let b_time = b.last_activity;
            b_time.cmp(&a_time)
        });

        // Select first row if we have threads
        if !self.threads.is_empty() && self.table_state.selected().is_none() {
            self.table_state.select(Some(0));
        }

        Ok(())
    }

    /// Handle keyboard input.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match &self.view_mode {
            ViewMode::List => self.handle_list_key(key),
            ViewMode::Detail { .. } => self.handle_detail_key(key),
            ViewMode::PlanList { .. } => self.handle_plan_list_key(key),
            ViewMode::PlanDetail { .. } => self.handle_plan_detail_key(key),
            ViewMode::Wrapped => self.handle_wrapped_key(key),
            ViewMode::Live => self.handle_live_key(key),
            ViewMode::ProjectList => self.handle_project_list_key(key),
            ViewMode::ProjectDetail { .. } => self.handle_project_detail_key(key),
            ViewMode::SessionDetail { .. } => self.handle_session_detail_key(key),
        }
    }

    /// Handle keyboard input in list view.
    fn handle_list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Enter => {
                self.open_detail_view();
            }
            KeyCode::Char('w') => {
                self.open_wrapped_view();
            }
            KeyCode::Esc | KeyCode::Tab => {
                // Tab cycles: Threads -> Live
                self.open_live_view();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_previous();
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.select_first();
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.select_last();
            }
            _ => {}
        }
    }

    /// Handle keyboard input in detail view.
    fn handle_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.close_detail_view();
            }
            KeyCode::Char('p') => {
                self.open_plan_list(true);
            }
            KeyCode::Char('a') => {
                self.toggle_analytics_view();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up();
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.scroll_offset = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.scroll_to_bottom();
            }
            KeyCode::PageDown | KeyCode::Char('d') | KeyCode::Char(' ') => {
                self.scroll_down_page();
            }
            KeyCode::PageUp | KeyCode::Char('u') => {
                self.scroll_up_page();
            }
            _ => {}
        }
    }

    /// Open detail view for the selected thread.
    fn open_detail_view(&mut self) {
        if let Some(idx) = self.table_state.selected() {
            if let Some(thread) = self.threads.get(idx) {
                let thread_id = thread.id.clone();
                let session_id = thread.session_id.clone();
                let thread_name = format!("{} - {}", thread.project_name, thread.short_id());

                // Load messages for this thread
                if let Ok(messages) = self.db.get_thread_messages(&thread_id, 10000) {
                    self.messages = messages;
                    self.scroll_offset = 0;

                    // Load metadata for the header
                    self.thread_metadata = self.load_thread_metadata(&thread_id, &session_id);

                    // Load/compute analytics (both session and thread)
                    self.load_session_analytics(&session_id);
                    self.load_thread_analytics(&thread_id);

                    // Reset analytics view mode to session (default)
                    self.analytics_view_mode = AnalyticsViewMode::Session;
                    self.analytics_scroll_offset = 0;

                    self.view_mode = ViewMode::Detail {
                        thread_id,
                        thread_name,
                    };
                }
            }
        }
    }

    /// Load metadata for the detail view header.
    fn load_thread_metadata(&self, thread_id: &str, session_id: &str) -> Option<ThreadMetadata> {
        // Get source path
        let source_path = self.db.get_session_source_path(session_id).ok().flatten();

        // Get model name
        let model_name = self.db.get_session_model_name(session_id).ok().flatten();

        // Get session metadata (cwd, git_branch) from JSON
        let (cwd, git_branch) = self
            .db
            .get_session_metadata(session_id)
            .ok()
            .flatten()
            .map(|json| {
                let cwd = json.get("cwd").and_then(|v| v.as_str()).map(String::from);
                let git_branch = json
                    .get("git_branch")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                (cwd, git_branch)
            })
            .unwrap_or((None, None));

        // Get timestamps and calculate duration
        let duration_secs = self
            .db
            .get_session_timestamps(session_id)
            .ok()
            .flatten()
            .map(|(started, last_activity)| {
                // Use last_activity if available, otherwise use started (0 duration)
                last_activity
                    .map(|last| last.signed_duration_since(started).num_seconds().max(0))
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        // Get message count for this thread
        let message_count = self.db.count_thread_messages(thread_id).unwrap_or(0);

        // Get agent count for this session
        let agent_count = self.db.count_session_agents(session_id).unwrap_or(0);

        // Get tool stats for this thread
        let tool_stats = self.db.get_thread_tool_stats(thread_id).unwrap_or_default();

        // Get plan count for this session
        let plan_count = self.db.count_session_plans(session_id).unwrap_or(0);

        // Get file stats for this thread
        let file_stats = self.db.get_thread_file_stats(thread_id).unwrap_or_default();

        Some(ThreadMetadata {
            source_path,
            cwd,
            git_branch,
            model_name,
            duration_secs,
            message_count,
            agent_count,
            tool_stats,
            plan_count,
            file_stats,
        })
    }

    /// Load or compute session analytics for the detail view.
    fn load_session_analytics(&mut self, session_id: &str) {
        use aiobscura_core::analytics::create_default_engine;

        let engine = create_default_engine();
        match engine.ensure_session_analytics(session_id, &self.db) {
            Ok(analytics) => {
                self.session_analytics = Some(analytics);
                self.session_analytics_error = None;
            }
            Err(e) => {
                self.session_analytics = None;
                self.session_analytics_error = Some(e.to_string());
                tracing::warn!(session_id, error = %e, "Failed to compute session analytics");
            }
        }
    }

    /// Load or compute thread analytics for the detail view.
    fn load_thread_analytics(&mut self, thread_id: &str) {
        use aiobscura_core::analytics::create_default_engine;

        let engine = create_default_engine();
        match engine.ensure_thread_analytics(thread_id, &self.db) {
            Ok(analytics) => {
                self.thread_analytics = Some(analytics);
                self.thread_analytics_error = None;
            }
            Err(e) => {
                self.thread_analytics = None;
                self.thread_analytics_error = Some(e.to_string());
                tracing::warn!(thread_id, error = %e, "Failed to compute thread analytics");
            }
        }
    }

    /// Toggle between session and thread analytics view.
    fn toggle_analytics_view(&mut self) {
        self.analytics_view_mode = match self.analytics_view_mode {
            AnalyticsViewMode::Session => AnalyticsViewMode::Thread,
            AnalyticsViewMode::Thread => AnalyticsViewMode::Session,
        };
        self.analytics_scroll_offset = 0; // Reset scroll when switching
    }

    /// Close detail view and return to list.
    fn close_detail_view(&mut self) {
        // Check if we should return to a project sub-tab
        if let Some((project_id, project_name, sub_tab)) = self.return_to_project.take() {
            self.view_mode = ViewMode::ProjectDetail {
                project_id,
                project_name,
                sub_tab,
            };
        } else {
            self.view_mode = ViewMode::List;
        }
        self.messages.clear();
        self.scroll_offset = 0;
        self.thread_metadata = None;
        self.session_analytics = None;
        self.session_analytics_error = None;
        self.thread_analytics = None;
        self.thread_analytics_error = None;
        self.analytics_view_mode = AnalyticsViewMode::Session;
        self.analytics_scroll_offset = 0;
    }

    /// Scroll down in detail view.
    fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    /// Scroll up in detail view.
    fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Scroll down by a page (10 lines).
    fn scroll_down_page(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(10);
    }

    /// Scroll up by a page (10 lines).
    fn scroll_up_page(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(10);
    }

    /// Scroll to the bottom.
    fn scroll_to_bottom(&mut self) {
        // This will be clamped during rendering
        self.scroll_offset = self.messages.len().saturating_sub(1);
    }

    /// Select the next row in the table.
    fn select_next(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.threads.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    /// Select the previous row in the table.
    fn select_previous(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.threads.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    /// Select the first row.
    fn select_first(&mut self) {
        if !self.threads.is_empty() {
            self.table_state.select(Some(0));
        }
    }

    /// Select the last row.
    fn select_last(&mut self) {
        if !self.threads.is_empty() {
            self.table_state.select(Some(self.threads.len() - 1));
        }
    }

    // ========== Plan View Methods ==========

    /// Handle keyboard input in plan list view.
    fn handle_plan_list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.close_plan_list();
            }
            KeyCode::Enter => {
                self.open_plan_detail();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next_plan();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_previous_plan();
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.select_first_plan();
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.select_last_plan();
            }
            _ => {}
        }
    }

    /// Handle keyboard input in plan detail view.
    fn handle_plan_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.close_plan_detail();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.plan_scroll_offset = self.plan_scroll_offset.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.plan_scroll_offset = self.plan_scroll_offset.saturating_sub(1);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.plan_scroll_offset = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                // Will be clamped during rendering
                if let Some(plan) = &self.selected_plan {
                    let lines = plan
                        .content
                        .as_ref()
                        .map(|c| c.lines().count())
                        .unwrap_or(0);
                    self.plan_scroll_offset = lines.saturating_sub(1);
                }
            }
            KeyCode::PageDown | KeyCode::Char('d') | KeyCode::Char(' ') => {
                self.plan_scroll_offset = self.plan_scroll_offset.saturating_add(10);
            }
            KeyCode::PageUp | KeyCode::Char('u') => {
                self.plan_scroll_offset = self.plan_scroll_offset.saturating_sub(10);
            }
            _ => {}
        }
    }

    /// Open plan list for the selected thread's session.
    fn open_plan_list(&mut self, from_detail: bool) {
        // Get the selected thread (from list or current detail view)
        let thread = if from_detail {
            // We're in detail view - find the thread by matching view state
            if let ViewMode::Detail {
                thread_id,
                thread_name,
            } = &self.view_mode
            {
                Some((thread_id.clone(), thread_name.clone()))
            } else {
                None
            }
        } else {
            // We're in list view - get selected thread
            self.table_state.selected().and_then(|idx| {
                self.threads.get(idx).map(|t| {
                    (
                        t.id.clone(),
                        format!("{} - {}", t.project_name, t.short_id()),
                    )
                })
            })
        };

        if let Some((thread_id, thread_name)) = thread {
            // Find the thread to get session_id
            if let Some(thread_row) = self.threads.iter().find(|t| t.id == thread_id) {
                let session_id = thread_row.session_id.clone();
                let session_name = format!(
                    "{} - {}",
                    thread_row.project_name,
                    &session_id[..8.min(session_id.len())]
                );

                // Load plans for this session
                if let Ok(plans) = self.db.get_plans_for_session(&session_id) {
                    self.plans = plans;
                    self.plan_table_state = TableState::default();
                    if !self.plans.is_empty() {
                        self.plan_table_state.select(Some(0));
                    }

                    self.view_mode = ViewMode::PlanList {
                        session_id,
                        session_name,
                        came_from_detail: from_detail,
                        return_thread_id: if from_detail { Some(thread_id) } else { None },
                        return_thread_name: if from_detail { Some(thread_name) } else { None },
                    };
                }
            }
        }
    }

    /// Close plan list and return to previous view.
    fn close_plan_list(&mut self) {
        if let ViewMode::PlanList {
            came_from_detail,
            return_thread_id,
            return_thread_name,
            ..
        } = &self.view_mode
        {
            if *came_from_detail {
                if let (Some(thread_id), Some(thread_name)) =
                    (return_thread_id.clone(), return_thread_name.clone())
                {
                    // Reload messages and return to detail view
                    if let Ok(messages) = self.db.get_thread_messages(&thread_id, 10000) {
                        self.messages = messages;
                        self.scroll_offset = 0;
                        self.view_mode = ViewMode::Detail {
                            thread_id,
                            thread_name,
                        };
                    } else {
                        self.view_mode = ViewMode::List;
                    }
                } else {
                    self.view_mode = ViewMode::List;
                }
            } else {
                self.view_mode = ViewMode::List;
            }
        }
        self.plans.clear();
    }

    /// Open plan detail for the selected plan.
    fn open_plan_detail(&mut self) {
        if let Some(idx) = self.plan_table_state.selected() {
            if let Some(plan) = self.plans.get(idx) {
                let plan_slug = plan.id.clone();
                let plan_title = plan.title.clone().unwrap_or_else(|| plan_slug.clone());

                self.selected_plan = Some(plan.clone());
                self.plan_scroll_offset = 0;
                self.view_mode = ViewMode::PlanDetail {
                    plan_slug,
                    plan_title,
                };
            }
        }
    }

    /// Close plan detail and return to plan list.
    fn close_plan_detail(&mut self) {
        // Check if we should return to a project sub-tab
        if let Some((project_id, project_name, sub_tab)) = self.return_to_project.take() {
            self.view_mode = ViewMode::ProjectDetail {
                project_id,
                project_name,
                sub_tab,
            };
            self.selected_plan = None;
            self.plan_scroll_offset = 0;
            return;
        }

        // We need to reconstruct the PlanList state
        // For now, find the session from the selected plan
        if let Some(plan) = &self.selected_plan {
            let session_id = plan.session_id.clone();
            // Find a thread with this session to get session name
            if let Some(thread) = self.threads.iter().find(|t| t.session_id == session_id) {
                let session_name = format!(
                    "{} - {}",
                    thread.project_name,
                    &session_id[..8.min(session_id.len())]
                );
                self.view_mode = ViewMode::PlanList {
                    session_id,
                    session_name,
                    came_from_detail: false, // We lose this info, default to List
                    return_thread_id: None,
                    return_thread_name: None,
                };
            } else {
                self.view_mode = ViewMode::List;
            }
        } else {
            self.view_mode = ViewMode::List;
        }
        self.selected_plan = None;
        self.plan_scroll_offset = 0;
    }

    /// Select the next plan in the list.
    fn select_next_plan(&mut self) {
        if self.plans.is_empty() {
            return;
        }
        let i = match self.plan_table_state.selected() {
            Some(i) => {
                if i >= self.plans.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.plan_table_state.select(Some(i));
    }

    /// Select the previous plan in the list.
    fn select_previous_plan(&mut self) {
        if self.plans.is_empty() {
            return;
        }
        let i = match self.plan_table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.plans.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.plan_table_state.select(Some(i));
    }

    /// Select the first plan.
    fn select_first_plan(&mut self) {
        if !self.plans.is_empty() {
            self.plan_table_state.select(Some(0));
        }
    }

    /// Select the last plan.
    fn select_last_plan(&mut self) {
        if !self.plans.is_empty() {
            self.plan_table_state.select(Some(self.plans.len() - 1));
        }
    }

    // ========== Wrapped View Methods ==========

    /// Handle keyboard input in wrapped view.
    fn handle_wrapped_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.close_wrapped_view();
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                self.next_wrapped_card();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.prev_wrapped_card();
            }
            KeyCode::Char('m') => {
                self.toggle_wrapped_period();
            }
            KeyCode::Char('[') | KeyCode::Down | KeyCode::Char('j') => {
                self.prev_wrapped_month();
            }
            KeyCode::Char(']') | KeyCode::Up | KeyCode::Char('k') => {
                self.next_wrapped_month();
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.wrapped_card_index = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.wrapped_card_index = self.wrapped_card_count().saturating_sub(1);
            }
            _ => {}
        }
    }

    /// Open the wrapped view.
    fn open_wrapped_view(&mut self) {
        // Check cache first
        if let Some(cached) = self.wrapped_cache.get(&self.wrapped_period) {
            self.wrapped_stats = Some(cached.clone());
            self.wrapped_card_index = 0;
            self.view_mode = ViewMode::Wrapped;
            return;
        }

        // Cache miss - generate and store
        let config = WrappedConfig::default();
        match generate_wrapped(&self.db, self.wrapped_period, &config) {
            Ok(stats) => {
                self.wrapped_cache
                    .insert(self.wrapped_period, stats.clone());
                self.wrapped_stats = Some(stats);
                self.wrapped_card_index = 0;
                self.view_mode = ViewMode::Wrapped;
            }
            Err(e) => {
                // Log error but don't crash
                tracing::error!("Failed to generate wrapped stats: {}", e);
            }
        }
    }

    /// Close the wrapped view.
    fn close_wrapped_view(&mut self) {
        self.view_mode = ViewMode::List;
        self.wrapped_stats = None;
        self.wrapped_card_index = 0;
        // Keep cache - stats are valid for the session duration
    }

    /// Get the number of cards to display.
    pub fn wrapped_card_count(&self) -> usize {
        if self.wrapped_stats.is_none() {
            return 0;
        }
        // Cards: Title, Tools, Time Patterns, Streaks, Projects, Trends (if available), Personality (if available)
        let mut count = 5; // Title, Tools, Time, Streaks, Projects
        if let Some(stats) = &self.wrapped_stats {
            if stats.trends.is_some() {
                count += 1;
            }
            if stats.personality.is_some() {
                count += 1;
            }
        }
        count
    }

    /// Move to the next wrapped card.
    fn next_wrapped_card(&mut self) {
        let max = self.wrapped_card_count();
        if max > 0 && self.wrapped_card_index < max - 1 {
            self.wrapped_card_index += 1;
        }
    }

    /// Move to the previous wrapped card.
    fn prev_wrapped_card(&mut self) {
        if self.wrapped_card_index > 0 {
            self.wrapped_card_index -= 1;
        }
    }

    /// Toggle between year and month period.
    fn toggle_wrapped_period(&mut self) {
        self.wrapped_period = match self.wrapped_period {
            WrappedPeriod::Year(year) => {
                // Switch to current month of that year (or current month if same year)
                let now = chrono::Utc::now();
                if year == now.year() {
                    WrappedPeriod::Month(year, now.month())
                } else {
                    WrappedPeriod::Month(year, 12) // Default to December for past years
                }
            }
            WrappedPeriod::Month(year, _) => WrappedPeriod::Year(year),
        };

        // Check cache first
        if let Some(cached) = self.wrapped_cache.get(&self.wrapped_period) {
            self.wrapped_stats = Some(cached.clone());
            self.wrapped_card_index = 0;
            return;
        }

        // Cache miss - generate and store
        let config = WrappedConfig::default();
        if let Ok(stats) = generate_wrapped(&self.db, self.wrapped_period, &config) {
            self.wrapped_cache
                .insert(self.wrapped_period, stats.clone());
            self.wrapped_stats = Some(stats);
            self.wrapped_card_index = 0;
        }
    }

    /// Go to previous month (or switch to month view if in year view).
    fn prev_wrapped_month(&mut self) {
        self.wrapped_period = match self.wrapped_period {
            WrappedPeriod::Year(year) => {
                // Switch to December of that year
                WrappedPeriod::Month(year, 12)
            }
            WrappedPeriod::Month(year, month) => {
                if month == 1 {
                    WrappedPeriod::Month(year - 1, 12)
                } else {
                    WrappedPeriod::Month(year, month - 1)
                }
            }
        };
        self.load_wrapped_for_period();
    }

    /// Go to next month (or switch to month view if in year view).
    fn next_wrapped_month(&mut self) {
        let now = chrono::Utc::now();
        self.wrapped_period = match self.wrapped_period {
            WrappedPeriod::Year(year) => {
                // Switch to January of that year
                WrappedPeriod::Month(year, 1)
            }
            WrappedPeriod::Month(year, month) => {
                // Don't go past current month
                if year == now.year() && month >= now.month() {
                    WrappedPeriod::Month(year, month) // Stay at current
                } else if month == 12 {
                    WrappedPeriod::Month(year + 1, 1)
                } else {
                    WrappedPeriod::Month(year, month + 1)
                }
            }
        };
        self.load_wrapped_for_period();
    }

    /// Load wrapped stats for current period (with caching).
    fn load_wrapped_for_period(&mut self) {
        // Check cache first
        if let Some(cached) = self.wrapped_cache.get(&self.wrapped_period) {
            self.wrapped_stats = Some(cached.clone());
            self.wrapped_card_index = 0;
            return;
        }

        // Cache miss - generate and store
        let config = WrappedConfig::default();
        if let Ok(stats) = generate_wrapped(&self.db, self.wrapped_period, &config) {
            self.wrapped_cache
                .insert(self.wrapped_period, stats.clone());
            self.wrapped_stats = Some(stats);
            self.wrapped_card_index = 0;
        }
    }

    // ========== Live View Methods ==========

    /// Handle keyboard input in live view.
    fn handle_live_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Esc | KeyCode::Tab => {
                // Tab cycles: Live -> Projects
                // Always reload for fresh data
                let _ = self.load_projects();
                self.dashboard_stats = self.db.get_dashboard_stats().ok();
                self.view_mode = ViewMode::ProjectList;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                // Scroll down (toward older messages, increase offset)
                self.live_scroll_offset = self.live_scroll_offset.saturating_add(1);
                self.live_auto_scroll = false;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                // Scroll up (toward newer messages at top, decrease offset)
                if self.live_scroll_offset > 0 {
                    self.live_scroll_offset -= 1;
                }
                // Re-enable auto-scroll when back at top
                if self.live_scroll_offset == 0 {
                    self.live_auto_scroll = true;
                }
            }
            KeyCode::Char(' ') => {
                // Toggle auto-scroll
                self.live_auto_scroll = !self.live_auto_scroll;
                if self.live_auto_scroll {
                    self.live_scroll_offset = 0;
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                // Scroll to newest (top)
                self.live_scroll_offset = 0;
                self.live_auto_scroll = true;
            }
            KeyCode::End | KeyCode::Char('G') => {
                // Scroll to oldest (bottom)
                self.live_scroll_offset = self.live_messages.len().saturating_sub(1);
                self.live_auto_scroll = false;
            }
            KeyCode::PageUp | KeyCode::Char('u') => {
                // Scroll up toward newer (decrease offset)
                self.live_scroll_offset = self.live_scroll_offset.saturating_sub(10);
                if self.live_scroll_offset == 0 {
                    self.live_auto_scroll = true;
                }
            }
            KeyCode::PageDown | KeyCode::Char('d') => {
                // Scroll down toward older (increase offset)
                self.live_scroll_offset = self.live_scroll_offset.saturating_add(10);
                self.live_auto_scroll = false;
            }
            _ => {}
        }
    }

    /// Open the live activity view (called from main for startup).
    pub fn start_live_view(&mut self) -> Result<()> {
        self.live_messages = self.db.get_recent_messages(50)?;
        // Get sessions active in last 30 minutes for the panel
        self.active_sessions = self.db.get_active_sessions(30).unwrap_or_default();
        self.live_stats = self.db.get_live_stats(30).unwrap_or_default();
        self.live_scroll_offset = 0;
        self.live_auto_scroll = true;
        self.view_mode = ViewMode::Live;
        Ok(())
    }

    /// Open the live activity view (internal navigation).
    fn open_live_view(&mut self) {
        // Load recent messages (limit to 50 as per plan)
        if let Ok(messages) = self.db.get_recent_messages(50) {
            self.live_messages = messages;
            self.active_sessions = self.db.get_active_sessions(30).unwrap_or_default();
            self.live_stats = self.db.get_live_stats(30).unwrap_or_default();
            self.dashboard_stats = self.db.get_dashboard_stats().ok();
            self.live_scroll_offset = 0;
            self.live_auto_scroll = true;
            self.view_mode = ViewMode::Live;
        }
    }

    /// Refresh live messages from database.
    pub fn refresh_live_messages(&mut self) -> Result<()> {
        if let Ok(messages) = self.db.get_recent_messages(50) {
            self.live_messages = messages;
            // If auto-scroll is on, keep at top (showing newest)
            if self.live_auto_scroll {
                self.live_scroll_offset = 0;
            }
        }
        // Also refresh active sessions and stats
        self.active_sessions = self.db.get_active_sessions(30).unwrap_or_default();
        self.live_stats = self.db.get_live_stats(30).unwrap_or_default();
        Ok(())
    }

    // ========== Project View Methods ==========

    /// Handle keyboard input in project list view.
    fn handle_project_list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Esc | KeyCode::Tab => {
                // Tab cycles: Projects -> Threads
                self.close_project_list();
            }
            KeyCode::Enter => {
                self.open_project_detail();
            }
            KeyCode::Char('w') => {
                self.open_wrapped_view();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next_project();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_previous_project();
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.select_first_project();
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.select_last_project();
            }
            _ => {}
        }
    }

    /// Handle keyboard input in project detail view.
    fn handle_project_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            // Quit
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            // Back to project list
            KeyCode::Esc => {
                self.close_project_detail();
            }
            // Sub-tab navigation by Tab key
            KeyCode::Tab => {
                self.cycle_project_sub_tab(true);
            }
            KeyCode::BackTab => {
                self.cycle_project_sub_tab(false);
            }
            // Sub-tab navigation by number
            KeyCode::Char('1') | KeyCode::Char('o') => {
                self.set_project_sub_tab(ProjectSubTab::Overview);
            }
            KeyCode::Char('2') | KeyCode::Char('t') => {
                self.set_project_sub_tab(ProjectSubTab::Sessions);
            }
            KeyCode::Char('3') | KeyCode::Char('p') => {
                self.set_project_sub_tab(ProjectSubTab::Plans);
            }
            KeyCode::Char('4') | KeyCode::Char('f') => {
                self.set_project_sub_tab(ProjectSubTab::Files);
            }
            // List navigation (for Threads/Plans/Files tabs)
            KeyCode::Down | KeyCode::Char('j') => {
                self.project_list_next();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.project_list_previous();
            }
            // Open selected item
            KeyCode::Enter => {
                self.open_project_item();
            }
            _ => {}
        }
    }

    /// Handle keyboard input in session detail view.
    fn handle_session_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.close_session_detail();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.session_scroll_offset = self.session_scroll_offset.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.session_scroll_offset = self.session_scroll_offset.saturating_sub(1);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.session_scroll_offset = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.session_scroll_offset = self.session_messages.len().saturating_sub(1);
            }
            KeyCode::PageDown | KeyCode::Char('d') | KeyCode::Char(' ') => {
                self.session_scroll_offset = self.session_scroll_offset.saturating_add(10);
            }
            KeyCode::PageUp | KeyCode::Char('u') => {
                self.session_scroll_offset = self.session_scroll_offset.saturating_sub(10);
            }
            _ => {}
        }
    }

    /// Close session detail and return to project sessions tab.
    fn close_session_detail(&mut self) {
        if let Some((project_id, project_name, sub_tab)) = self.return_to_project.take() {
            self.view_mode = ViewMode::ProjectDetail {
                project_id,
                project_name,
                sub_tab,
            };
        } else {
            self.view_mode = ViewMode::ProjectList;
        }
        self.session_messages.clear();
        self.session_threads.clear();
        self.session_scroll_offset = 0;
        self.session_analytics = None;
        self.session_analytics_error = None;
    }

    /// Close project list and switch to thread list.
    fn close_project_list(&mut self) {
        // Always reload threads and stats for fresh data
        let _ = self.load_threads();
        self.dashboard_stats = self.db.get_dashboard_stats().ok();
        self.view_mode = ViewMode::List;
    }

    /// Open project detail for the selected project.
    fn open_project_detail(&mut self) {
        if let Some(idx) = self.project_table_state.selected() {
            if let Some(project) = self.projects.get(idx) {
                let project_id = project.id.clone();
                let project_name = project.name.clone();

                // Load project stats
                if let Ok(Some(stats)) = self.db.get_project_stats(&project_id) {
                    self.project_stats = Some(stats);
                    self.view_mode = ViewMode::ProjectDetail {
                        project_id,
                        project_name,
                        sub_tab: ProjectSubTab::Overview,
                    };
                }
            }
        }
    }

    /// Close project detail and return to project list.
    fn close_project_detail(&mut self) {
        self.view_mode = ViewMode::ProjectList;
        self.project_stats = None;
    }

    /// Select the next project in the list.
    fn select_next_project(&mut self) {
        if self.projects.is_empty() {
            return;
        }
        let i = match self.project_table_state.selected() {
            Some(i) => {
                if i >= self.projects.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.project_table_state.select(Some(i));
    }

    /// Select the previous project in the list.
    fn select_previous_project(&mut self) {
        if self.projects.is_empty() {
            return;
        }
        let i = match self.project_table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.projects.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.project_table_state.select(Some(i));
    }

    /// Select the first project.
    fn select_first_project(&mut self) {
        if !self.projects.is_empty() {
            self.project_table_state.select(Some(0));
        }
    }

    /// Select the last project.
    fn select_last_project(&mut self) {
        if !self.projects.is_empty() {
            self.project_table_state
                .select(Some(self.projects.len() - 1));
        }
    }

    /// Load projects from the database (for initial startup).
    pub fn load_projects(&mut self) -> Result<()> {
        self.projects = self.db.list_projects_with_stats()?;
        self.project_table_state = TableState::default();
        if !self.projects.is_empty() {
            self.project_table_state.select(Some(0));
        }
        // Load dashboard stats for the header panel
        self.dashboard_stats = self.db.get_dashboard_stats().ok();
        Ok(())
    }

    // ========== Live Refresh ==========

    /// Check if database has new data since last check.
    /// Returns true if data changed and view should be refreshed.
    pub fn check_for_updates(&mut self) -> Result<bool> {
        let latest = self.db.get_latest_message_ts()?;
        if latest != self.last_known_ts {
            self.last_known_ts = latest;
            // Record tick when update was detected (for live indicator flash)
            self.live_update_tick = Some(self.tick_count);
            return Ok(true);
        }
        Ok(false)
    }

    /// Returns true if the live indicator should be shown (within ~2 seconds of last update).
    pub fn should_show_live_indicator(&self) -> bool {
        if let Some(update_tick) = self.live_update_tick {
            // Show for ~20 ticks (about 2 seconds at 100ms per tick)
            self.tick_count.wrapping_sub(update_tick) < 20
        } else {
            false
        }
    }

    /// Check if the current view is a list view that should auto-refresh.
    pub fn is_list_view(&self) -> bool {
        matches!(
            self.view_mode,
            ViewMode::ProjectList
                | ViewMode::List
                | ViewMode::ProjectDetail { .. }
                | ViewMode::Live
        )
    }

    /// Refresh data for the current view, preserving selection state.
    pub fn refresh_current_view(&mut self) -> Result<()> {
        // Always refresh dashboard stats for list views
        if self.is_list_view() {
            self.dashboard_stats = self.db.get_dashboard_stats().ok();
        }

        match &self.view_mode {
            ViewMode::ProjectList => {
                let selected = self.project_table_state.selected();
                self.projects = self.db.list_projects_with_stats()?;
                // Restore selection if valid
                if let Some(idx) = selected {
                    if idx < self.projects.len() {
                        self.project_table_state.select(Some(idx));
                    }
                }
            }
            ViewMode::List => {
                let selected = self.table_state.selected();
                self.load_threads()?;
                if let Some(idx) = selected {
                    if idx < self.threads.len() {
                        self.table_state.select(Some(idx));
                    }
                }
            }
            ViewMode::ProjectDetail {
                project_id,
                project_name: _,
                sub_tab,
            } => {
                let project_id = project_id.clone();
                match sub_tab {
                    ProjectSubTab::Sessions => {
                        let selected = self.project_sessions_table_state.selected();
                        self.load_project_sessions(&project_id)?;
                        if let Some(idx) = selected {
                            if idx < self.project_sessions.len() {
                                self.project_sessions_table_state.select(Some(idx));
                            }
                        }
                    }
                    ProjectSubTab::Plans => {
                        let selected = self.project_plans_table_state.selected();
                        self.load_project_plans(&project_id)?;
                        if let Some(idx) = selected {
                            if idx < self.project_plans.len() {
                                self.project_plans_table_state.select(Some(idx));
                            }
                        }
                    }
                    ProjectSubTab::Files => {
                        // Files don't need refresh as frequently
                    }
                    ProjectSubTab::Overview => {
                        // Overview doesn't need frequent refresh
                    }
                }
            }
            ViewMode::Live => {
                self.refresh_live_messages()?;
            }
            // Detail views and other modes: don't auto-refresh (would disrupt reading)
            ViewMode::Detail { .. }
            | ViewMode::PlanDetail { .. }
            | ViewMode::PlanList { .. }
            | ViewMode::SessionDetail { .. }
            | ViewMode::Wrapped => {}
        }
        Ok(())
    }

    // ========== Project Sub-Tab Data Loading ==========

    /// Load sessions for a project.
    fn load_project_sessions(&mut self, project_id: &str) -> Result<()> {
        let summaries = self.db.list_project_sessions(project_id)?;
        self.project_sessions.clear();

        for summary in summaries {
            // Calculate duration from started_at to last_activity_at
            let duration_secs = summary
                .last_activity_at
                .map(|last| last.signed_duration_since(summary.started_at).num_seconds())
                .unwrap_or(0)
                .max(0);

            self.project_sessions.push(SessionRow {
                id: summary.id,
                last_activity: summary.last_activity_at,
                duration_secs,
                thread_count: summary.thread_count,
                message_count: summary.message_count,
                model_name: summary.model_name,
            });
        }

        // Select first if any
        self.project_sessions_table_state = TableState::default();
        if !self.project_sessions.is_empty() {
            self.project_sessions_table_state.select(Some(0));
        }

        Ok(())
    }

    /// Load plans for all sessions in a project.
    fn load_project_plans(&mut self, project_id: &str) -> Result<()> {
        let sessions = self.db.list_sessions(&SessionFilter::default())?;
        self.project_plans.clear();

        for session in sessions {
            // Skip sessions that don't belong to this project
            if session.project_id.as_ref() != Some(&project_id.to_string()) {
                continue;
            }

            // Get plans for this session
            if let Ok(plans) = self.db.get_plans_for_session(&session.id) {
                self.project_plans.extend(plans);
            }
        }

        // Sort by modified_at (most recent first)
        self.project_plans
            .sort_by(|a, b| b.modified_at.cmp(&a.modified_at));

        // Select first if any
        self.project_plans_table_state = TableState::default();
        if !self.project_plans.is_empty() {
            self.project_plans_table_state.select(Some(0));
        }

        Ok(())
    }

    /// Load complete file list for project.
    fn load_project_files(&mut self, project_id: &str) -> Result<()> {
        // Use the file_stats from ProjectStats if available
        if let Some(stats) = &self.project_stats {
            self.project_files = stats.file_stats.breakdown.clone();
        } else if let Ok(Some(stats)) = self.db.get_project_stats(project_id) {
            self.project_files = stats.file_stats.breakdown.clone();
        } else {
            self.project_files.clear();
        }

        // Select first if any
        self.project_files_table_state = TableState::default();
        if !self.project_files.is_empty() {
            self.project_files_table_state.select(Some(0));
        }

        Ok(())
    }

    /// Cycle to the next/previous project sub-tab.
    fn cycle_project_sub_tab(&mut self, forward: bool) {
        if let ViewMode::ProjectDetail { sub_tab, .. } = &self.view_mode {
            let next_tab = if forward {
                match sub_tab {
                    ProjectSubTab::Overview => ProjectSubTab::Sessions,
                    ProjectSubTab::Sessions => ProjectSubTab::Plans,
                    ProjectSubTab::Plans => ProjectSubTab::Files,
                    ProjectSubTab::Files => ProjectSubTab::Overview,
                }
            } else {
                match sub_tab {
                    ProjectSubTab::Overview => ProjectSubTab::Files,
                    ProjectSubTab::Sessions => ProjectSubTab::Overview,
                    ProjectSubTab::Plans => ProjectSubTab::Sessions,
                    ProjectSubTab::Files => ProjectSubTab::Plans,
                }
            };
            self.set_project_sub_tab(next_tab);
        }
    }

    /// Set the project sub-tab and load data if needed.
    fn set_project_sub_tab(&mut self, sub_tab: ProjectSubTab) {
        if let ViewMode::ProjectDetail {
            project_id,
            project_name,
            sub_tab: current_tab,
        } = &self.view_mode
        {
            // Skip if already on this tab
            if *current_tab == sub_tab {
                return;
            }

            let project_id = project_id.clone();
            let project_name = project_name.clone();

            // Load data for the new tab
            match sub_tab {
                ProjectSubTab::Overview => {
                    // Overview data is already loaded in project_stats
                }
                ProjectSubTab::Sessions => {
                    let _ = self.load_project_sessions(&project_id);
                }
                ProjectSubTab::Plans => {
                    let _ = self.load_project_plans(&project_id);
                }
                ProjectSubTab::Files => {
                    let _ = self.load_project_files(&project_id);
                }
            }

            self.view_mode = ViewMode::ProjectDetail {
                project_id,
                project_name,
                sub_tab,
            };
        }
    }

    /// Navigate in project sub-tab lists.
    fn project_list_next(&mut self) {
        if let ViewMode::ProjectDetail { sub_tab, .. } = &self.view_mode {
            match sub_tab {
                ProjectSubTab::Sessions => {
                    if self.project_sessions.is_empty() {
                        return;
                    }
                    let i = match self.project_sessions_table_state.selected() {
                        Some(i) if i >= self.project_sessions.len() - 1 => 0,
                        Some(i) => i + 1,
                        None => 0,
                    };
                    self.project_sessions_table_state.select(Some(i));
                }
                ProjectSubTab::Plans => {
                    if self.project_plans.is_empty() {
                        return;
                    }
                    let i = match self.project_plans_table_state.selected() {
                        Some(i) if i >= self.project_plans.len() - 1 => 0,
                        Some(i) => i + 1,
                        None => 0,
                    };
                    self.project_plans_table_state.select(Some(i));
                }
                ProjectSubTab::Files => {
                    if self.project_files.is_empty() {
                        return;
                    }
                    let i = match self.project_files_table_state.selected() {
                        Some(i) if i >= self.project_files.len() - 1 => 0,
                        Some(i) => i + 1,
                        None => 0,
                    };
                    self.project_files_table_state.select(Some(i));
                }
                _ => {}
            }
        }
    }

    /// Navigate in project sub-tab lists (previous).
    fn project_list_previous(&mut self) {
        if let ViewMode::ProjectDetail { sub_tab, .. } = &self.view_mode {
            match sub_tab {
                ProjectSubTab::Sessions => {
                    if self.project_sessions.is_empty() {
                        return;
                    }
                    let i = match self.project_sessions_table_state.selected() {
                        Some(0) => self.project_sessions.len() - 1,
                        Some(i) => i - 1,
                        None => 0,
                    };
                    self.project_sessions_table_state.select(Some(i));
                }
                ProjectSubTab::Plans => {
                    if self.project_plans.is_empty() {
                        return;
                    }
                    let i = match self.project_plans_table_state.selected() {
                        Some(0) => self.project_plans.len() - 1,
                        Some(i) => i - 1,
                        None => 0,
                    };
                    self.project_plans_table_state.select(Some(i));
                }
                ProjectSubTab::Files => {
                    if self.project_files.is_empty() {
                        return;
                    }
                    let i = match self.project_files_table_state.selected() {
                        Some(0) => self.project_files.len() - 1,
                        Some(i) => i - 1,
                        None => 0,
                    };
                    self.project_files_table_state.select(Some(i));
                }
                _ => {}
            }
        }
    }

    /// Open the selected item in a project sub-tab.
    fn open_project_item(&mut self) {
        if let ViewMode::ProjectDetail {
            sub_tab,
            project_id,
            project_name,
        } = self.view_mode.clone()
        {
            match sub_tab {
                ProjectSubTab::Sessions => {
                    // Open the selected session in detail view
                    if let Some(idx) = self.project_sessions_table_state.selected() {
                        if let Some(session_row) = self.project_sessions.get(idx) {
                            let session_id = session_row.id.clone();
                            let session_name = format!("Session {}", session_row.short_id());

                            // Load messages for this session (merged across all threads)
                            if let Ok(messages) =
                                self.db.get_session_messages(&session_id, 10_000)
                            {
                                // Load threads for this session
                                let threads = self
                                    .db
                                    .get_session_threads(&session_id)
                                    .unwrap_or_default();

                                // Save return destination
                                self.return_to_project = Some((project_id, project_name, sub_tab));
                                self.session_messages = messages;
                                self.session_threads = threads;
                                self.session_scroll_offset = 0;

                                // Load session analytics
                                self.load_session_analytics(&session_id);

                                self.view_mode = ViewMode::SessionDetail {
                                    session_id,
                                    session_name,
                                };
                            }
                        }
                    }
                }
                ProjectSubTab::Plans => {
                    // Open the selected plan in detail view
                    if let Some(idx) = self.project_plans_table_state.selected() {
                        if let Some(plan) = self.project_plans.get(idx) {
                            let plan_slug = plan.id.clone();
                            let plan_title =
                                plan.title.clone().unwrap_or_else(|| plan_slug.clone());

                            // Save return destination
                            self.return_to_project = Some((project_id, project_name, sub_tab));
                            self.selected_plan = Some(plan.clone());
                            self.plan_scroll_offset = 0;
                            self.view_mode = ViewMode::PlanDetail {
                                plan_slug,
                                plan_title,
                            };
                        }
                    }
                }
                _ => {
                    // No action for Overview or Files
                }
            }
        }
    }
}
