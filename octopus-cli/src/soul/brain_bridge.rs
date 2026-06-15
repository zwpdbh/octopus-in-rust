use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::hooks::{HookEngine, HookEvent};
use crate::llm::{LLM, kosong_to_wire_message, wire_to_kosong_message};
use crate::soul::compaction::{SimpleCompaction, should_auto_compact};
use crate::soul::context::Context;
use crate::soul::dynamic_injection::{DynamicInjectionProvider, InjectionContext};
use crate::wire;

/// Wraps the file-backed [`Context`] as a Brain [`MessageStore`].
pub struct ContextMessageStore {
    context: Arc<tokio::sync::Mutex<Context>>,
}

impl ContextMessageStore {
    pub fn new(context: Arc<tokio::sync::Mutex<Context>>) -> Self {
        Self { context }
    }
}

#[async_trait::async_trait]
impl brain::session::store::MessageStore for ContextMessageStore {
    async fn push(&mut self, message: kosong::Message) {
        let wire_message = kosong_to_wire_message(message);
        let mut ctx = self.context.lock().await;
        if let Err(e) = ctx.append_message(wire_message).await {
            tracing::error!("Failed to persist message: {}", e);
        }
    }

    async fn history(&self) -> Vec<kosong::Message> {
        let ctx = self.context.lock().await;
        let messages: Vec<kosong::Message> =
            ctx.history().iter().map(wire_to_kosong_message).collect();
        // Merge adjacent user messages to match the CLI's normalize_history behavior.
        let mut normalized: Vec<kosong::Message> = Vec::new();
        for msg in messages {
            if let Some(last) = normalized.last_mut() {
                if last.role == kosong::Role::User && msg.role == kosong::Role::User {
                    last.content.extend(msg.content);
                    continue;
                }
            }
            normalized.push(msg);
        }
        normalized
    }

    async fn set_history(&mut self, history: Vec<kosong::Message>) {
        let wire_history: Vec<wire::Message> =
            history.into_iter().map(kosong_to_wire_message).collect();
        let mut ctx = self.context.lock().await;
        let _ = ctx.replace_history(wire_history).await;
    }

    async fn clear(&mut self) {
        let mut ctx = self.context.lock().await;
        if let Err(e) = ctx.clear().await {
            tracing::error!("Failed to clear context: {}", e);
        }
    }
}

/// Bridges the CLI's [`SimpleCompaction`] into a Brain [`CompactionPolicy`].
pub struct CliCompactionPolicy {
    compaction: SimpleCompaction,
    llm: Arc<crate::llm::LLM>,
    max_context_size: usize,
    trigger_ratio: f64,
    reserved_context_size: usize,
    custom_instruction: String,
}

impl CliCompactionPolicy {
    pub fn new(
        compaction: SimpleCompaction,
        llm: Arc<crate::llm::LLM>,
        max_context_size: usize,
        trigger_ratio: f64,
        reserved_context_size: usize,
        custom_instruction: String,
    ) -> Self {
        Self {
            compaction,
            llm,
            max_context_size,
            trigger_ratio,
            reserved_context_size,
            custom_instruction,
        }
    }
}

#[async_trait::async_trait]
impl brain::session::compaction::CompactionPolicy for CliCompactionPolicy {
    async fn maybe_compact(&self, history: &[kosong::Message]) -> Option<Vec<kosong::Message>> {
        let wire_history: Vec<wire::Message> = history
            .iter()
            .map(|m| kosong_to_wire_message(m.clone()))
            .collect();
        let token_count = crate::soul::context::estimate_text_tokens(&wire_history);

        if !should_auto_compact(
            token_count,
            self.max_context_size,
            self.trigger_ratio,
            self.reserved_context_size,
        ) {
            return None;
        }

        match self
            .compaction
            .compact(&wire_history, &self.llm, &self.custom_instruction)
            .await
        {
            Ok(result) => Some(
                result
                    .messages
                    .into_iter()
                    .map(|m| wire_to_kosong_message(&m))
                    .collect(),
            ),
            Err(e) => {
                tracing::error!("Context compaction failed: {}", e);
                None
            }
        }
    }
}

