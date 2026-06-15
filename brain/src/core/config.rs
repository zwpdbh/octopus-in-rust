use std::sync::Arc;

use crate::core::approval::{ApprovalRuntime, AutoApprove, DefaultApprovalRuntime};
use crate::core::errors::BrainError;
use crate::core::provider::{DefaultProviderFactory, ProviderFactory};
use crate::core::recovery::{DefaultRecoveryPolicy, RecoveryPolicy};
use crate::core::registry::ToolSource;
use crate::core::retry::{ExponentialBackoffRetryPolicy, RetryPolicy};
use crate::hooks::policy::{HookPolicy, NoOpHookPolicy};
use crate::session::compaction::CompactionPolicy;
use crate::session::injection::InjectionPolicy;
use crate::session::store::{InMemoryMessageStore, MessageStore};

/// Configuration for a Brain instance.
#[derive(Clone)]
pub struct BrainConfig {
    /// System prompt sent on every turn.
    pub system_prompt: String,

    /// Base URL of the LLM API, e.g. `https://api.kimi.com/coding/v1`.
    ///
    /// Used by the default [`ProviderFactory`]; custom factories may ignore this.
    pub base_url: String,

    /// API key or OAuth access token.
    ///
    /// Used by the default [`ProviderFactory`]; custom factories may ignore this.
    pub api_key: String,

    /// Model name, e.g. `kimi-for-coding`.
    ///
    /// Used by the default [`ProviderFactory`]; custom factories may ignore this.
    pub model: String,

    /// Maximum reasoning steps per turn.
    pub max_steps_per_turn: usize,

    /// Maximum retry attempts for a single step before recovery policies run.
    pub max_step_attempts: usize,

    /// Pre-built LLM provider. When `Some`, it takes precedence over the
    /// provider factory for the initial construction.
    pub provider: Option<Arc<dyn kosong::ChatProvider>>,

    /// Factory used to build or rebuild the LLM provider.
    pub provider_factory: Arc<dyn ProviderFactory>,

    /// Runtime that decides whether a tool call may execute.
    pub approval_runtime: Arc<dyn ApprovalRuntime>,

    /// External sources of tools (plugins, skills, MCP).
    pub tool_sources: Vec<Arc<dyn ToolSource>>,

    /// Pre-built toolset. When `Some`, it takes precedence over `tool_sources`
    /// and the Brain will use it directly instead of constructing a registry.
    pub toolset: Option<Arc<dyn kosong::Toolset>>,

    /// Persistent message history.
    pub message_store: Arc<tokio::sync::Mutex<dyn MessageStore>>,

    /// Optional context compaction policy.
    pub compaction_policy: Option<Arc<dyn CompactionPolicy>>,

    /// Optional dynamic context injection policy.
    pub injection_policy: Option<Arc<dyn InjectionPolicy>>,

    /// Hook policy for lifecycle observation/blocking.
    pub hook_policy: Arc<dyn HookPolicy>,

    /// Retry policy for transient step failures.
    pub retry_policy: Arc<dyn RetryPolicy>,

    /// Recovery policy invoked after retries are exhausted.
    pub recovery_policy: Arc<dyn RecoveryPolicy>,
}

impl std::fmt::Debug for BrainConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrainConfig")
            .field("system_prompt", &self.system_prompt)
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .field("max_steps_per_turn", &self.max_steps_per_turn)
            .field("max_step_attempts", &self.max_step_attempts)
            .finish_non_exhaustive()
    }
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            system_prompt: "You are a helpful assistant.".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "gpt-4o".to_string(),
            max_steps_per_turn: 16,
            max_step_attempts: 3,
            provider: None,
            provider_factory: Arc::new(DefaultProviderFactory),
            approval_runtime: Arc::new(DefaultApprovalRuntime::new(Arc::new(AutoApprove))),
            tool_sources: Vec::new(),
            toolset: None,
            message_store: Arc::new(tokio::sync::Mutex::new(InMemoryMessageStore::new())),
            compaction_policy: None,
            injection_policy: None,
            hook_policy: Arc::new(NoOpHookPolicy),
            retry_policy: Arc::new(ExponentialBackoffRetryPolicy::new(3)),
            recovery_policy: Arc::new(DefaultRecoveryPolicy),
        }
    }
}

impl BrainConfig {
    /// Build or rebuild the LLM provider using the configured factory.
    pub async fn build_provider(&self) -> Result<Arc<dyn kosong::ChatProvider>, BrainError> {
        self.provider_factory.create(self).await
    }
}
