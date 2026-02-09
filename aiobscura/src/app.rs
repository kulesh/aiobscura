//! Application state for the TUI.

mod live;
mod plan;
mod project;
mod wrapped;

use std::collections::HashMap;

use aiobscura_core::analytics::{
    generate_wrapped, DashboardStats, FirstOrderSessionMetrics, ProjectRow, ProjectStats,
    SessionAnalytics, ThreadAnalytics, WrappedConfig, WrappedPeriod, WrappedStats,
};
use aiobscura_core::db::{EnvironmentHealth, ThreadMetadata};
use aiobscura_core::{
    ActiveSession, Database, LiveStats, Message, MessageWithContext, Plan, Thread, ThreadType,
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

#[cfg(test)]
mod tests {
    use super::*;
    use aiobscura_core::Project;
    use crossterm::event::KeyModifiers;
    use std::path::PathBuf;

    fn create_test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.migrate().expect("migrate");
        db
    }

    fn create_test_project(id: &str, path: &str, name: &str) -> Project {
        Project {
            id: id.to_string(),
            path: PathBuf::from(path),
            name: Some(name.to_string()),
            created_at: chrono::Utc::now(),
            last_activity_at: None,
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn start_live_view_sets_live_mode() {
        let db = create_test_db();
        let mut app = App::new(db);

        app.start_live_view().expect("start live view");

        assert!(matches!(app.view_mode, ViewMode::Live));
        assert_eq!(app.live_scroll_offset, 0);
        assert!(app.live_auto_scroll);
    }

    #[test]
    fn live_escape_returns_to_project_list() {
        let db = create_test_db();
        let mut app = App::new(db);
        app.start_live_view().expect("start live view");

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(matches!(app.view_mode, ViewMode::ProjectList));
    }

    #[test]
    fn refresh_current_view_preserves_project_selection() {
        let db = create_test_db();
        db.upsert_project(&create_test_project("p1", "/tmp/project-1", "Project 1"))
            .expect("insert project 1");
        db.upsert_project(&create_test_project("p2", "/tmp/project-2", "Project 2"))
            .expect("insert project 2");

        let mut app = App::new(db);
        app.load_projects().expect("load projects");
        app.project_table_state.select(Some(1));

        app.refresh_current_view().expect("refresh current view");

        assert_eq!(app.project_table_state.selected(), Some(1));
    }
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
    /// First-order metrics for current session (detail view)
    pub session_first_order_metrics: Option<FirstOrderSessionMetrics>,
    /// Error message if first-order metrics computation failed
    pub session_first_order_error: Option<String>,
    /// Analytics for current thread (detail view)
    pub thread_analytics: Option<ThreadAnalytics>,
    /// Error message if thread analytics computation failed
    pub thread_analytics_error: Option<String>,

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
    /// Aggregate stats for the live view's toolbar (30 min window)
    pub live_stats: LiveStats,
    /// Aggregate stats for 24 hour window
    pub live_stats_24h: LiveStats,
    /// Environment health stats
    pub environment_health: EnvironmentHealth,
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
            session_first_order_metrics: None,
            session_first_order_error: None,
            thread_analytics: None,
            thread_analytics_error: None,

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
            live_stats_24h: LiveStats::default(),
            environment_health: EnvironmentHealth::default(),
        }
    }

    /// Load environment health stats from the database.
    fn load_environment_health(&mut self) -> Result<()> {
        self.environment_health = self.db.get_environment_health()?;
        Ok(())
    }

    /// Refresh supporting data shown in live view side panels.
    fn refresh_live_supporting_data(&mut self) -> Result<()> {
        // Get sessions active in last 30 minutes for the panel.
        self.active_sessions = self.db.get_active_sessions(30)?;
        // Load stats for both time windows (30m and 24h).
        self.live_stats = self.db.get_live_stats(30)?;
        self.live_stats_24h = self.db.get_live_stats(24 * 60)?;
        // Load dashboard stats and projects for the dashboard panel.
        self.dashboard_stats = Some(self.db.get_dashboard_stats()?);
        self.projects = self.db.list_projects_with_stats()?;
        // Load environment health.
        self.load_environment_health()?;
        Ok(())
    }

    /// Refresh dashboard stats and log failures instead of silently dropping them.
    fn refresh_dashboard_stats(&mut self) {
        match self.db.get_dashboard_stats() {
            Ok(stats) => self.dashboard_stats = Some(stats),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load dashboard stats");
                self.dashboard_stats = None;
            }
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
        let summaries = self.db.list_threads_with_counts()?;
        self.threads.clear();

        // Collect all threads with their session info
        struct ThreadInfo {
            thread: Thread,
            project_name: String,
            assistant_name: String,
            message_count: i64,
        }

        let mut all_threads: Vec<ThreadInfo> = Vec::new();

        for summary in summaries {
            let project_name = summary
                .project_name
                .unwrap_or_else(|| "(no project)".to_string());
            let assistant_name = summary.assistant.display_name().to_string();

            all_threads.push(ThreadInfo {
                thread: summary.thread,
                project_name,
                assistant_name,
                message_count: summary.message_count,
            });
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
            let Some(threads) = project_threads.get(&project_name) else {
                continue;
            };

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
                let message_count = main_info.message_count;
                let last_activity = main_info
                    .thread
                    .last_activity_at
                    .or(main_info.thread.ended_at)
                    .or(Some(main_info.thread.started_at));

                self.threads.push(ThreadRow {
                    id: main_info.thread.id.clone(),
                    session_id: main_info.thread.session_id.clone(),
                    thread_type: main_info.thread.thread_type,
                    agent_subtype: main_info.thread.agent_subtype().map(|s| s.to_string()),
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
                        let message_count = child_info.message_count;
                        let last_activity = child_info
                            .thread
                            .last_activity_at
                            .or(child_info.thread.ended_at)
                            .or(Some(child_info.thread.started_at));

                        self.threads.push(ThreadRow {
                            id: child_info.thread.id.clone(),
                            session_id: child_info.thread.session_id.clone(),
                            thread_type: child_info.thread.thread_type,
                            agent_subtype: child_info.thread.agent_subtype().map(|s| s.to_string()),
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
                let message_count = orphan_info.message_count;
                let last_activity = orphan_info
                    .thread
                    .last_activity_at
                    .or(orphan_info.thread.ended_at)
                    .or(Some(orphan_info.thread.started_at));

                self.threads.push(ThreadRow {
                    id: orphan_info.thread.id.clone(),
                    session_id: orphan_info.thread.session_id.clone(),
                    thread_type: orphan_info.thread.thread_type,
                    agent_subtype: orphan_info.thread.agent_subtype().map(|s| s.to_string()),
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
                match self.db.get_thread_messages(&thread_id, 10000) {
                    Ok(messages) => {
                        self.messages = messages;
                        self.scroll_offset = 0;

                        // Load metadata for the header
                        self.thread_metadata = self.load_thread_metadata(&thread_id);

                        // Load/compute analytics (both session and thread)
                        self.load_session_analytics(&session_id);
                        self.load_first_order_metrics(&session_id);
                        self.load_thread_analytics(&thread_id);

                        self.view_mode = ViewMode::Detail {
                            thread_id,
                            thread_name,
                        };
                    }
                    Err(e) => {
                        tracing::warn!(thread_id = %thread_id, error = %e, "Failed to load thread messages");
                    }
                }
            }
        }
    }

    /// Load metadata for the detail view header.
    fn load_thread_metadata(&self, thread_id: &str) -> Option<ThreadMetadata> {
        match self.db.get_thread_metadata(thread_id) {
            Ok(metadata) => metadata,
            Err(e) => {
                tracing::warn!(thread_id = %thread_id, error = %e, "Failed to load thread metadata");
                None
            }
        }
    }

    /// Load or compute session analytics for the detail view.
    fn load_session_analytics(&mut self, session_id: &str) {
        match aiobscura_core::analytics::ensure_session_analytics(session_id, &self.db) {
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

    /// Load or compute first-order session metrics for the detail view.
    fn load_first_order_metrics(&mut self, session_id: &str) {
        match aiobscura_core::analytics::ensure_first_order_metrics(session_id, &self.db) {
            Ok(metrics) => {
                self.session_first_order_metrics = Some(metrics);
                self.session_first_order_error = None;
            }
            Err(e) => {
                self.session_first_order_metrics = None;
                self.session_first_order_error = Some(e.to_string());
                tracing::warn!(session_id, error = %e, "Failed to compute first-order metrics");
            }
        }
    }

    /// Load or compute thread analytics for the detail view.
    fn load_thread_analytics(&mut self, thread_id: &str) {
        match aiobscura_core::analytics::ensure_thread_analytics(thread_id, &self.db) {
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
        self.session_first_order_metrics = None;
        self.session_first_order_error = None;
        self.thread_analytics = None;
        self.thread_analytics_error = None;
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
}
