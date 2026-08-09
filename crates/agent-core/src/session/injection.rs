use llm_provider::message::Message;

/// Injects additional context/reminders before an agent step.
#[async_trait::async_trait]
pub trait InjectionPolicy: Send + Sync {
    /// Return extra messages to prepend to the history for the next step.
    ///
    /// The returned messages are not persisted; they are used only for the
    /// current step.
    async fn inject(&self, history: &[Message]) -> Vec<Message>;
}

/// Default no-op injection policy.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpInjection;

#[async_trait::async_trait]
impl InjectionPolicy for NoOpInjection {
    async fn inject(&self, _history: &[Message]) -> Vec<Message> {
        Vec::new()
    }
}
