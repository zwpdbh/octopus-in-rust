/// Input to a single Brain turn.
#[derive(Debug, Clone)]
pub struct TurnInput {
    /// The user message for this turn.
    pub user_message: String,
}

impl From<String> for TurnInput {
    fn from(value: String) -> Self {
        Self {
            user_message: value,
        }
    }
}

impl From<&str> for TurnInput {
    fn from(value: &str) -> Self {
        Self {
            user_message: value.to_string(),
        }
    }
}

/// Result of running a Brain turn to completion.
#[derive(Debug, Clone)]
pub struct TurnResult {
    /// All events emitted during the turn.
    pub events: Vec<crate::core::events::BrainEvent>,

    /// The final assistant text after any tool calls have been resolved.
    pub final_text: String,
}
