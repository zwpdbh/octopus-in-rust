use llm_provider::message::Message;

/// Stores the message history for a Brain session.
///
/// The default implementation keeps messages in memory. Future implementations
/// may persist to disk or a database.
#[async_trait::async_trait]
pub trait MessageStore: Send + Sync {
    /// Append a message to the store.
    async fn push(&mut self, message: Message);

    /// Return the current history.
    async fn history(&self) -> Vec<Message>;

    /// Replace the entire history.
    async fn set_history(&mut self, history: Vec<Message>);

    /// Clear all messages.
    async fn clear(&mut self);
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

#[async_trait::async_trait]
impl MessageStore for InMemoryMessageStore {
    async fn push(&mut self, message: Message) {
        self.history.push(message);
    }

    async fn history(&self) -> Vec<Message> {
        self.history.clone()
    }

    async fn set_history(&mut self, history: Vec<Message>) {
        self.history = history;
    }

    async fn clear(&mut self) {
        self.history.clear();
    }
}
