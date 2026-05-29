use std::path::Path;

use crate::wire::Message;

/// A dynamic prompt content to be injected before an LLM step.
#[derive(Debug, Clone)]
pub struct DynamicInjection {
    /// Identifier, e.g. "plan_mode" or "afk_mode".
    pub typ: String,
    /// Text content (will be wrapped in `<system-reminder>` tags by the caller).
    pub content: String,
}

/// Context snapshot passed to injection providers so they can query soul state
/// without creating a circular reference on `KimiSoul`.
pub struct InjectionContext<'a> {
    pub plan_mode: bool,
    /// Effective AFK state — true when the user is away (includes both
    /// persisted session AFK and runtime invocation AFK like `--afk`).
    pub effective_afk: bool,
    /// Persisted session AFK flag — true only when the session itself
    /// is in AFK mode (not counting runtime-only overlays).
    pub persisted_afk: bool,
    pub plan_file_path: Option<&'a Path>,
    pub pending_plan_activation: bool,
}

/// Base trait for dynamic injection providers.
///
/// Called before each LLM step. Implementations handle their own throttling.
#[async_trait::async_trait]
pub trait DynamicInjectionProvider: Send + Sync {
    /// Produce injections for the current step.
    async fn get_injections(
        &mut self,
        history: &[Message],
        ctx: &InjectionContext<'_>,
    ) -> Vec<DynamicInjection>;

    /// Called after the context is compacted (history is rebuilt).
    ///
    /// Override to reset internal throttling state when prior injections
    /// may have been collapsed into the compaction summary and are no
    /// longer literally present in history. Default is a no-op.
    async fn on_context_compacted(&mut self) {}

    /// Called when afk mode is toggled at runtime.
    ///
    /// Override to reset internal throttling state when a mode-specific
    /// reminder should be eligible to fire again after a user toggle.
    async fn on_afk_changed(&mut self, _enabled: bool) {}
}
