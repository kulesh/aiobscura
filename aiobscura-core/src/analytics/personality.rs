//! Personality classification for Wrapped
//!
//! Assigns fun "coding personality" archetypes based on usage patterns.

/// Coding personality archetypes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Personality {
    /// High Read:Edit ratio - loves exploring before changing
    Archaeologist,
    /// High agent spawning - delegates to sub-agents
    Delegator,
    /// Long sessions, few edits - thinks deeply before acting
    Philosopher,
    /// Short sessions, many edits - quick iterations
    Sprinter,
    /// Heavy Bash usage - command line is home
    TerminalDweller,
    /// Lots of plans - plans before building
    Architect,
    /// Most activity late night (10pm-4am)
    NightOwl,
    /// Most activity early morning (5am-9am)
    EarlyBird,
    /// Works on many projects
    Explorer,
    /// Focuses deeply on one project
    DeepDiver,
}

impl Personality {
    /// Get the display name for this personality.
    pub fn name(&self) -> &'static str {
        match self {
            Personality::Archaeologist => "The Archaeologist",
            Personality::Delegator => "The Delegator",
            Personality::Philosopher => "The Philosopher",
            Personality::Sprinter => "The Sprinter",
            Personality::TerminalDweller => "The Terminal Dweller",
            Personality::Architect => "The Architect",
            Personality::NightOwl => "The Night Owl",
            Personality::EarlyBird => "The Early Bird",
            Personality::Explorer => "The Explorer",
            Personality::DeepDiver => "The Deep Diver",
        }
    }

    /// Get the tagline for this personality.
    pub fn tagline(&self) -> &'static str {
        match self {
            Personality::Archaeologist => "You love exploring before changing",
            Personality::Delegator => "Why do it yourself when agents can help?",
            Personality::Philosopher => "You think deeply before acting",
            Personality::Sprinter => "Quick iterations are your style",
            Personality::TerminalDweller => "Command line is your home",
            Personality::Architect => "You plan before you build",
            Personality::NightOwl => "Your best code happens after midnight",
            Personality::EarlyBird => "Dawn is your productive time",
            Personality::Explorer => "Variety is the spice of code",
            Personality::DeepDiver => "Focused dedication",
        }
    }

    /// Get an emoji for this personality.
    pub fn emoji(&self) -> &'static str {
        match self {
            Personality::Archaeologist => "🔍",
            Personality::Delegator => "👥",
            Personality::Philosopher => "🤔",
            Personality::Sprinter => "⚡",
            Personality::TerminalDweller => "💻",
            Personality::Architect => "📐",
            Personality::NightOwl => "🦉",
            Personality::EarlyBird => "🐦",
            Personality::Explorer => "🧭",
            Personality::DeepDiver => "🤿",
        }
    }
}

/// Usage profile for personality classification.
#[derive(Debug, Clone, Default)]
pub struct UsageProfile {
    /// Read tool calls / Edit tool calls
    pub read_to_edit_ratio: f64,
    /// Agent spawns / total sessions
    pub agent_spawn_rate: f64,
    /// Average session duration in seconds
    pub avg_session_duration_secs: f64,
    /// Edit calls / sessions
    pub edits_per_session: f64,
    /// Bash calls / total tool calls
    pub bash_percentage: f64,
    /// Plans / sessions
    pub plans_per_session: f64,
    /// Activity between 10pm-4am / total activity
    pub late_night_percentage: f64,
    /// Activity between 5am-9am / total activity
    pub early_morning_percentage: f64,
    /// Unique projects / sessions (higher = more variety)
    pub project_diversity: f64,
    /// Percentage of sessions on top project
    pub top_project_concentration: f64,
}

impl UsageProfile {
    /// Classify the dominant personality based on the profile.
    pub fn classify(&self) -> Personality {
        // Score each personality trait
        let mut scores: Vec<(Personality, f64)> = vec![
            (Personality::Archaeologist, self.archaeologist_score()),
            (Personality::Delegator, self.delegator_score()),
            (Personality::Philosopher, self.philosopher_score()),
            (Personality::Sprinter, self.sprinter_score()),
            (Personality::TerminalDweller, self.terminal_dweller_score()),
            (Personality::Architect, self.architect_score()),
            (Personality::NightOwl, self.night_owl_score()),
            (Personality::EarlyBird, self.early_bird_score()),
            (Personality::Explorer, self.explorer_score()),
            (Personality::DeepDiver, self.deep_diver_score()),
        ];

        // Sort by score descending
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Return highest scoring personality
        scores.first().map(|(p, _)| *p).unwrap_or(Personality::Explorer)
    }

