use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use async_trait::async_trait;
use brain::{Brain, BrainConfig, ExtismPluginSource, TurnInput};
use kosong::tooling::ToolReturnValue;
use serde::Deserialize;
use tracing::info;

use crate::config::Config;
use crate::memory::MemoryStore;
use crate::oauth::OAuthManager;

/// Host-provided tool that fetches recent messages from the bot's memory.
pub struct RecentMessagesTool {
    memory: MemoryStore,
    group_id: i64,
}

impl RecentMessagesTool {
    pub fn new(memory: MemoryStore, group_id: i64) -> Self {
        Self { memory, group_id }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct RecentMessagesParams {
    /// How many recent messages to retrieve.
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

#[async_trait]
impl kosong::tooling::CallableTool2 for RecentMessagesTool {
    type Params = RecentMessagesParams;

    fn name(&self) -> &str {
        "qqbot::recent_messages"
    }

    fn description(&self) -> &str {
        "Retrieve the most recent messages in the current QQ group."
    }

    async fn call_typed(&self, params: RecentMessagesParams) -> ToolReturnValue {
        let recent = self.memory.recent(self.group_id, params.limit);
        if recent.is_empty() {
            return ToolReturnValue::ok("No recent messages.".to_string());
        }

        let lines: Vec<String> = recent
            .into_iter()
            .map(|(user_id, text)| format!("{}: {}", user_id, text))
            .collect();
        ToolReturnValue::ok(lines.join("\n"))
    }
}

/// One Brain per allowed group.
pub struct GroupBrainManager {
    /// group_id -> Brain
    brains: Mutex<HashMap<i64, Brain>>,
    config: Config,
    memory: MemoryStore,
    oauth: Option<OAuthManager>,
    plugin_dir: PathBuf,
}

impl GroupBrainManager {
    pub fn new(
        config: Config,
        memory: MemoryStore,
        oauth: Option<OAuthManager>,
        plugin_dir: PathBuf,
    ) -> Self {
        Self {
            brains: Mutex::new(HashMap::new()),
            config,
            memory,
            oauth,
            plugin_dir,
        }
    }

    /// Drop all group Brains so they are recreated with fresh plugins/config.
    pub fn clear(&self) {
        let mut brains = self.brains.lock().unwrap();
        brains.clear();
        info!("cleared all group brains");
    }

    /// Run a turn for a group. Creates the Brain lazily on first use.
    pub async fn run_turn(&self, group_id: i64, user_message: String) -> Result<brain::TurnResult> {
        let mut brains = self.brains.lock().unwrap();
        if !brains.contains_key(&group_id) {
            let brain = self
                .create_brain(group_id)
                .await
                .context("failed to create Brain for group")?;
            brains.insert(group_id, brain);
            info!(group_id, "created group brain");
        }

        let brain = brains.get_mut(&group_id).unwrap();
        brain
            .run_turn_to_completion(TurnInput::from(user_message))
            .await
    }

    async fn create_brain(&self, group_id: i64) -> Result<Brain> {
        let api_key = match self.oauth {
            Some(ref manager) => manager.access_token().await.unwrap_or_default(),
            None => self.config.llm.api_key.clone(),
        };

        let tool_sources: Vec<std::sync::Arc<dyn brain::ToolSource>> = vec![std::sync::Arc::new(
            ExtismPluginSource::new(&self.plugin_dir),
        )];

        let config = BrainConfig {
            system_prompt: self.config.llm.system_prompt.clone(),
            base_url: self.config.llm.api_url.clone(),
            api_key,
            model: self.config.llm.model.clone(),
            max_steps_per_turn: 16,
            tool_sources,
            ..Default::default()
        };

        let mut brain = Brain::new(config)?;
        brain.register_tool(Box::new(kosong::tooling::CallableTool2Adapter::new(
            RecentMessagesTool::new(self.memory.clone(), group_id),
        )));

        // Append dynamic tool instructions based on what is actually loaded.
        let mut instructions = vec![
            "When asked to summarize the conversation, first call qqbot::recent_messages to retrieve the recent messages, then provide a concise summary.".to_string(),
        ];
        if brain
            .registry()
            .find("summary::format_conversation")
            .is_some()
        {
            instructions.push("You may also use summary::format_conversation to format the raw conversation before summarizing.".to_string());
        }
        let system_prompt = format!(
            "{}\n\n{}",
            self.config.llm.system_prompt,
            instructions.join("\n")
        );
        brain.set_system_prompt(system_prompt);

        Ok(brain)
    }
}