/// Mutable runtime state for [`CliInjectionPolicy`].
#[derive(Debug, Clone)]
pub struct InjectionState {
    pub plan_mode: bool,
    pub effective_afk: bool,
    pub persisted_afk: bool,
    pub plan_file_path: Option<PathBuf>,
    pub pending_plan_activation: bool,
}

impl From<InjectionContext<'_>> for InjectionState {
    fn from(ctx: InjectionContext<'_>) -> Self {
        Self {
            plan_mode: ctx.plan_mode,
            effective_afk: ctx.effective_afk,
            persisted_afk: ctx.persisted_afk,
            plan_file_path: ctx.plan_file_path.map(|p| p.to_path_buf()),
            pending_plan_activation: ctx.pending_plan_activation,
        }
    }
}

/// Bridges the CLI's dynamic injection providers into a Brain [`InjectionPolicy`].
pub struct CliInjectionPolicy {
    providers: Arc<tokio::sync::Mutex<Vec<Box<dyn DynamicInjectionProvider>>>>,
    state: Arc<std::sync::RwLock<InjectionState>>,
}

impl CliInjectionPolicy {
    pub fn new(
        providers: Vec<Box<dyn DynamicInjectionProvider>>,
        injection_context: InjectionContext<'_>,
    ) -> Self {
        Self {
            providers: Arc::new(tokio::sync::Mutex::new(providers)),
            state: Arc::new(std::sync::RwLock::new(InjectionState::from(
                injection_context,
            ))),
        }
    }

    pub fn set_state(&self, state: InjectionState) {
        if let Ok(mut s) = self.state.write() {
            *s = state;
        }
    }
}

#[async_trait::async_trait]
impl brain::session::injection::InjectionPolicy for CliInjectionPolicy {
    async fn inject(&self, history: &[kosong::Message]) -> Vec<kosong::Message> {
        let wire_history: Vec<wire::Message> = history
            .iter()
            .map(|m| kosong_to_wire_message(m.clone()))
            .collect();

        let (plan_mode, effective_afk, persisted_afk, plan_file_path, pending_plan_activation) = {
            let s = self.state.read().unwrap();
            (
                s.plan_mode,
                s.effective_afk,
                s.persisted_afk,
                s.plan_file_path.clone(),
                s.pending_plan_activation,
            )
        };

        let ctx = InjectionContext {
            plan_mode,
            effective_afk,
            persisted_afk,
            plan_file_path: plan_file_path.as_deref(),
            pending_plan_activation,
        };

        let mut providers = self.providers.lock().await;
        let mut injections: Vec<wire::Message> = Vec::new();
        for provider in providers.iter_mut() {
            for injection in provider.get_injections(&wire_history, &ctx).await {
                injections.push(wire::Message {
                    role: "user".to_string(),
                    content: vec![wire::ContentPart::Text {
                        text: format!(
                            "<system-reminder>\n{}\n</system-reminder>",
                            injection.content
                        ),
                    }],
                    tool_call_id: None,
                    tool_calls: None,
                });
            }
        }

        injections
            .into_iter()
            .map(|m| wire_to_kosong_message(&m))
            .collect()
    }
}

/// Bridges the CLI's [`HookEngine`] into a Brain [`HookPolicy`].
pub struct CliHookPolicy {
    engine: HookEngine,
    session_id: String,
    cwd: PathBuf,
}

impl CliHookPolicy {
    pub fn new(engine: HookEngine, session_id: String, cwd: PathBuf) -> Self {
        Self {
            engine,
            session_id,
            cwd,
        }
    }
}

#[async_trait::async_trait]
impl brain::hooks::policy::HookPolicy for CliHookPolicy {
    async fn on_user_prompt_submit(&self, prompt: &str) -> brain::hooks::policy::HookAction {
        let event =
            HookEvent::user_prompt_submit(&self.session_id, self.cwd.to_string_lossy(), prompt);
        let results = self.engine.trigger(event).await;
        aggregate_hook_action(results)
    }

    async fn on_pre_tool_use(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        tool_call_id: &str,
    ) -> brain::hooks::policy::HookAction {
        let input_map = match tool_input.as_object() {
            Some(obj) => obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            None => std::collections::HashMap::new(),
        };
        let event = HookEvent::pre_tool_use(
            &self.session_id,
            self.cwd.to_string_lossy(),
            tool_name,
            &input_map,
            tool_call_id,
        );
        let results = self.engine.trigger(event).await;
        aggregate_hook_action(results)
    }

