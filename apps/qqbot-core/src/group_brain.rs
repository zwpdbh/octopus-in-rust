use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use brain::{Brain, BrainConfig, BrainEvent, ExtismPluginSource};
use futures_util::StreamExt;
use kosong::message::{ContentPart, Message, Role};
use kosong::tooling::ToolReturnValue;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time::{interval, MissedTickBehavior};
use tracing::{error, info};

use crate::config::Config;
use crate::llm_provider::QqbotProviderFactory;
use crate::memory::MemoryStore;
use crate::onebot::types::Action;

const DEFAULT_MAX_STEPS_PER_TURN: usize = 16;
const PROGRESS_UPDATE_AFTER: Duration = Duration::from_secs(10);
const STREAM_FLUSH_INTERVAL: Duration = Duration::from_millis(1200);
const STREAM_FLUSH_MAX_LEN: usize = 240;
const TOOL_ARGS_MAX_LEN: usize = 120;
const TOOL_RESULT_MAX_LEN: usize = 240;

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
    action_tx: crate::onebot::ActionTx,
    max_steps_per_turn: usize,
}

impl GroupBrainManager {
    pub fn new(
        config: Config,
        memory: MemoryStore,
        plugin_dir: PathBuf,
        action_tx: crate::onebot::ActionTx,
    ) -> Self {
        Self {
            brains: Mutex::new(HashMap::new()),
            running: Mutex::new(HashMap::new()),
            config,
            memory,
            plugin_dir,
            action_tx,
            max_steps_per_turn: DEFAULT_MAX_STEPS_PER_TURN,
        }
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
    pub async fn handle_prompt(&self, group_id: i64, user_id: i64, text: String) {
        let mut running = self.running.lock().await;
        if let Some(group) = running.get_mut(&group_id) {
            if group.handle.is_finished() {
                running.remove(&group_id);
            } else if let Err(e) = group.steer_tx.send(text) {
                error!(group_id, error = %e, "failed to steer running turn");
                return;
            } else {
                let _ = self.action_tx.send(Action::reply_group_msg(
                    group_id,
                    user_id,
                    "Got it — adding that context... 🤔",
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

        let (steer_tx, steer_rx) = mpsc::unbounded_channel();
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let action_tx = self.action_tx.clone();
        let max_steps = self.max_steps_per_turn;
        let handle = tokio::spawn(async move {
            turn_worker(
                brain,
                group_id,
                user_id,
                text,
                action_tx,
                steer_rx,
                cancel_rx,
                max_steps,
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
        let tool_sources: Vec<std::sync::Arc<dyn brain::ToolSource>> = vec![std::sync::Arc::new(
            ExtismPluginSource::new(&self.plugin_dir),
        )];

        let config = BrainConfig {
            system_prompt: self.config.llm.system_prompt.clone(),
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
            .build()
            .await?;
        brain.register_tool(Box::new(kosong::tooling::CallableTool2Adapter::new(
            RecentMessagesTool::new(self.memory.clone(), group_id),
        )));

        // Append dynamic tool instructions based on what is actually loaded.
        let mut instructions = vec![
            "When asked to summarize the conversation, first call qqbot::recent_messages to retrieve the recent messages, then provide a concise summary.".to_string(),
        ];
        if brain
            .registry()
            .find("summary_format_conversation")
            .is_some()
        {
            instructions.push("You may also use summary_format_conversation to format the raw conversation before summarizing.".to_string());
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

/// Send a reply addressed to the user.
fn send_addressed(action_tx: &crate::onebot::ActionTx, group_id: i64, user_id: i64, text: String) {
    let _ = action_tx.send(Action::reply_group_msg(group_id, user_id, text, None));
}

/// Send a plain group message (used for streaming chunks and tool updates).
fn send_plain(action_tx: &crate::onebot::ActionTx, group_id: i64, text: String) {
    let _ = action_tx.send(Action::send_group_msg(group_id, text, None));
}

/// Send an error reply addressed to the user.
fn send_error(
    action_tx: &crate::onebot::ActionTx,
    group_id: i64,
    user_id: i64,
    error: String,
) {
    send_addressed(
        action_tx,
        group_id,
        user_id,
        format!("Sorry, I couldn't process that right now: {}", error),
    );
}

/// Truncate text for display, adding an ellipsis if needed.
fn truncate(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", &text[..text.char_indices().nth(max_len).map(|(i, _)| i).unwrap_or(max_len)])
    }
}

/// Format tool arguments for a status message.
fn format_tool_args(arguments: &Value) -> String {
    if arguments.is_null() || arguments.as_object().map(|o| o.is_empty()).unwrap_or(false) {
        return String::new();
    }
    let args = serde_json::to_string(arguments).unwrap_or_default();
    if args.len() <= TOOL_ARGS_MAX_LEN {
        format!(" with {}", args)
    } else {
        format!(" with {}", truncate(&args, TOOL_ARGS_MAX_LEN))
    }
}

/// Worker that runs one turn step-by-step, consuming steers between steps.
#[allow(clippy::too_many_arguments)]
async fn turn_worker(
    mut brain: Brain,
    group_id: i64,
    user_id: i64,
    initial_message: String,
    action_tx: crate::onebot::ActionTx,
    mut steer_rx: mpsc::UnboundedReceiver<String>,
    cancel_rx: watch::Receiver<bool>,
    max_steps: usize,
) {
    // Seed the initial user message.
    push_user_message(&brain, &initial_message).await;

    send_addressed(
        &action_tx,
        group_id,
        user_id,
        "Got it — thinking... 🤔".to_string(),
    );

    let mut cancelled = false;
    let mut any_text_seen = false;

    for _step in 0..max_steps {
        if *cancel_rx.borrow() {
            cancelled = true;
            break;
        }

        // If the step takes a while with no visible output, remind the user.
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let had_output = std::sync::Arc::new(AtomicBool::new(false));
        let progress_done = done.clone();
        let progress_had_output = had_output.clone();
        let progress_action_tx = action_tx.clone();
        let progress_handle = tokio::spawn(async move {
            tokio::time::sleep(PROGRESS_UPDATE_AFTER).await;
            if !progress_done.load(Ordering::Relaxed)
                && !progress_had_output.load(Ordering::Relaxed)
            {
                let _ = progress_action_tx.send(Action::reply_group_msg(
                    group_id,
                    user_id,
                    "Still working on it... ⏳",
                    None,
                ));
            }
        });

        let mut text_buffer = String::new();
        let mut had_tool_results = false;
        let mut step_error: Option<String> = None;
        let mut tool_names: HashMap<String, String> = HashMap::new();

        match brain.run_step().await {
            Ok(mut stream) => {
                let mut flush_interval = interval(STREAM_FLUSH_INTERVAL);
                flush_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                // Skip the immediate first tick.
                let _ = flush_interval.tick().await;

                loop {
                    tokio::select! {
                        maybe_event = stream.next() => {
                            match maybe_event {
                                Some(BrainEvent::TextPart(text)) => {
                                    any_text_seen = true;
                                    had_output.store(true, Ordering::Relaxed);
                                    text_buffer.push_str(&text);
                                    if text_buffer.len() >= STREAM_FLUSH_MAX_LEN {
                                        flush_text_buffer(&action_tx, group_id, &mut text_buffer);
                                    }
                                }
                                Some(BrainEvent::ToolCall { id, name, arguments }) => {
                                    flush_text_buffer(&action_tx, group_id, &mut text_buffer);
                                    had_output.store(true, Ordering::Relaxed);
                                    tool_names.insert(id, name.clone());
                                    let args = format_tool_args(&arguments);
                                    send_plain(
                                        &action_tx,
                                        group_id,
                                        format!("🔧 Using tool `{name}`{args}"),
                                    );
                                }
                                Some(BrainEvent::ToolResult { id, output, is_error }) => {
                                    had_output.store(true, Ordering::Relaxed);
                                    had_tool_results = true;
                                    let name = tool_names
                                        .get(&id)
                                        .cloned()
                                        .unwrap_or_else(|| "tool".to_string());
                                    let status = if is_error { "❌" } else { "📥" };
                                    let summary = truncate(&output, TOOL_RESULT_MAX_LEN);
                                    send_plain(
                                        &action_tx,
                                        group_id,
                                        format!("{status} `{name}` result: {summary}"),
                                    );
                                }
                                Some(BrainEvent::Error(e)) => {
                                    step_error = Some(e);
                                    break;
                                }
                                None => break,
                                _ => {}
                            }
                        }
                        _ = flush_interval.tick() => {
                            flush_text_buffer(&action_tx, group_id, &mut text_buffer);
                        }
                    }
                }
            }
            Err(e) => {
                done.store(true, Ordering::Relaxed);
                progress_handle.abort();
                send_error(&action_tx, group_id, user_id, e.to_string());
                return;
            }
        }

        done.store(true, Ordering::Relaxed);
        progress_handle.abort();
        flush_text_buffer(&action_tx, group_id, &mut text_buffer);

        if let Some(err) = step_error {
            send_error(&action_tx, group_id, user_id, err);
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
        if !any_text_seen {
            send_addressed(
                &action_tx,
                group_id,
                user_id,
                "I thought about it but couldn't come up with a good answer. Try rephrasing?"
                    .to_string(),
            );
        }
        break;
    }

    if cancelled {
        send_addressed(
            &action_tx,
            group_id,
            user_id,
            "Cancelled the current reasoning.".to_string(),
        );
    }
}

/// Flush buffered streaming text as a plain group message.
fn flush_text_buffer(
    action_tx: &crate::onebot::ActionTx,
    group_id: i64,
    buffer: &mut String,
) {
    let trimmed = buffer.trim();
    if !trimmed.is_empty() {
        send_plain(action_tx, group_id, trimmed.to_string());
    }
    buffer.clear();
}
