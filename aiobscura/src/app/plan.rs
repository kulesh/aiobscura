use super::*;

impl App {
    // ========== Plan View Methods ==========

    /// Handle keyboard input in plan list view.
    pub(super) fn handle_plan_list_key(&mut self, key: KeyEvent) {
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
    pub(super) fn handle_plan_detail_key(&mut self, key: KeyEvent) {
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
    pub(super) fn open_plan_list(&mut self, from_detail: bool) {
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
                match self.db.get_plans_for_session(&session_id) {
                    Ok(plans) => {
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
                    Err(e) => {
                        tracing::warn!(
                            session_id = %session_id,
                            error = %e,
                            "Failed to load plans for session"
                        );
                    }
                }
            }
        }
    }

    /// Close plan list and return to previous view.
    pub(super) fn close_plan_list(&mut self) {
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
                    match self.db.get_thread_messages(&thread_id, 10000) {
                        Ok(messages) => {
                            self.messages = messages;
                            self.scroll_offset = 0;
                            self.view_mode = ViewMode::Detail {
                                thread_id,
                                thread_name,
                            };
                        }
                        Err(e) => {
                            tracing::warn!(
                                thread_id = %thread_id,
                                error = %e,
                                "Failed to reload thread messages while closing plan list"
                            );
                            self.view_mode = ViewMode::List;
                        }
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
    pub(super) fn open_plan_detail(&mut self) {
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
    pub(super) fn close_plan_detail(&mut self) {
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
    pub(super) fn select_next_plan(&mut self) {
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
    pub(super) fn select_previous_plan(&mut self) {
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
    pub(super) fn select_first_plan(&mut self) {
        if !self.plans.is_empty() {
            self.plan_table_state.select(Some(0));
        }
    }

    /// Select the last plan.
    pub(super) fn select_last_plan(&mut self) {
        if !self.plans.is_empty() {
            self.plan_table_state.select(Some(self.plans.len() - 1));
        }
    }
}
