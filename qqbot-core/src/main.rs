mod config;
mod llm;
mod onebot;
mod plugin_host;

use crate::config::Config;
use crate::llm::LlmClient;
use crate::onebot::types::{Action, Event};
use crate::plugin_host::{discover_plugins, Plugin, PluginAction};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config.toml"));

    info!(config = %config_path.display(), "loading configuration");
    let config = Config::from_file(&config_path)?;
    info!("configuration loaded");

    let mut plugins = load_plugins(&config.bot.plugin_dir)?;
    if plugins.is_empty() {
        warn!("no plugins loaded");
    }

    let llm = LlmClient::new(
        config.llm.api_url.clone(),
        config.llm.api_key.clone(),
        config.llm.model.clone(),
        config.llm.system_prompt.clone(),
    );
    let llm = Arc::new(llm);

    info!(onebot_ws = %config.onebot.ws_url, "connecting to OneBot");
    let (mut event_rx, action_tx) =
        onebot::connect(&config.onebot.ws_url, &config.onebot.access_token).await?;

    info!("qqbot-core is running; press Ctrl+C to stop");

    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                handle_event(event, &config, &mut plugins, &action_tx, llm.clone()).await;
            }
            _ = &mut shutdown => {
                info!("shutdown signal received; exiting");
                break;
            }
            else => {
                warn!("event channel closed; exiting");
                break;
            }
        }
    }

    Ok(())
}

fn load_plugins(plugin_dir: &str) -> anyhow::Result<Vec<Plugin>> {
    let paths = discover_plugins(plugin_dir);
    let mut plugins = Vec::with_capacity(paths.len());
    for path in paths {
        match Plugin::load(path) {
            Ok(p) => plugins.push(p),
            Err(e) => error!(error = %e, "failed to load plugin"),
        }
    }
    Ok(plugins)
}

async fn handle_event(
    event: Event,
    config: &Config,
    plugins: &mut [Plugin],
    action_tx: &onebot::ActionTx,
    llm: Arc<LlmClient>,
) {
    if event.post_type != "message" {
        return;
    }

    if event.message_type.as_deref() != Some("group") {
        return;
    }

    let group_id = match event.group_id {
        Some(id) => id,
        None => return,
    };

    if !config.bot.is_group_allowed(group_id) {
        debug!(group_id, "group not allowed");
        return;
    }

    let text = event.text_message();
    let event_json = match serde_json::to_string(&event) {
        Ok(j) => j,
        Err(e) => {
            error!(error = %e, "failed to serialize event");
            return;
        }
    };

    // Dispatch to all plugins for message buffering.
    for plugin in plugins.iter_mut() {
        if let Err(e) = plugin.on_message(&event_json) {
            error!(plugin = %plugin.name(), error = %e, "on_message failed");
        }
    }

    // Handle commands.
    let prefix = &config.bot.command_prefix;
    if text.trim_start().starts_with(prefix) {
        let trimmed = text.trim_start().strip_prefix(prefix).unwrap_or("").trim();
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next().unwrap_or("");

        for plugin in plugins.iter_mut() {
            match plugin.on_command(cmd, &event_json) {
                Ok(actions) => {
                    for action in actions {
                        execute_action(action, group_id, action_tx, llm.clone()).await;
                    }
                }
                Err(e) => {
                    error!(plugin = %plugin.name(), error = %e, "on_command failed");
                }
            }
        }
    }
}

async fn execute_action(
    action: PluginAction,
    _group_id: i64,
    action_tx: &onebot::ActionTx,
    llm: Arc<LlmClient>,
) {
    match action {
        PluginAction::SendGroupMsg {
            group_id: gid,
            text,
        } => {
            let action = Action::send_group_msg(gid, text, None);
            if let Err(e) = action_tx.send(action) {
                error!(error = %e, "failed to send action");
            }
        }
        PluginAction::Log { level, message } => match level.as_str() {
            "error" => error!(message),
            "warn" => warn!(message),
            "debug" => debug!(message),
            _ => info!(message),
        },
        PluginAction::LlmRequest { group_id, prompt } => match llm.chat(&prompt).await {
            Ok(reply) => {
                let action = Action::send_group_msg(group_id, reply, None);
                if let Err(e) = action_tx.send(action) {
                    error!(error = %e, "failed to send LLM reply");
                }
            }
            Err(e) => {
                error!(error = %e, "LLM request failed");
                let action = Action::send_group_msg(
                    group_id,
                    format!("Sorry, I couldn't generate a summary right now: {}", e),
                    None,
                );
                let _ = action_tx.send(action);
            }
        },
    }
}
