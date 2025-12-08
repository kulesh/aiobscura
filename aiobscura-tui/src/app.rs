//! Application state for the TUI.

use std::collections::HashMap;

use aiobscura_core::db::{FileStats, ToolStats};
use aiobscura_core::{Database, Message, Plan, SessionFilter, Thread, ThreadType};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use crate::thread_row::ThreadRow;

/// Current view mode
#[derive(Debug, Clone, Default)]
pub enum ViewMode {
    /// Thread list view
    #[default]
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
    /// Whether the app should exit
    pub should_quit: bool,
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
            should_quit: false,
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
                children_map
                    .entry(parent_id.clone())
                    .or_default()
                    .push(idx);
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

                self.threads.push(ThreadRow {
                    id: main_info.thread.id.clone(),
                    session_id: main_info.session_id.clone(),
                    thread_type: main_info.thread.thread_type,
                    parent_thread_id: None,
                    last_activity: main_info.thread.ended_at.or(Some(main_info.thread.started_at)),
                    project_name: main_info.project_name.clone(),
                    assistant_name: main_info.assistant_name.clone(),
                    message_count,
                    indent_level: 0,
                });

                // Add child threads (agents spawned by this main thread)
                if let Some(child_indices) = children_map.get(&main_info.thread.id) {
                    let mut children: Vec<&ThreadInfo> = child_indices
                        .iter()
                        .map(|&idx| &all_threads[idx])
                        .collect();
                    // Sort children by started_at
                    children.sort_by(|a, b| a.thread.started_at.cmp(&b.thread.started_at));

                    for child_info in children {
                        let message_count = self
                            .db
                            .count_thread_messages(&child_info.thread.id)
                            .unwrap_or(0);

                        self.threads.push(ThreadRow {
                            id: child_info.thread.id.clone(),
                            session_id: child_info.session_id.clone(),
                            thread_type: child_info.thread.thread_type,
                            parent_thread_id: child_info.thread.parent_thread_id.clone(),
                            last_activity: child_info
                                .thread
                                .ended_at
                                .or(Some(child_info.thread.started_at)),
                            project_name: child_info.project_name.clone(),
                            assistant_name: child_info.assistant_name.clone(),
                            message_count,
                            indent_level: 1,
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

                self.threads.push(ThreadRow {
                    id: orphan_info.thread.id.clone(),
                    session_id: orphan_info.session_id.clone(),
                    thread_type: orphan_info.thread.thread_type,
                    parent_thread_id: None,
                    last_activity: orphan_info
                        .thread
                        .ended_at
                        .or(Some(orphan_info.thread.started_at)),
                    project_name: orphan_info.project_name.clone(),
                    assistant_name: orphan_info.assistant_name.clone(),
                    message_count,
                    indent_level: 0,
                });
            }
        }

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
            KeyCode::Char('p') => {
                self.open_plan_list(false);
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
            KeyCode::PageDown | KeyCode::Char('d') => {
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

    /// Close detail view and return to list.
    fn close_detail_view(&mut self) {
        self.view_mode = ViewMode::List;
        self.messages.clear();
        self.scroll_offset = 0;
        self.thread_metadata = None;
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
                    let lines = plan.content.as_ref().map(|c| c.lines().count()).unwrap_or(0);
                    self.plan_scroll_offset = lines.saturating_sub(1);
                }
            }
            KeyCode::PageDown | KeyCode::Char('d') => {
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
            if let ViewMode::Detail { thread_id, thread_name } = &self.view_mode {
                Some((thread_id.clone(), thread_name.clone()))
            } else {
                None
            }
        } else {
            // We're in list view - get selected thread
            self.table_state.selected().and_then(|idx| {
                self.threads.get(idx).map(|t| {
                    (t.id.clone(), format!("{} - {}", t.project_name, t.short_id()))
                })
            })
        };

        if let Some((thread_id, thread_name)) = thread {
            // Find the thread to get session_id
            if let Some(thread_row) = self.threads.iter().find(|t| t.id == thread_id) {
                let session_id = thread_row.session_id.clone();
                let session_name = format!("{} - {}", thread_row.project_name, &session_id[..8.min(session_id.len())]);

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
                if let (Some(thread_id), Some(thread_name)) = (return_thread_id.clone(), return_thread_name.clone()) {
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
        // We need to reconstruct the PlanList state
        // For now, find the session from the selected plan
        if let Some(plan) = &self.selected_plan {
            let session_id = plan.session_id.clone();
            // Find a thread with this session to get session name
            if let Some(thread) = self.threads.iter().find(|t| t.session_id == session_id) {
                let session_name = format!("{} - {}", thread.project_name, &session_id[..8.min(session_id.len())]);
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
}
