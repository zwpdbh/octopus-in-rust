mod config;
mod group_brain;
mod llm_provider;
mod memory;
mod oauth;
mod onebot;

use crate::config::Config;
use crate::group_brain::GroupBrainManager;
use crate::memory::MemoryStore;
use crate::onebot::types::{
    Action, CommandEvent, GroupMessageEvent, MessageToBotEvent, MetaEvent, OneBotEvent,
    PrivateMessageEvent,
};
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

    info!(onebot_ws = %config.onebot.ws_url, "connecting to OneBot");
    let (mut event_rx, action_tx) = onebot::connect(
        &config.onebot.ws_url,
        &config.onebot.access_token,
        config.bot.bot_qq,
        config.bot.bot_aliases.clone(),
        config.bot.command_prefix.clone(),
    )
    .await?;

    let group_brains = Arc::new(GroupBrainManager::new(
        config.clone(),
        memory.clone(),
        plugin_dir.clone(),
        action_tx.clone(),
    ));

    info!("qqbot-core is running; press Ctrl+C to stop, SIGHUP to reload plugins");

    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    let mut sighup = signal(SignalKind::hangup()).context("failed to bind SIGHUP")?;

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                handle_onebot_event(
                    event,
                    &config,
                    &action_tx,
                    memory.clone(),
                    group_brains.clone(),
                ).await;
            }
            _ = sighup.recv() => {
                info!("SIGHUP received; clearing group brains so plugins reload on next use");
                group_brains.clear().await;
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

async fn handle_onebot_event(
    event: OneBotEvent,
    config: &Config,
    action_tx: &onebot::ActionTx,
    memory: MemoryStore,
    group_brains: Arc<GroupBrainManager>,
) {
    match event {
        OneBotEvent::MessageToBot(msg) => {
            handle_message_to_bot(msg, config, action_tx, memory, group_brains).await;
        }
        OneBotEvent::GroupChat(group_msg) => {
            handle_group_chat(group_msg, config, memory).await;
        }
        OneBotEvent::SystemCommand(cmd) => {
            handle_command(cmd, config, action_tx, memory, group_brains).await;
        }
        OneBotEvent::Notice(notice) => {
            debug!(?notice, "notice event ignored");
        }
        OneBotEvent::Request(request) => {
            debug!(?request, "request event ignored");
        }
        OneBotEvent::Meta(MetaEvent::Heartbeat) => {
            // Heartbeats are expected; keep them at trace if needed.
        }
        OneBotEvent::Meta(MetaEvent::Lifecycle { sub_type }) => {
            info!(sub_type, "OneBot lifecycle event");
        }
        OneBotEvent::Meta(MetaEvent::Other {
            meta_event_type, ..
        }) => {
            debug!(meta_event_type, "unknown meta event");
        }
        OneBotEvent::Unknown(value) => {
            debug!(?value, "unknown OneBot event");
        }
    }
}

async fn handle_message_to_bot(
    msg: MessageToBotEvent,
    config: &Config,
    action_tx: &onebot::ActionTx,
    memory: MemoryStore,
    group_brains: Arc<GroupBrainManager>,
) {
    match msg {
        MessageToBotEvent::Group(event) => {
            handle_addressed_group_message(event, config, action_tx, memory, group_brains).await;
        }
        MessageToBotEvent::Private(event) => {
            handle_private_message(event, memory).await;
        }
    }
}

async fn handle_addressed_group_message(
    event: GroupMessageEvent,
    config: &Config,
    action_tx: &onebot::ActionTx,
    memory: MemoryStore,
    group_brains: Arc<GroupBrainManager>,
) {
    let group_id = event.group_id;
    let user_id = event.user_id;
    let text = event.text();
    let prompt = event.prompt_text(config.bot.bot_qq, &config.bot.bot_aliases);

    // Remember every group message for tools like qqbot::recent_messages.
    memory.push(group_id, user_id, text.clone());

    let req_id = uuid::Uuid::new_v4().to_string();
    let span = info_span!(
        "addressed_group_message",
        request_id = %req_id,
        group_id,
        user_id,
    );

    async {
        debug!(text = %text, prompt = %prompt, "received addressed group message");

        if !config.bot.is_group_allowed(group_id) {
            debug!(group_id, "group not allowed");
            return;
        }

        if prompt.is_empty() {
            debug!("bot addressed with no prompt; sending invitation");
            let action = if let Some(id) = event.message_id {
                Action::quote_group_msg(group_id, id, "Hi! What would you like me to do?", None)
            } else {
                Action::send_group_msg(group_id, "Hi! What would you like me to do?", None)
            };
            let _ = action_tx.send(action);
            return;
        }

        info!(prompt = %prompt, "handling addressed prompt");
        group_brains
            .handle_prompt(group_id, user_id, event.message_id, prompt)
            .await;
    }
    .instrument(span)
    .await;
}

async fn handle_private_message(event: PrivateMessageEvent, memory: MemoryStore) {
    // Private messages are directed to the bot by definition. For now we just
    // store them in memory (keyed by user_id) and log; replies are not yet
    // implemented.
    memory.push(event.user_id, event.user_id, event.text());
    debug!(user_id = event.user_id, text = %event.text(), "private message ignored");
}

async fn handle_group_chat(event: GroupMessageEvent, config: &Config, memory: MemoryStore) {
    let group_id = event.group_id;
    let user_id = event.user_id;
    let text = event.text();

    // Remember every group message for tools like qqbot::recent_messages.
    memory.push(group_id, user_id, text.clone());

    let span = info_span!("group_chat", group_id, user_id,);

    async {
        debug!(text = %text, "received group chat");

        if !config.bot.is_group_allowed(group_id) {
            debug!(group_id, "group not allowed");
            return;
        }

        // Plain message: stored in memory; no reply needed.
        debug!("message not addressed to bot; ignoring");
    }
    .instrument(span)
    .await;
}

async fn handle_command(
    cmd: CommandEvent,
    config: &Config,
    action_tx: &onebot::ActionTx,
    memory: MemoryStore,
    group_brains: Arc<GroupBrainManager>,
) {
    let req_id = uuid::Uuid::new_v4().to_string();

    match cmd {
        CommandEvent::Status {
            group_id, message_id, ..
        } => {
            if !config.bot.is_group_allowed(group_id) {
                debug!(group_id, "group not allowed");
                return;
            }
            info!(group_id, "handling /status command");
            handle_status(group_id, message_id, action_tx, memory, &req_id).await;
        }
        CommandEvent::Help {
            group_id, message_id, ..
        } => {
            if !config.bot.is_group_allowed(group_id) {
                debug!(group_id, "group not allowed");
                return;
            }
            info!(group_id, "handling /help command");
            handle_help(group_id, message_id, action_tx, &req_id).await;
        }
        CommandEvent::Cancel {
            group_id,
            user_id,
            message_id,
        } => {
            if !config.bot.is_group_allowed(group_id) {
                debug!(group_id, "group not allowed");
                return;
            }
            info!(group_id, "handling /cancel command");
            let cancelled = group_brains.cancel_turn(group_id).await;
            if cancelled {
                // The worker will send the cancellation confirmation after it
                // safely stops at the next step boundary.
            } else {
                let action = match message_id {
                    Some(id) => Action::quote_group_msg(
                        group_id,
                        id,
                        "No active reasoning to cancel.",
                        None,
                    ),
                    None => Action::reply_group_msg(
                        group_id,
                        user_id,
                        "No active reasoning to cancel.",
                        None,
                    ),
                };
                if let Err(e) = action_tx.send(action) {
                    error!(request_id = %req_id, error = %e, "failed to send cancel reply");
                }
            }
        }
        CommandEvent::Unknown {
            group_id, command, ..
        } => {
            debug!(group_id, command, "unknown command; ignoring");
        }
    }
}

async fn handle_status(
    group_id: i64,
    message_id: Option<i32>,
    action_tx: &onebot::ActionTx,
    memory: MemoryStore,
    req_id: &str,
) {
    let count = memory.len(group_id);
    let text = format!("Buffered {} messages in this group.", count);
    let action = match message_id {
        Some(id) => Action::quote_group_msg(group_id, id, text, None),
        None => Action::send_group_msg(group_id, text, None),
    };
    if let Err(e) = action_tx.send(action) {
        error!(request_id = %req_id, error = %e, "failed to send status reply");
    }
}

async fn handle_help(
    group_id: i64,
    message_id: Option<i32>,
    action_tx: &onebot::ActionTx,
    req_id: &str,
) {
    let text = "Mention me with @bot <question>, or use /status and /help.".to_string();
    let action = match message_id {
        Some(id) => Action::quote_group_msg(group_id, id, text, None),
        None => Action::send_group_msg(group_id, text, None),
    };
    if let Err(e) = action_tx.send(action) {
        error!(request_id = %req_id, error = %e, "failed to send help reply");
    }
}
