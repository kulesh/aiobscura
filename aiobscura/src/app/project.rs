use super::*;

impl App {
    // ========== Project View Methods ==========

    /// Handle keyboard input in project list view.
    pub(super) fn handle_project_list_key(&mut self, key: KeyEvent) {
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
    pub(super) fn handle_project_detail_key(&mut self, key: KeyEvent) {
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
    pub(super) fn handle_session_detail_key(&mut self, key: KeyEvent) {
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
    pub(super) fn close_session_detail(&mut self) {
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
        self.session_first_order_metrics = None;
        self.session_first_order_error = None;
    }

    /// Close project list and switch to thread list.
    pub(super) fn close_project_list(&mut self) {
        // Always reload threads and stats for fresh data
        if let Err(e) = self.load_threads() {
            tracing::warn!(error = %e, "Failed to load threads while closing project list");
        }
        self.refresh_dashboard_stats();
        self.view_mode = ViewMode::List;
    }

    /// Open project detail for the selected project.
    pub(super) fn open_project_detail(&mut self) {
        if let Some(idx) = self.project_table_state.selected() {
            if let Some(project) = self.projects.get(idx) {
                let project_id = project.id.clone();
                let project_name = project.name.clone();

                // Load project stats
                match self.db.get_project_stats(&project_id) {
                    Ok(Some(stats)) => {
                        self.project_stats = Some(stats);
                        self.view_mode = ViewMode::ProjectDetail {
                            project_id,
                            project_name,
                            sub_tab: ProjectSubTab::Overview,
                        };
                    }
                    Ok(None) => {
                        tracing::warn!(project_id = %project_id, "Project stats missing for selected project");
                    }
                    Err(e) => {
                        tracing::warn!(project_id = %project_id, error = %e, "Failed to load project stats");
                    }
                }
            }
        }
    }

    /// Close project detail and return to project list.
    pub(super) fn close_project_detail(&mut self) {
        self.view_mode = ViewMode::ProjectList;
        self.project_stats = None;
    }

    /// Select the next project in the list.
    pub(super) fn select_next_project(&mut self) {
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
    pub(super) fn select_previous_project(&mut self) {
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
    pub(super) fn select_first_project(&mut self) {
        if !self.projects.is_empty() {
            self.project_table_state.select(Some(0));
        }
    }

    /// Select the last project.
    pub(super) fn select_last_project(&mut self) {
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
        self.refresh_dashboard_stats();
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

    /// Returns true if current view is Live.
    pub fn is_live_view(&self) -> bool {
        matches!(self.view_mode, ViewMode::Live)
    }

    /// Refresh data for the current view, preserving selection state.
    pub fn refresh_current_view(&mut self) -> Result<()> {
        // Always refresh dashboard stats for list views
        if self.is_list_view() {
            self.refresh_dashboard_stats();
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
    pub(super) fn load_project_sessions(&mut self, project_id: &str) -> Result<()> {
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
    pub(super) fn load_project_plans(&mut self, project_id: &str) -> Result<()> {
        self.project_plans = self.db.list_project_plans(project_id)?;

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
    pub(super) fn load_project_files(&mut self, project_id: &str) -> Result<()> {
        // Use the file_stats from ProjectStats if available
        if let Some(stats) = &self.project_stats {
            self.project_files = stats.file_stats.breakdown.clone();
        } else {
            match self.db.get_project_stats(project_id) {
                Ok(Some(stats)) => self.project_files = stats.file_stats.breakdown.clone(),
                Ok(None) => self.project_files.clear(),
                Err(e) => {
                    tracing::warn!(
                        project_id = %project_id,
                        error = %e,
                        "Failed to load project files from project stats"
                    );
                    self.project_files.clear();
                }
            }
        }

        // Select first if any
        self.project_files_table_state = TableState::default();
        if !self.project_files.is_empty() {
            self.project_files_table_state.select(Some(0));
        }

        Ok(())
    }

    /// Cycle to the next/previous project sub-tab.
    pub(super) fn cycle_project_sub_tab(&mut self, forward: bool) {
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
    pub(super) fn set_project_sub_tab(&mut self, sub_tab: ProjectSubTab) {
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
                    if let Err(e) = self.load_project_sessions(&project_id) {
                        tracing::warn!(
                            project_id = %project_id,
                            error = %e,
                            "Failed to load project sessions for sub-tab switch"
                        );
                    }
                }
                ProjectSubTab::Plans => {
                    if let Err(e) = self.load_project_plans(&project_id) {
                        tracing::warn!(
                            project_id = %project_id,
                            error = %e,
                            "Failed to load project plans for sub-tab switch"
                        );
                    }
                }
                ProjectSubTab::Files => {
                    if let Err(e) = self.load_project_files(&project_id) {
                        tracing::warn!(
                            project_id = %project_id,
                            error = %e,
                            "Failed to load project files for sub-tab switch"
                        );
                    }
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
    pub(super) fn project_list_next(&mut self) {
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
    pub(super) fn project_list_previous(&mut self) {
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
    pub(super) fn open_project_item(&mut self) {
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
                            match self.db.get_session_messages(&session_id, 10_000) {
                                Ok(messages) => {
                                    // Load threads for this session
                                    let threads = match self.db.get_session_threads(&session_id) {
                                        Ok(threads) => threads,
                                        Err(e) => {
                                            tracing::warn!(
                                                session_id = %session_id,
                                                error = %e,
                                                "Failed to load session threads"
                                            );
                                            Vec::new()
                                        }
                                    };

                                    // Save return destination
                                    self.return_to_project =
                                        Some((project_id, project_name, sub_tab));
                                    self.session_messages = messages;
                                    self.session_threads = threads;
                                    self.session_scroll_offset = 0;

                                    // Load session analytics
                                    self.load_session_analytics(&session_id);
                                    self.load_first_order_metrics(&session_id);

                                    self.view_mode = ViewMode::SessionDetail {
                                        session_id,
                                        session_name,
                                    };
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        session_id = %session_id,
                                        error = %e,
                                        "Failed to load session messages"
                                    );
                                }
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