    fn archaeologist_score(&self) -> f64 {
        // High Read:Edit ratio indicates exploration
        if self.read_to_edit_ratio > 2.0 {
            (self.read_to_edit_ratio - 2.0).min(5.0) / 5.0
        } else {
            0.0
        }
    }

    fn delegator_score(&self) -> f64 {
        // High agent spawn rate
        if self.agent_spawn_rate > 0.5 {
            (self.agent_spawn_rate - 0.5).min(2.0) / 2.0
        } else {
            0.0
        }
    }

    fn philosopher_score(&self) -> f64 {
        // Long sessions with relatively few edits
        let duration_factor = (self.avg_session_duration_secs / 3600.0).min(2.0) / 2.0;
        let low_edit_factor = if self.edits_per_session < 10.0 {
            1.0 - (self.edits_per_session / 10.0)
        } else {
            0.0
        };
        duration_factor * 0.6 + low_edit_factor * 0.4
    }

    fn sprinter_score(&self) -> f64 {
        // Short sessions with many edits
        let short_factor = if self.avg_session_duration_secs < 1800.0 {
            1.0 - (self.avg_session_duration_secs / 1800.0)
        } else {
            0.0
        };
        let high_edit_factor = (self.edits_per_session / 50.0).min(1.0);
        short_factor * 0.4 + high_edit_factor * 0.6
    }

    fn terminal_dweller_score(&self) -> f64 {
        // High Bash percentage
        if self.bash_percentage > 0.15 {
            ((self.bash_percentage - 0.15) / 0.35).min(1.0)
        } else {
            0.0
        }
    }

    fn architect_score(&self) -> f64 {
        // High plans per session
        if self.plans_per_session > 0.1 {
            ((self.plans_per_session - 0.1) / 0.4).min(1.0)
        } else {
            0.0
        }
    }

    fn night_owl_score(&self) -> f64 {
        // High late night activity
        if self.late_night_percentage > 0.2 {
            ((self.late_night_percentage - 0.2) / 0.3).min(1.0)
        } else {
            0.0
        }
    }

    fn early_bird_score(&self) -> f64 {
        // High early morning activity
        if self.early_morning_percentage > 0.2 {
            ((self.early_morning_percentage - 0.2) / 0.3).min(1.0)
        } else {
            0.0
        }
    }

    fn explorer_score(&self) -> f64 {
        // High project diversity
        if self.project_diversity > 0.3 {
            ((self.project_diversity - 0.3) / 0.4).min(1.0)
        } else {
            0.0
        }
    }

    fn deep_diver_score(&self) -> f64 {
        // High concentration on single project
        if self.top_project_concentration > 0.6 {
            ((self.top_project_concentration - 0.6) / 0.3).min(1.0)
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_archaeologist_classification() {
        let profile = UsageProfile {
            read_to_edit_ratio: 5.0,
            ..Default::default()
        };
        assert_eq!(profile.classify(), Personality::Archaeologist);
    }

    #[test]
    fn test_terminal_dweller_classification() {
        let profile = UsageProfile {
            bash_percentage: 0.5,
            ..Default::default()
        };
        assert_eq!(profile.classify(), Personality::TerminalDweller);
    }

    #[test]
    fn test_architect_classification() {
        let profile = UsageProfile {
            plans_per_session: 0.5,
            ..Default::default()
        };
        assert_eq!(profile.classify(), Personality::Architect);
    }

    #[test]
    fn test_night_owl_classification() {
        let profile = UsageProfile {
            late_night_percentage: 0.5,
            ..Default::default()
        };
        assert_eq!(profile.classify(), Personality::NightOwl);
    }

    #[test]
    fn test_personality_display() {
        assert_eq!(Personality::Architect.name(), "The Architect");
        assert_eq!(Personality::Architect.emoji(), "📐");
    }
}
