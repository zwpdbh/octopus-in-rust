use crate::buffer::{BufferedMessage, MessageBuffer};
use crate::config::{BotConfig, LlmConfig};
use crate::llm;
use crate::onebot::{Action, Event, GroupMessage, OneBotClient};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Runtime state of the bot.
pub struct Bot {
    bot_config: BotConfig,
    llm_config: LlmConfig,
    buffer: Mutex<MessageBuffer>,
    client: OneBotClient,
    http: reqwest::Client,
}

impl Bot {
    pub fn new(bot_config: BotConfig, llm_config: LlmConfig, client: OneBotClient) -> Self {
        let buffer = MessageBuffer::new(bot_config.max_buffer_size, bot_config.summary_window_secs);
        Self {
            bot_config,
            llm_config,
            buffer: Mutex::new(buffer),
            client,
            http: reqwest::Client::new(),
        }
    }

    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Process a single OneBot event.
    pub async fn handle_event(self: &Arc<Self>, event: Event) {
        let Some(msg) = event.as_group_message() else {
            return;
        };

        if !self.bot_config.is_group_allowed(msg.group_id) {
            debug!(
                group_id = msg.group_id,
                "ignoring message from disallowed group"
            );
            return;
        }

        let is_command = self.is_command(&msg.raw_message);

        // Buffer the message before handling commands so the summary includes it.
        {
            let mut buffer = self.buffer.lock().await;
            buffer.push(msg.group_id, BufferedMessage::from(&msg));
        }

        if is_command {
            self.handle_command(&msg).await;
        }
    }

    fn is_command(&self, text: &str) -> bool {
        let trimmed = text.trim();
        !trimmed.is_empty() && trimmed.starts_with(&self.bot_config.command_prefix)
    }

    async fn handle_command(&self, msg: &GroupMessage) {
        let text = msg.raw_message.trim();
        let prefix = &self.bot_config.command_prefix;

        if text == format!("{prefix}summary") || text == format!("{prefix}s") {
            self.do_summary(msg.group_id).await;
        } else if text == format!("{prefix}help") {
            let help = format!(
                "Available commands:\n{}summary or {}s — summarize recent conversation\n{}help — show this message",
                prefix, prefix, prefix
            );
            self.reply(msg.group_id, &help).await;
        } else if text == format!("{prefix}status") {
            let len = self.buffer.lock().await.len(msg.group_id);
            self.reply(
                msg.group_id,
                &format!("Buffered messages in this group: {len}"),
            )
            .await;
        }
    }

    async fn do_summary(&self, group_id: i64) {
        let conversation = {
            let buffer = self.buffer.lock().await;
            if buffer.len(group_id) == 0 {
                self.reply(group_id, "No messages to summarize yet.").await;
                return;
            }
            buffer.format_context(group_id)
        };

        info!(group_id, "requesting summary from LLM");
        match llm::summarize(
            &self.http,
            &self.llm_config.api_url,
            &self.llm_config.api_key,
            &self.llm_config.model,
            &self.llm_config.system_prompt,
            &conversation,
        )
        .await
        {
            Ok(summary) => {
                self.reply(group_id, &summary).await;
            }
            Err(e) => {
                warn!(error = %e, group_id, "failed to generate summary");
                self.reply(group_id, "Sorry, I could not generate a summary right now.")
                    .await;
            }
        }
    }

    async fn reply(&self, group_id: i64, text: &str) {
        let action = Action::send_group_msg(group_id, text, None);
        if let Err(e) = self.client.send(action).await {
            warn!(error = %e, group_id, "failed to send reply");
        }
    }
}
