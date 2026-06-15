use kosong::message::Message;

/// Stores the message history for a Brain session.
///
/// The default implementation keeps messages in memory. Future implementations
/// may persist to disk or a database.
pub trait MessageStore: Send + Sync {
    /// Append a message to the store.
    fn push(&mut self, message: Message);

    /// Return the current history.
    fn history(&self) -> &[Message];

    /// Replace the entire history.
    fn set_history(&mut self, history: Vec<Message>);

    /// Clear all messages.
    fn clear(&mut self);
}

/// In-memory message store.
#[derive(Debug, Clone, Default)]
pub struct InMemoryMessageStore {
    history: Vec<Message>,
}

impl InMemoryMessageStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl MessageStore for InMemoryMessageStore {
    fn push(&mut self, message: Message) {
        self.history.push(message);
    }

    fn history(&self) -> &[Message] {
        &self.history
    }

    fn set_history(&mut self, history: Vec<Message>) {
        self.history = history;
    }

    fn clear(&mut self) {
        self.history.clear();
    }
}
