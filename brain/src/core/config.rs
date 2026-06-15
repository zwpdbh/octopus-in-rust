use std::sync::Arc;

use crate::core::approval::{ApprovalPolicy, AutoApprove};
use crate::core::registry::ToolSource;
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
    pub base_url: String,

    /// API key or OAuth access token.
    pub api_key: String,

    /// Model name, e.g. `kimi-for-coding`.
    pub model: String,

    /// Maximum reasoning steps per turn.
    pub max_steps_per_turn: usize,

    /// Policy that decides whether a tool call may execute.
    pub approval_policy: Arc<dyn ApprovalPolicy>,

    /// External sources of tools (plugins, skills, MCP).
    pub tool_sources: Vec<Arc<dyn ToolSource>>,

    /// Persistent message history.
    pub message_store: Arc<std::sync::Mutex<dyn MessageStore>>,

    /// Optional context compaction policy.
    pub compaction_policy: Option<Arc<dyn CompactionPolicy>>,

    /// Optional dynamic context injection policy.
    pub injection_policy: Option<Arc<dyn InjectionPolicy>>,

    /// Optional hook policy.
    pub hook_policy: Arc<dyn HookPolicy>,
}

impl std::fmt::Debug for BrainConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrainConfig")
            .field("system_prompt", &self.system_prompt)
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .field("max_steps_per_turn", &self.max_steps_per_turn)
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
            approval_policy: Arc::new(AutoApprove),
            tool_sources: Vec::new(),
            message_store: Arc::new(std::sync::Mutex::new(InMemoryMessageStore::new())),
            compaction_policy: None,
            injection_policy: None,
            hook_policy: Arc::new(NoOpHookPolicy),
        }
    }
}
