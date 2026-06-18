use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use brain::{
    control::{GroupRuntimeStatus, ToolRuntimeInfo},
    Brain, BrainConfig, BrainEvent, ExtismPluginSource,
};
use futures_util::StreamExt;
use kosong::message::{ContentPart, Message, Role};
use kosong::tooling::ToolReturnValue;
use kosong::Toolset;
use serde::Deserialize;
use tokio::sync::{mpsc, watch, Mutex};
use tracing::{error, info};

use crate::config::Config;
use crate::llm_provider::QqbotProviderFactory;
use crate::memory::MemoryStore;
use crate::onebot::types::Action;

const DEFAULT_MAX_STEPS_PER_TURN: usize = 16;

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
        "qqbot_recent_messages"
    }

    fn description(&self) -> &str {
        "Retrieve the most recent messages in the current QQ group."
    }

    fn prompt_fragment(&self) -> Option<&str> {
        Some("When asked to summarize the conversation, first call qqbot_recent_messages to retrieve the recent messages, then provide a concise summary.")
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

/// Channels used to interact with a running turn worker.
struct RunningGroup {
    steer_tx: mpsc::UnboundedSender<String>,
    cancel_tx: watch::Sender<bool>,
    handle: tokio::task::JoinHandle<()>,
}

/// One Brain per allowed group, with per-group turn workers.
pub struct GroupBrainManager {
    /// group_id -> Brain
    brains: Mutex<HashMap<i64, Brain>>,
    /// group_id -> active turn worker
    running: Mutex<HashMap<i64, RunningGroup>>,
    config: Config,
    memory: MemoryStore,
    plugin_dir: PathBuf,
    data_dir: PathBuf,
    action_tx: crate::onebot::ActionTx,
    max_steps_per_turn: usize,
}

impl GroupBrainManager {
    pub fn new(
        config: Config,
        memory: MemoryStore,
        plugin_dir: PathBuf,
        data_dir: PathBuf,
        action_tx: crate::onebot::ActionTx,
    ) -> Self {
        Self {
            brains: Mutex::new(HashMap::new()),
            running: Mutex::new(HashMap::new()),
            config,
            memory,
            plugin_dir,
            data_dir,
            action_tx,
            max_steps_per_turn: DEFAULT_MAX_STEPS_PER_TURN,
        }
    }

    /// Return the file stems of all `.wasm` plugins in the plugin directory.
    fn installed_plugin_names(plugin_dir: &PathBuf) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(plugin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
        names
    }

    /// Drop all group Brains so they are recreated with fresh plugins/config.
    pub async fn clear(&self) {
        // Cancel any active workers first so they do not continue using old state.
        let running_ids: Vec<i64> = {
            let running = self.running.lock().await;
            running.keys().copied().collect()
        };
        for group_id in running_ids {
            self.cancel_turn(group_id).await;
        }

        let mut brains = self.brains.lock().await;
        brains.clear();
        info!("cleared all group brains");
    }

    /// Eagerly create a Brain for every allowed group.
    ///
    /// This is called at startup and after each SIGHUP so that tools are loaded
    /// immediately rather than waiting for the first addressed message.
    pub async fn initialize(&self) {
        for group_id in &self.config.bot.allowed_groups {
            if let Err(e) = self.get_or_create_brain(*group_id).await {
                error!(group_id, error = %e, "failed to eagerly create Brain for group");
            }
        }
    }

    /// Return runtime status for every configured allowed group.
    pub async fn group_status(&self) -> Vec<GroupRuntimeStatus> {
        let brains = self.brains.lock().await;
        self.config
            .bot
            .allowed_groups
            .iter()
            .map(|group_id| {
                let mut tools: Vec<ToolRuntimeInfo> = brains
                    .get(group_id)
                    .map(|brain| {
                        brain
                            .registry()
                            .tool_sources()
                            .into_iter()
                            .map(|(name, source)| ToolRuntimeInfo { name, source })
                            .collect()
                    })
                    .unwrap_or_default();
                tools.sort_by(|a, b| a.name.cmp(&b.name));
                let tool_count = tools.len();
                GroupRuntimeStatus {
                    group_id: *group_id,
                    brain_ready: brains.contains_key(group_id),
                    tool_count,
                    tools,
                }
            })
            .collect()
    }

    /// Return all tools currently loaded in any group Brain.
    ///
    /// This reflects the runtime toolset, not the plugin directory. Brains are
    /// created eagerly at startup and after each reload, so this should reflect
    /// the currently installed plugins once initialization has finished.
    pub async fn loaded_tools(&self) -> Vec<ToolRuntimeInfo> {
        let brains = self.brains.lock().await;
        let mut seen = std::collections::BTreeSet::new();
        let mut tools = Vec::new();
        for brain in brains.values() {
            for (name, source) in brain.registry().tool_sources() {
                if seen.insert(name.clone()) {
                    tools.push(ToolRuntimeInfo { name, source });
                }
            }
        }
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    /// Cancel an in-progress turn for a group, if any.
    ///
    /// Returns `true` if a running worker was found and asked to stop.
    /// The worker itself will send the cancellation confirmation message.
    pub async fn cancel_turn(&self, group_id: i64) -> bool {
        let mut running = self.running.lock().await;
        if let Some(group) = running.get(&group_id) {
            if group.handle.is_finished() {
                running.remove(&group_id);
                return false;
            }
            let _ = group.cancel_tx.send(true);
            return true;
        }
        false
    }

    /// Handle a user prompt addressed to the bot.
    ///
    /// If a turn is already running for this group, the prompt is injected as
    /// "steer" context between reasoning steps (matching `kimi-cli`). Otherwise
    /// a new turn worker is started.
    pub async fn handle_prompt(
        &self,
        group_id: i64,
        user_id: i64,
        message_id: Option<i32>,
        text: String,
    ) {
        let mut running = self.running.lock().await;
        if let Some(group) = running.get_mut(&group_id) {
            if group.handle.is_finished() {
                running.remove(&group_id);
            } else if let Err(e) = group.steer_tx.send(text) {
                error!(group_id, error = %e, "failed to steer running turn");
                return;
            } else {
                let _ = self.action_tx.send(Action::send_group_msg(
                    group_id,
                    "Got it — adding that context... 🤔".to_string(),
                    None,
                ));
                return;
            }
        }
        drop(running);

        let brain = match self.get_or_create_brain(group_id).await {
            Ok(brain) => brain,
            Err(e) => {
                error!(group_id, error = %e, "failed to create Brain for group");
                let _ = self.action_tx.send(Action::reply_group_msg(
                    group_id,
                    user_id,
                    "Sorry, I couldn't start my brain right now.",
                    None,
                ));
                return;
            }
        };

        let profile = qqbot_config::GroupProfile::load(&self.data_dir, group_id)
            .ok()
            .flatten()
            .unwrap_or_default();
        let progress_interval = Duration::from_secs(profile.progress_interval_secs());

        let (steer_tx, steer_rx) = mpsc::unbounded_channel();
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let action_tx = self.action_tx.clone();
        let max_steps = self.max_steps_per_turn;
        let handle = tokio::spawn(async move {
            turn_worker(
                brain,
                group_id,
                user_id,
                message_id,
                text,
                action_tx,
                steer_rx,
                cancel_rx,
                max_steps,
                progress_interval,
            )
            .await;
        });

        {
            let mut running = self.running.lock().await;
            running.insert(
                group_id,
                RunningGroup {
                    steer_tx,
                    cancel_tx,
                    handle,
                },
            );
        }
    }

    async fn get_or_create_brain(&self, group_id: i64) -> Result<Brain> {
        let mut brains = self.brains.lock().await;
        if let std::collections::hash_map::Entry::Vacant(e) = brains.entry(group_id) {
            let brain = self
                .create_brain(group_id)
                .await
                .context("failed to create Brain for group")?;
            e.insert(brain);
            info!(group_id, "created group brain");
        }
        brains
            .get(&group_id)
            .cloned()
            .context("brain disappeared after creation")
    }

    async fn create_brain(&self, group_id: i64) -> Result<Brain> {
        let profile = match qqbot_config::GroupProfile::load(&self.data_dir, group_id) {
            Ok(Some(p)) => p,
            Ok(None) => qqbot_config::GroupProfile::default(),
            Err(e) => {
                tracing::warn!(group_id, error = %e, "failed to load group profile; using defaults");
                qqbot_config::GroupProfile::default()
            }
        };

        // Determine which plugin file stems are allowed for this group.
        let tool_source: std::sync::Arc<dyn brain::ToolSource> =
            if profile.enabled_plugins.is_some() || !profile.disabled_plugins.is_empty() {
                let installed = Self::installed_plugin_names(&self.plugin_dir);
                let allowed: HashSet<String> = profile
                    .filter_plugins(installed.iter().map(|s| s.as_str()))
                    .into_iter()
                    .collect();
                std::sync::Arc::new(ExtismPluginSource::with_filter(&self.plugin_dir, allowed))
            } else {
                std::sync::Arc::new(ExtismPluginSource::new(&self.plugin_dir))
            };

        let tool_sources: Vec<std::sync::Arc<dyn brain::ToolSource>> = vec![tool_source];

        let config = BrainConfig {
            system_prompt: profile
                .system_prompt
                .clone()
                .unwrap_or_else(|| self.config.llm.system_prompt.clone()),
            base_url: self.config.llm.api_url().to_string(),
            api_key: String::new(),
            model: self.config.llm.model.clone(),
            max_steps_per_turn: self.max_steps_per_turn,
            tool_sources,
            ..Default::default()
        };

        let provider_factory =
            std::sync::Arc::new(QqbotProviderFactory::new(self.config.llm.provider.clone()));

        let mut brain = brain::BrainBuilder::default()
            .from_config(config)
            .with_provider_factory(provider_factory)
            .with_system_prompt_policy(std::sync::Arc::new(brain::ToolAwareSystemPromptPolicy))
            .build()
            .await?;
        brain.register_tool(
            Box::new(kosong::tooling::CallableTool2Adapter::new(
                RecentMessagesTool::new(self.memory.clone(), group_id),
            )),
            "host",
        );

        let tool_names: Vec<String> = brain
            .registry()
            .tools()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        info!(group_id, tools = ?tool_names, "registered tools for group brain");

        Ok(brain)
    }
}

/// Push a user message into the Brain's message store.
async fn push_user_message(brain: &Brain, text: &str) {
    let msg = Message {
        role: Role::User,
        name: None,
        content: vec![ContentPart::Text {
            text: text.to_string(),
        }],
        tool_calls: None,
        tool_call_id: None,
        partial: None,
    };
    brain.message_store().lock().await.push(msg).await;
}

/// Send a plain group message (used for streaming chunks and tool updates).
fn send_plain(action_tx: &crate::onebot::ActionTx, group_id: i64, text: String) {
    let _ = action_tx.send(Action::send_group_msg(group_id, text, None));
}

/// Send a quoted reply to the original triggering message, falling back to a
/// plain group message if no message id is available.
fn send_reply(
    action_tx: &crate::onebot::ActionTx,
    group_id: i64,
    message_id: Option<i32>,
    text: String,
) {
    if let Some(id) = message_id {
        let _ = action_tx.send(Action::quote_group_msg(group_id, id, text, None));
    } else {
        send_plain(action_tx, group_id, text);
    }
}

/// Format the periodic progress message. Lists invoked tool names in brackets
/// so users can clearly see whether tools are being used.
fn format_progress_message(tools: &[String]) -> String {
    if tools.is_empty() {
        "Still checking... (tools: [])".to_string()
    } else {
        format!("Still checking... (tools: [{}])", tools.join(", "))
    }
}

/// Worker that runs one turn step-by-step, consuming steers between steps.
#[allow(clippy::too_many_arguments)]
async fn turn_worker(
    mut brain: Brain,
    group_id: i64,
    _user_id: i64,
    message_id: Option<i32>,
    initial_message: String,
    action_tx: crate::onebot::ActionTx,
    mut steer_rx: mpsc::UnboundedReceiver<String>,
    cancel_rx: watch::Receiver<bool>,
    max_steps: usize,
    progress_interval: Duration,
) {
    // Seed the initial user message.
    push_user_message(&brain, &initial_message).await;

    send_reply(
        &action_tx,
        group_id,
        message_id,
        format!("Thinking about: \"{}\" 🤔", initial_message),
    );

    let mut cancelled = false;
    let mut any_text_seen = false;
    let mut final_text = String::new();

    for _step in 0..max_steps {
        if *cancel_rx.borrow() {
            cancelled = true;
            break;
        }

        // Track tool names invoked this step for the periodic progress message.
        let progress_tools: std::sync::Arc<std::sync::Mutex<HashSet<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(HashSet::new()));

        // Periodically post a progress message while the step is running.
        // A message is sent when the tool set changes or when the interval
        // elapses, whichever comes first. Duplicate messages for the same tool
        // set are suppressed.
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let progress_done = done.clone();
        let progress_action_tx = action_tx.clone();
        let progress_tools_clone = progress_tools.clone();
        let progress_handle = tokio::spawn(async move {
            let mut last_sent_tools: Vec<String> = Vec::new();
            let mut last_sent_at = tokio::time::Instant::now();
            let poll_interval = Duration::from_secs(1);

            loop {
                tokio::time::sleep(poll_interval).await;
                if progress_done.load(Ordering::Relaxed) {
                    break;
                }

                let tools: Vec<String> = {
                    let set = progress_tools_clone.lock().unwrap();
                    let mut v: Vec<_> = set.iter().cloned().collect();
                    v.sort();
                    v
                };

                let tools_changed = tools != last_sent_tools;
                let interval_elapsed =
                    tokio::time::Instant::now().duration_since(last_sent_at) >= progress_interval;

                if tools_changed || interval_elapsed {
                    let msg = format_progress_message(&tools);
                    send_reply(&progress_action_tx, group_id, message_id, msg);
                    last_sent_tools = tools;
                    last_sent_at = tokio::time::Instant::now();
                }
            }
        });

        final_text.clear();
        let mut had_tool_results = false;
        let mut step_error: Option<String> = None;
        let mut tool_names: HashMap<String, String> = HashMap::new();

        match brain.run_step().await {
            Ok(mut stream) => {
                while let Some(event) = stream.next().await {
                    match event {
                        BrainEvent::TextPart(text) => {
                            any_text_seen = true;
                            final_text.push_str(&text);
                        }
                        BrainEvent::ToolCall {
                            id,
                            name,
                            arguments: _,
                        } => {
                            tool_names.insert(id, name.clone());
                            progress_tools.lock().unwrap().insert(name);
                        }
                        BrainEvent::ToolResult {
                            id,
                            output: _,
                            is_error: _,
                        } => {
                            had_tool_results = true;
                            let _name = tool_names
                                .get(&id)
                                .cloned()
                                .unwrap_or_else(|| "tool".to_string());
                            // Tool results are no longer posted to the group chat.
                        }
                        BrainEvent::Error(e) => {
                            step_error = Some(e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                done.store(true, Ordering::Relaxed);
                progress_handle.abort();
                send_reply(
                    &action_tx,
                    group_id,
                    message_id,
                    format!("Sorry, I couldn't process that right now: {}", e),
                );
                return;
            }
        }

        done.store(true, Ordering::Relaxed);
        progress_handle.abort();

        if let Some(err) = step_error {
            send_reply(
                &action_tx,
                group_id,
                message_id,
                format!("Sorry, I couldn't process that right now: {}", err),
            );
            return;
        }

        if *cancel_rx.borrow() {
            cancelled = true;
            break;
        }

        // Drain any steers that arrived during the step and append them as user messages.
        let mut steers = Vec::new();
        while let Ok(msg) = steer_rx.try_recv() {
            steers.push(msg);
        }
        for msg in &steers {
            push_user_message(&brain, msg).await;
        }

        if had_tool_results {
            // Assistant requested tools; continue to the next reasoning step.
            continue;
        }

        // No tool calls: this step produced a final answer. If text was already
        // streamed, there is nothing left to send. Otherwise send a fallback.
        let reply = if any_text_seen {
            final_text
        } else {
            "I thought about it but couldn't come up with a good answer. Try rephrasing?"
                .to_string()
        };
        send_reply(&action_tx, group_id, message_id, reply);
        break;
    }

    if cancelled {
        send_reply(
            &action_tx,
            group_id,
            message_id,
            "Cancelled the current reasoning.".to_string(),
        );
    }
}
