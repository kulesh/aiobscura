use super::*;

impl App {
    // ========== Live View Methods ==========

    /// Handle keyboard input in live view.
    pub(super) fn handle_live_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Esc | KeyCode::Tab => {
                // Tab cycles: Live -> Projects
                // Always reload for fresh data
                if let Err(e) = self.load_projects() {
                    tracing::warn!(error = %e, "Failed to load projects for project list");
                }
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
            // Number keys 1-5 jump to project by index (quick project switcher)
            KeyCode::Char(c @ '1'..='5') => {
                let idx = (c as usize) - ('1' as usize);
                if idx < self.projects.len() {
                    // Navigate to project detail
                    let project = &self.projects[idx];
                    self.open_project_detail_by_id(&project.id.clone(), &project.name.clone());
                }
            }
            _ => {}
        }
    }

    /// Open project detail view by ID and name (for quick navigation).
    pub(super) fn open_project_detail_by_id(&mut self, project_id: &str, project_name: &str) {
        // Load project stats
        match self.db.get_project_stats(project_id) {
            Ok(Some(stats)) => self.project_stats = Some(stats),
            Ok(None) => self.project_stats = None,
            Err(e) => {
                tracing::warn!(project_id = %project_id, error = %e, "Failed to load project stats");
                self.project_stats = None;
            }
        }
        // Load sessions for the sessions sub-tab
        if let Err(e) = self.load_project_sessions(project_id) {
            tracing::warn!(project_id = %project_id, error = %e, "Failed to load project sessions");
        }
        // Load plans for the plans sub-tab
        if let Err(e) = self.load_project_plans(project_id) {
            tracing::warn!(project_id = %project_id, error = %e, "Failed to load project plans");
        }
        // Load files for the files sub-tab
        if let Err(e) = self.load_project_files(project_id) {
            tracing::warn!(project_id = %project_id, error = %e, "Failed to load project files");
        }

        self.view_mode = ViewMode::ProjectDetail {
            project_id: project_id.to_string(),
            project_name: project_name.to_string(),
            sub_tab: ProjectSubTab::Overview,
        };
    }

    /// Open the live activity view (called from main for startup).
    pub fn start_live_view(&mut self) -> Result<()> {
        self.live_messages = self.db.get_recent_messages(50)?;
        self.refresh_live_supporting_data()?;
        self.live_scroll_offset = 0;
        self.live_auto_scroll = true;
        self.view_mode = ViewMode::Live;
        Ok(())
    }

    /// Open the live activity view (internal navigation).
    pub(super) fn open_live_view(&mut self) {
        // Load recent messages (limit to 50 as per plan)
        match self.db.get_recent_messages(50) {
            Ok(messages) => {
                self.live_messages = messages;
                if let Err(e) = self.refresh_live_supporting_data() {
                    tracing::warn!(error = %e, "Failed to refresh live supporting data");
                }
                self.live_scroll_offset = 0;
                self.live_auto_scroll = true;
                self.view_mode = ViewMode::Live;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load live messages");
            }
        }
    }

    /// Refresh live messages from database.
    pub fn refresh_live_messages(&mut self) -> Result<()> {
        let messages = self.db.get_recent_messages(50)?;
        self.live_messages = messages;
        // If auto-scroll is on, keep at top (showing newest)
        if self.live_auto_scroll {
            self.live_scroll_offset = 0;
        }
        // Also refresh active sessions and stats (both time windows)
        self.active_sessions = self.db.get_active_sessions(30)?;
        self.live_stats = self.db.get_live_stats(30)?;
        self.live_stats_24h = self.db.get_live_stats(24 * 60)?;
        Ok(())
    }
}
