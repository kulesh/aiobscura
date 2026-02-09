use super::*;

impl App {
    // ========== Wrapped View Methods ==========

    /// Handle keyboard input in wrapped view.
    pub(super) fn handle_wrapped_key(&mut self, key: KeyEvent) {
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
    pub(super) fn open_wrapped_view(&mut self) {
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
    pub(super) fn close_wrapped_view(&mut self) {
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
    pub(super) fn next_wrapped_card(&mut self) {
        let max = self.wrapped_card_count();
        if max > 0 && self.wrapped_card_index < max - 1 {
            self.wrapped_card_index += 1;
        }
    }

    /// Move to the previous wrapped card.
    pub(super) fn prev_wrapped_card(&mut self) {
        if self.wrapped_card_index > 0 {
            self.wrapped_card_index -= 1;
        }
    }

    /// Toggle between year and month period.
    pub(super) fn toggle_wrapped_period(&mut self) {
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
        match generate_wrapped(&self.db, self.wrapped_period, &config) {
            Ok(stats) => {
                self.wrapped_cache
                    .insert(self.wrapped_period, stats.clone());
                self.wrapped_stats = Some(stats);
                self.wrapped_card_index = 0;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to generate wrapped stats");
            }
        }
    }

    /// Go to previous month (or switch to month view if in year view).
    pub(super) fn prev_wrapped_month(&mut self) {
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
    pub(super) fn next_wrapped_month(&mut self) {
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
    pub(super) fn load_wrapped_for_period(&mut self) {
        // Check cache first
        if let Some(cached) = self.wrapped_cache.get(&self.wrapped_period) {
            self.wrapped_stats = Some(cached.clone());
            self.wrapped_card_index = 0;
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
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to generate wrapped stats");
            }
        }
    }
}