    async fn on_post_tool_use(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        tool_output: &str,
        tool_call_id: &str,
    ) {
        let input_map = match tool_input.as_object() {
            Some(obj) => obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            None => std::collections::HashMap::new(),
        };
        let event = HookEvent::post_tool_use(
            &self.session_id,
            self.cwd.to_string_lossy(),
            tool_name,
            &input_map,
            tool_output,
            tool_call_id,
        );
        let _ = self.engine.trigger(event).await;
    }

    async fn on_post_tool_use_failure(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        error: &str,
        tool_call_id: &str,
    ) {
        let input_map = match tool_input.as_object() {
            Some(obj) => obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            None => std::collections::HashMap::new(),
        };
        let event = HookEvent::post_tool_use_failure(
            &self.session_id,
            self.cwd.to_string_lossy(),
            tool_name,
            &input_map,
            error,
            tool_call_id,
        );
        let _ = self.engine.fire_and_forget_trigger(event).await;
    }
}

fn aggregate_hook_action(
    results: Vec<crate::hooks::runner::HookResult>,
) -> brain::hooks::policy::HookAction {
    for result in results {
        if let crate::hooks::runner::HookAction::Block(reason) = result.action {
            return brain::hooks::policy::HookAction::Block { reason };
        }
    }
    brain::hooks::policy::HookAction::Allow
}

/// Builds kosong providers from the CLI's [`LLM`] configuration.
///
/// On construction, it ensures any OAuth token is fresh so that recovered
/// providers pick up refreshed credentials after a 401.
pub struct CliProviderFactory {
    llm: Arc<LLM>,
    oauth: crate::auth::OAuthManager,
}

impl CliProviderFactory {
    pub fn new(llm: Arc<LLM>, oauth: crate::auth::OAuthManager) -> Self {
        Self { llm, oauth }
    }
}

#[async_trait]
impl brain::ProviderFactory for CliProviderFactory {
    async fn create(
        &self,
        _config: &brain::BrainConfig,
    ) -> Result<Arc<dyn kosong::ChatProvider>, brain::BrainError> {
        // Ensure the OAuth token is fresh before building the provider.
        let _ = self.oauth.ensure_fresh(&self.llm, true).await;

        self.llm
            .build_kosong_provider()
            .map_err(|e| brain::BrainError::Llm(e.to_string()))
    }
}

/// CLI retry policy with exponential backoff and jitter.
pub struct CliRetryPolicy {
    max_attempts: usize,
}

impl CliRetryPolicy {
    pub fn new(max_attempts: usize) -> Self {
        Self { max_attempts }
    }
}

#[async_trait]
impl brain::RetryPolicy for CliRetryPolicy {
    fn max_attempts(&self) -> usize {
        self.max_attempts
    }

    fn should_retry(&self, error: &brain::BrainError, attempt: usize) -> Option<Duration> {
        if attempt > self.max_attempts {
            return None;
        }
        if !error.is_transient() && !error.is_auth_failure() {
            return None;
        }
        let base = 0.3_f64 * 2_f64.powi((attempt - 1) as i32);
        let capped = base.min(5.0);
        let jitter = rand::random::<f64>() * 0.5;
        Some(Duration::from_secs_f64(capped + jitter))
    }
}

/// CLI recovery policy: refresh the provider on auth failures and retry
/// transient failures once more.
pub struct CliRecoveryPolicy {
    oauth: crate::auth::OAuthManager,
    llm: Arc<LLM>,
}

impl CliRecoveryPolicy {
    pub fn new(oauth: crate::auth::OAuthManager, llm: Arc<LLM>) -> Self {
        Self { oauth, llm }
    }
}

#[async_trait]
impl brain::RecoveryPolicy for CliRecoveryPolicy {
    async fn recover(&self, error: &brain::BrainError) -> brain::RecoveryAction {
        if error.is_auth_failure() {
            // Try to refresh the token before the factory rebuilds the provider.
            let _ = self.oauth.ensure_fresh(&self.llm, true).await;
            return brain::RecoveryAction::RefreshProvider;
        }

        if error.is_transient() {
            return brain::RecoveryAction::Retry {
                wait: Duration::from_secs(1),
            };
        }

        brain::RecoveryAction::Abort
    }
}
