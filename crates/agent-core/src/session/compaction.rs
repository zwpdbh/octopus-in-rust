use llm_provider::message::Message;

/// Decides when and how to compact conversation history.
#[async_trait::async_trait]
pub trait CompactionPolicy: Send + Sync {
    /// Given the current history, return a compacted replacement or `None` if
    /// no compaction should happen.
    async fn maybe_compact(&self, history: &[Message]) -> Option<Vec<Message>>;
}

/// Default no-op compaction policy that never compacts.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpCompaction;

#[async_trait::async_trait]
impl CompactionPolicy for NoOpCompaction {
    async fn maybe_compact(&self, _history: &[Message]) -> Option<Vec<Message>> {
        None
    }
}
