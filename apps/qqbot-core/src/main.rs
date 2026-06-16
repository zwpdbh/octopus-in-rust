mod config;
mod group_brain;
mod memory;
mod oauth;
mod onebot;

use crate::config::Config;
use crate::group_brain::GroupBrainManager;
use crate::memory::MemoryStore;
use crate::onebot::types::{Action, Event};
use anyhow::Context;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tracing::{debug, error, info, info_span, warn, Instrument};

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
    info!(bot_qq = config.bot.bot_qq, "configuration loaded");

    let plugin_dir = PathBuf::from(&config.bot.plugin_dir);

    let memory = MemoryStore::new(200);

    let oauth = config
        .llm
        .oauth
        .clone()
        .map(crate::oauth::OAuthManager::new);
    let group_brains = Arc::new(GroupBrainManager::new(
        config.clone(),
        memory.clone(),
        oauth,
        plugin_dir.clone(),
    ));

    info!(onebot_ws = %config.onebot.ws_url, "connecting to OneBot");
    let (mut event_rx, action_tx) =
        onebot::connect(&config.onebot.ws_url, &config.onebot.access_token).await?;

    info!("qqbot-core is running; press Ctrl+C to stop, SIGHUP to reload plugins");

    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    let mut sighup = signal(SignalKind::hangup()).context("failed to bind SIGHUP")?;

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                handle_event(
                    event,
                    &config,
                    &action_tx,
                    memory.clone(),
                    group_brains.clone(),
                ).await;
            }
            _ = sighup.recv() => {
                info!("SIGHUP received; clearing group brains so plugins reload on next use");
                group_brains.clear();
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

async fn handle_event(
    event: Event,
    config: &Config,
    action_tx: &onebot::ActionTx,
    memory: MemoryStore,
    group_brains: Arc<GroupBrainManager>,
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

    // Remember every group message for tools like qqbot::recent_messages.
    if let Some(user_id) = event.user_id {
        memory.push(group_id, user_id, text.clone());
    }

    let req_id = uuid::Uuid::new_v4().to_string();
    let span = info_span!(
        "handle_event",
        request_id = %req_id,
        group_id,
        user_id = ?event.user_id,
    );

    async {
        let user_id = event.user_id.unwrap_or(0);

        // 1. Explicit @-mention: treat the remaining text as a natural-language
        //    prompt and run it through the group's Brain.
        let bot_qq = config.bot.bot_qq;
        if bot_qq != 0 && event.is_at(bot_qq) {
            let prompt = event.prompt_text(bot_qq);
            if prompt.is_empty() {
                debug!("bot @-mentioned with no prompt; ignoring");
                return;
            }
            info!(prompt = %prompt, "handling @-mention prompt");
            handle_brain_turn(
                group_id,
                user_id,
                action_tx,
                group_brains.clone(),
                prompt,
                &req_id,
            )
            .await;
            return;
        }

        // 2. Command prefix: legacy shorthand for natural-language tasks.
        let prefix = &config.bot.command_prefix;
        if text.trim_start().starts_with(prefix) {
            let trimmed = text.trim_start().strip_prefix(prefix).unwrap_or("").trim();
            let mut parts = trimmed.split_whitespace();
            let cmd = parts.next().unwrap_or("");

            info!(cmd, "handling command");
            match cmd {
                "status" => {
                    handle_status(group_id, action_tx, memory.clone(), &req_id).await;
                }
                "help" | "h" => {
                    handle_help(group_id, action_tx, &req_id).await;
                }
                _ => {
                    debug!(cmd, "unknown command");
                }
            }
            return;
        }

        // 3. Plain message: do not reply, but it is already stored in memory.
        debug!("plain group message; not replying");
    }
    .instrument(span)
    .await;
}

async fn handle_brain_turn(
    group_id: i64,
    user_id: i64,
    action_tx: &onebot::ActionTx,
    group_brains: Arc<GroupBrainManager>,
    user_message: String,
    req_id: &str,
) {
    info!(request_id = %req_id, group_id, "running Brain turn");

    let processing_text = "🤔 Thinking...".to_string();
    let _ = action_tx.send(Action::send_group_msg(group_id, processing_text, None));

    match group_brains.run_turn(group_id, user_message).await {
        Ok(result) => {
            let reply = if result.final_text.is_empty() {
                "I couldn't come up with an answer.".to_string()
            } else {
                result.final_text
            };
            let action = if user_id != 0 {
                Action::reply_group_msg(group_id, user_id, reply, None)
            } else {
                Action::send_group_msg(group_id, reply, None)
            };
            if let Err(e) = action_tx.send(action) {
                error!(request_id = %req_id, error = %e, "failed to send Brain reply");
            }
        }
        Err(e) => {
            error!(request_id = %req_id, error = %e, "Brain turn failed");
            let action = if user_id != 0 {
                Action::reply_group_msg(
                    group_id,
                    user_id,
                    format!(
                        "Sorry, I couldn't process that right now: {} (request: {})",
                        e, req_id
                    ),
                    None,
                )
            } else {
                Action::send_group_msg(
                    group_id,
                    format!(
                        "Sorry, I couldn't process that right now: {} (request: {})",
                        e, req_id
                    ),
                    None,
                )
            };
            let _ = action_tx.send(action);
        }
    }
}

async fn handle_status(
    group_id: i64,
    action_tx: &onebot::ActionTx,
    memory: MemoryStore,
    req_id: &str,
) {
    let count = memory.len(group_id);
    let text = format!("Buffered {} messages in this group.", count);
    let action = Action::send_group_msg(group_id, text, None);
    if let Err(e) = action_tx.send(action) {
        error!(request_id = %req_id, error = %e, "failed to send status reply");
    }
}

async fn handle_help(group_id: i64, action_tx: &onebot::ActionTx, req_id: &str) {
    let text = "Mention me with @bot <question>, or use /status and /help.".to_string();
    let action = Action::send_group_msg(group_id, text, None);
    if let Err(e) = action_tx.send(action) {
        error!(request_id = %req_id, error = %e, "failed to send help reply");
    }
}
