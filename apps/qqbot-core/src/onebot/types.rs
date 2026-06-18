#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A parsed OneBot v11 event.
#[derive(Debug, Clone)]
pub enum OneBotEvent {
    /// A message explicitly directed at the bot.
    MessageToBot(MessageToBotEvent),
    /// A group message not directed at the bot.
    GroupChat(GroupMessageEvent),
    /// A bot command issued in a group.
    SystemCommand(CommandEvent),
    /// A notice event (member join/leave, ban, etc.).
    Notice(NoticeEvent),
    /// A friend or group request.
    Request(RequestEvent),
    /// A meta event (lifecycle, heartbeat).
    Meta(MetaEvent),
    /// Any event we do not yet model.
    Unknown(serde_json::Value),
}

/// A command issued by a group member.
#[derive(Debug, Clone)]
pub enum CommandEvent {
    /// `/status` command.
    Status {
        group_id: i64,
        user_id: i64,
        message_id: Option<i32>,
    },
    /// `/help` command.
    Help {
        group_id: i64,
        user_id: i64,
        message_id: Option<i32>,
    },
    /// `/cancel` (or `/c`) command.
    Cancel {
        group_id: i64,
        user_id: i64,
        message_id: Option<i32>,
    },
    /// Any other `/` prefixed command we do not handle.
    Unknown {
        group_id: i64,
        user_id: i64,
        message_id: Option<i32>,
        command: String,
    },
}

/// A message explicitly directed at the bot.
#[derive(Debug, Clone)]
pub enum MessageToBotEvent {
    /// Bot was @-mentioned in a group, or addressed via text alias.
    Group(GroupMessageEvent),
    /// Private message sent to the bot.
    Private(PrivateMessageEvent),
}

/// The body of a OneBot v11 group message event.
#[derive(Debug, Clone)]
pub struct GroupMessageEvent {
    pub message_id: Option<i32>,
    pub group_id: i64,
    pub user_id: i64,
    pub message: MessageContent,
    pub raw_message: Option<String>,
    pub sender: SenderInfo,
}

/// The body of a OneBot v11 private message event.
#[derive(Debug, Clone)]
pub struct PrivateMessageEvent {
    pub message_id: Option<i32>,
    pub user_id: i64,
    pub message: MessageContent,
    pub raw_message: Option<String>,
    pub sender: SenderInfo,
}

/// A notice event.
#[derive(Debug, Clone)]
pub enum NoticeEvent {
    GroupMemberIncrease {
        group_id: i64,
        user_id: i64,
        operator_id: Option<i64>,
    },
    GroupMemberDecrease {
        group_id: i64,
        user_id: i64,
        operator_id: Option<i64>,
    },
    Other {
        notice_type: String,
        payload: serde_json::Value,
    },
}

/// A friend or group request event.
#[derive(Debug, Clone)]
pub enum RequestEvent {
    Friend {
        user_id: i64,
        comment: Option<String>,
        flag: String,
    },
    Group {
        group_id: i64,
        user_id: i64,
        comment: Option<String>,
        flag: String,
    },
}

/// A meta event.
#[derive(Debug, Clone)]
pub enum MetaEvent {
    Lifecycle {
        sub_type: String,
    },
    Heartbeat,
    Other {
        meta_event_type: String,
        payload: serde_json::Value,
    },
}

/// OneBot message content, which may arrive as a string or as an array of segments.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Segments(Vec<MessageSegment>),
}

impl Default for MessageContent {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl MessageContent {
    /// Human-readable text extracted from the message.
    ///
    /// For segment arrays, only `text` segments are concatenated. For string
    /// content, the string is returned as-is.
    pub fn text(&self) -> String {
        match self {
            MessageContent::Text(s) => s.clone(),
            MessageContent::Segments(segments) => segments
                .iter()
                .filter_map(|seg| {
                    if seg.seg_type == "text" {
                        seg.data
                            .get("text")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }

    /// Return the parsed segments, if the message was an array.
    pub fn segments(&self) -> Option<&[MessageSegment]> {
        match self {
            MessageContent::Segments(segments) => Some(segments),
            MessageContent::Text(_) => None,
        }
    }

    /// Whether the message contains a real `at` segment targeting `bot_qq`.
    pub fn has_at(&self, bot_qq: i64) -> bool {
        self.segments()
            .is_some_and(|segments| segments.iter().any(|seg| segment_at_qq(seg, bot_qq)))
    }
}

/// A single message segment in OneBot array format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSegment {
    #[serde(rename = "type")]
    pub seg_type: String,
    pub data: serde_json::Map<String, serde_json::Value>,
}

/// Information about the sender of a message.
#[derive(Debug, Clone, Default)]
pub struct SenderInfo {
    pub user_id: i64,
    pub nickname: Option<String>,
    pub role: Option<String>,
    pub extras: HashMap<String, serde_json::Value>,
}

/// How the bot is addressed in a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Addressing {
    /// Real OneBot `at` segment targeting the bot.
    AtSegment,
    /// Plain-text `@<target>` where target is the bot's QQ or an alias.
    TextMention(String),
    /// Not addressed to the bot.
    None,
}

impl OneBotEvent {
    /// Parse a raw OneBot v11 JSON payload into a typed event.
    ///
    /// The bot identity (`bot_qq` and `aliases`) and `command_prefix` are used
    /// immediately to distinguish messages addressed to the bot, bot commands,
    /// and general group traffic.
    pub fn from_json(
        value: serde_json::Value,
        bot_qq: i64,
        aliases: &[String],
        command_prefix: &str,
    ) -> Result<Self, ParseError> {
        let post_type = value
            .get("post_type")
            .and_then(|v| v.as_str())
            .ok_or(ParseError::MissingField("post_type"))?
            .to_string();

        match post_type.as_str() {
            "message" => Self::parse_message(value, bot_qq, aliases, command_prefix),
            "notice" => Self::parse_notice(value),
            "request" => Self::parse_request(value),
            "meta_event" => Self::parse_meta(value),
            _ => Ok(OneBotEvent::Unknown(value)),
        }
    }

    fn parse_message(
        value: serde_json::Value,
        bot_qq: i64,
        aliases: &[String],
        command_prefix: &str,
    ) -> Result<Self, ParseError> {
        let message_type = value
            .get("message_type")
            .and_then(|v| v.as_str())
            .ok_or(ParseError::MissingField("message_type"))?
            .to_string();

        let message = value
            .get("message")
            .cloned()
            .map(MessageContent::deserialize)
            .transpose()
            .map_err(|e| ParseError::InvalidMessage(e.to_string()))?
            .unwrap_or_default();

        let raw_message = value
            .get("raw_message")
            .and_then(|v| v.as_str())
            .map(String::from);

        let message_id = value
            .get("message_id")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32);
        let user_id = value
            .get("user_id")
            .and_then(|v| v.as_i64())
            .ok_or(ParseError::MissingField("user_id"))?;
        let sender = parse_sender(&value);

        match message_type.as_str() {
            "group" => {
                let group_id = value
                    .get("group_id")
                    .and_then(|v| v.as_i64())
                    .ok_or(ParseError::MissingField("group_id"))?;
                let event = GroupMessageEvent {
                    message_id,
                    group_id,
                    user_id,
                    message,
                    raw_message,
                    sender,
                };

                // 1. Plain command in group chat.
                if let Some(kind) = parse_command_kind(&event.text(), command_prefix) {
                    return Ok(OneBotEvent::SystemCommand(CommandEvent::from_kind(
                        group_id, user_id, message_id, kind,
                    )));
                }

                // 2. Addressed to the bot.
                if event.addressing(bot_qq, aliases) != Addressing::None {
                    let prompt = event.prompt_text(bot_qq, aliases);

                    // 2a. Addressed command (e.g. "@bot /status").
                    if let Some(kind) = parse_command_kind(&prompt, command_prefix) {
                        return Ok(OneBotEvent::SystemCommand(CommandEvent::from_kind(
                            group_id, user_id, message_id, kind,
                        )));
                    }

                    // 2b. Addressed natural-language prompt.
                    return Ok(OneBotEvent::MessageToBot(MessageToBotEvent::Group(event)));
                }

                // 3. Plain group chat.
                Ok(OneBotEvent::GroupChat(event))
            }
            "private" => Ok(OneBotEvent::MessageToBot(MessageToBotEvent::Private(
                PrivateMessageEvent {
                    message_id,
                    user_id,
                    message,
                    raw_message,
                    sender,
                },
            ))),
            _ => Ok(OneBotEvent::Unknown(value)),
        }
    }

    fn parse_notice(value: serde_json::Value) -> Result<Self, ParseError> {
        let notice_type = value
            .get("notice_type")
            .and_then(|v| v.as_str())
            .ok_or(ParseError::MissingField("notice_type"))?
            .to_string();

        let event = match notice_type.as_str() {
            "group_increase" => {
                let group_id = value
                    .get("group_id")
                    .and_then(|v| v.as_i64())
                    .ok_or(ParseError::MissingField("group_id"))?;
                let user_id = value
                    .get("user_id")
                    .and_then(|v| v.as_i64())
                    .ok_or(ParseError::MissingField("user_id"))?;
                let operator_id = value.get("operator_id").and_then(|v| v.as_i64());
                NoticeEvent::GroupMemberIncrease {
                    group_id,
                    user_id,
                    operator_id,
                }
            }
            "group_decrease" => {
                let group_id = value
                    .get("group_id")
                    .and_then(|v| v.as_i64())
                    .ok_or(ParseError::MissingField("group_id"))?;
                let user_id = value
                    .get("user_id")
                    .and_then(|v| v.as_i64())
                    .ok_or(ParseError::MissingField("user_id"))?;
                let operator_id = value.get("operator_id").and_then(|v| v.as_i64());
                NoticeEvent::GroupMemberDecrease {
                    group_id,
                    user_id,
                    operator_id,
                }
            }
            _ => NoticeEvent::Other {
                notice_type,
                payload: value.clone(),
            },
        };

        Ok(OneBotEvent::Notice(event))
    }

    fn parse_request(value: serde_json::Value) -> Result<Self, ParseError> {
        let request_type = value
            .get("request_type")
            .and_then(|v| v.as_str())
            .ok_or(ParseError::MissingField("request_type"))?
            .to_string();

        let user_id = value
            .get("user_id")
            .and_then(|v| v.as_i64())
            .ok_or(ParseError::MissingField("user_id"))?;
        let comment = value
            .get("comment")
            .and_then(|v| v.as_str())
            .map(String::from);
        let flag = value
            .get("flag")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or(ParseError::MissingField("flag"))?;

        match request_type.as_str() {
            "friend" => Ok(OneBotEvent::Request(RequestEvent::Friend {
                user_id,
                comment,
                flag,
            })),
            "group" => {
                let group_id = value
                    .get("group_id")
                    .and_then(|v| v.as_i64())
                    .ok_or(ParseError::MissingField("group_id"))?;
                Ok(OneBotEvent::Request(RequestEvent::Group {
                    group_id,
                    user_id,
                    comment,
                    flag,
                }))
            }
            _ => Ok(OneBotEvent::Unknown(value)),
        }
    }

    fn parse_meta(value: serde_json::Value) -> Result<Self, ParseError> {
        let meta_event_type = value
            .get("meta_event_type")
            .and_then(|v| v.as_str())
            .ok_or(ParseError::MissingField("meta_event_type"))?
            .to_string();

        let event = match meta_event_type.as_str() {
            "lifecycle" => {
                let sub_type = value
                    .get("sub_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                MetaEvent::Lifecycle { sub_type }
            }
            "heartbeat" => MetaEvent::Heartbeat,
            _ => MetaEvent::Other {
                meta_event_type,
                payload: value.clone(),
            },
        };

        Ok(OneBotEvent::Meta(event))
    }
}

impl PrivateMessageEvent {
    /// Human-readable text extracted from the message.
    pub fn text(&self) -> String {
        self.message.text()
    }
}

impl GroupMessageEvent {
    /// Human-readable text extracted from the message.
    pub fn text(&self) -> String {
        self.message.text()
    }

    /// Determine how the bot is addressed in this message.
    pub fn addressing(&self, bot_qq: i64, aliases: &[String]) -> Addressing {
        // Prefer real OneBot at segments.
        if self.message.has_at(bot_qq) {
            return Addressing::AtSegment;
        }

        // Fall back to plain-text @<qq> or @<alias>.
        if let Some(target) = text_mention_target(&self.text(), bot_qq, aliases) {
            return Addressing::TextMention(target);
        }

        Addressing::None
    }

    /// Extract the prompt text with the bot's mention removed.
    pub fn prompt_text(&self, bot_qq: i64, aliases: &[String]) -> String {
        match self.addressing(bot_qq, aliases) {
            Addressing::AtSegment => self
                .message
                .segments()
                .map(|segments| {
                    segments
                        .iter()
                        .filter_map(|seg| {
                            if seg.seg_type == "text" {
                                seg.data
                                    .get("text")
                                    .and_then(|v| v.as_str())
                                    .map(String::from)
                            } else if seg.seg_type == "at" && segment_at_qq(seg, bot_qq) {
                                // Drop @ segments targeting the bot.
                                Some(String::new())
                            } else {
                                None
                            }
                        })
                        .collect::<String>()
                        .trim()
                        .to_string()
                })
                .unwrap_or_else(|| self.text().trim().to_string()),
            Addressing::TextMention(target) => {
                let text = self.text();
                let trimmed = text.trim_start();
                let prefix = format!("@{}", target);
                trimmed
                    .strip_prefix(&prefix)
                    .map(|rest| rest.trim().to_string())
                    .unwrap_or_else(|| text.trim().to_string())
            }
            Addressing::None => self.text().trim().to_string(),
        }
    }
}

/// The recognized kind of a bot command.
#[derive(Debug, Clone)]
enum CommandKind {
    Status,
    Help,
    Cancel,
    Unknown(String),
}

impl CommandEvent {
    fn from_kind(group_id: i64, user_id: i64, message_id: Option<i32>, kind: CommandKind) -> Self {
        match kind {
            CommandKind::Status => CommandEvent::Status {
                group_id,
                user_id,
                message_id,
            },
            CommandKind::Help => CommandEvent::Help {
                group_id,
                user_id,
                message_id,
            },
            CommandKind::Cancel => CommandEvent::Cancel {
                group_id,
                user_id,
                message_id,
            },
            CommandKind::Unknown(command) => CommandEvent::Unknown {
                group_id,
                user_id,
                message_id,
                command,
            },
        }
    }
}

/// If `text` starts with `command_prefix` and names a command, return its kind.
fn parse_command_kind(text: &str, command_prefix: &str) -> Option<CommandKind> {
    if command_prefix.is_empty() {
        return None;
    }
    let trimmed = text.trim_start();
    if !trimmed.starts_with(command_prefix) {
        return None;
    }
    let after_prefix = trimmed.strip_prefix(command_prefix).unwrap_or("").trim();
    let mut parts = after_prefix.split_whitespace();
    let cmd = parts.next()?;
    let kind = match cmd {
        "status" => CommandKind::Status,
        "help" | "h" => CommandKind::Help,
        "cancel" | "c" => CommandKind::Cancel,
        _ => CommandKind::Unknown(cmd.to_string()),
    };
    Some(kind)
}

fn parse_sender(value: &serde_json::Value) -> SenderInfo {
    let mut info = SenderInfo::default();
    if let Some(sender) = value.get("sender").and_then(|v| v.as_object()) {
        info.user_id = sender
            .get("user_id")
            .and_then(|v| v.as_i64())
            .unwrap_or_default();
        info.nickname = sender
            .get("nickname")
            .and_then(|v| v.as_str())
            .map(String::from);
        info.role = sender
            .get("role")
            .and_then(|v| v.as_str())
            .map(String::from);
        for (k, v) in sender {
            if !matches!(k.as_str(), "user_id" | "nickname" | "role") {
                info.extras.insert(k.clone(), v.clone());
            }
        }
    } else {
        // Fallback to top-level user_id.
        info.user_id = value
            .get("user_id")
            .and_then(|v| v.as_i64())
            .unwrap_or_default();
    }
    info
}

fn segment_at_qq(seg: &MessageSegment, bot_qq: i64) -> bool {
    if seg.seg_type != "at" {
        return false;
    }
    seg.data.get("qq").and_then(|v| match v {
        serde_json::Value::String(s) => s.parse::<i64>().ok(),
        serde_json::Value::Number(n) => n.as_i64(),
        _ => None,
    }) == Some(bot_qq)
}

/// If `text` starts with `@<bot_qq>` or `@<alias>`, return the matched target.
fn text_mention_target(text: &str, bot_qq: i64, aliases: &[String]) -> Option<String> {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('@') {
        return None;
    }
    let rest = &trimmed[1..];
    let target = rest.split_whitespace().next()?;
    let target_lower = target.to_lowercase();
    let qq_str = bot_qq.to_string();
    if target == qq_str || aliases.iter().any(|a| a.to_lowercase() == target_lower) {
        Some(target.to_string())
    } else {
        None
    }
}

/// Errors that can occur when parsing a OneBot event.
#[derive(Debug, Clone)]
pub enum ParseError {
    MissingField(&'static str),
    InvalidMessage(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::MissingField(field) => write!(f, "missing required field: {}", field),
            ParseError::InvalidMessage(e) => write!(f, "invalid message content: {}", e),
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, Serialize)]
pub struct Action {
    pub action: String,
    pub params: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub echo: Option<String>,
}

impl Action {
    pub fn send_group_msg(group_id: i64, text: impl Into<String>, echo: Option<String>) -> Self {
        Self {
            action: "send_group_msg".to_string(),
            params: serde_json::json!({
                "group_id": group_id,
                "message": [{"type": "text", "data": {"text": text.into()}}],
            }),
            echo,
        }
    }

    /// Reply in a group with an @ to the target user.
    pub fn reply_group_msg(
        group_id: i64,
        user_id: i64,
        text: impl Into<String>,
        echo: Option<String>,
    ) -> Self {
        Self::send_group_msg_with_mentions(group_id, &[user_id], text, echo)
    }

    /// Send a group message that @-mentions multiple users, followed by text.
    pub fn send_group_msg_with_mentions(
        group_id: i64,
        user_ids: &[i64],
        text: impl Into<String>,
        echo: Option<String>,
    ) -> Self {
        let mut message: Vec<serde_json::Value> = user_ids
            .iter()
            .map(|user_id| {
                serde_json::json!({
                    "type": "at",
                    "data": {"qq": user_id.to_string()}
                })
            })
            .collect();
        message.push(serde_json::json!({
            "type": "text",
            "data": {"text": format!(" {}", text.into())}
        }));

        Self {
            action: "send_group_msg".to_string(),
            params: serde_json::json!({
                "group_id": group_id,
                "message": message,
            }),
            echo,
        }
    }

    /// Quote (reply to) a specific group message.
    ///
    /// OneBot v11 supports a `reply` segment whose `id` is the `message_id`
    /// of the message being quoted. NapCat/SnowLuma render this as a QQ quote.
    pub fn quote_group_msg(
        group_id: i64,
        message_id: i32,
        text: impl Into<String>,
        echo: Option<String>,
    ) -> Self {
        Self {
            action: "send_group_msg".to_string(),
            params: serde_json::json!({
                "group_id": group_id,
                "message": [
                    {"type": "reply", "data": {"id": message_id}},
                    {"type": "text", "data": {"text": text.into()}},
                ],
            }),
            echo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group_event_with_text(text: impl Into<String>) -> GroupMessageEvent {
        GroupMessageEvent {
            message_id: Some(1),
            group_id: 123,
            user_id: 456,
            message: MessageContent::Text(text.into()),
            raw_message: None,
            sender: SenderInfo::default(),
        }
    }

    fn group_event_with_segments(segments: Vec<MessageSegment>) -> GroupMessageEvent {
        GroupMessageEvent {
            message_id: Some(1),
            group_id: 123,
            user_id: 456,
            message: MessageContent::Segments(segments),
            raw_message: None,
            sender: SenderInfo::default(),
        }
    }

    fn text_segment(text: &str) -> MessageSegment {
        let mut data = serde_json::Map::new();
        data.insert(
            "text".to_string(),
            serde_json::Value::String(text.to_string()),
        );
        MessageSegment {
            seg_type: "text".to_string(),
            data,
        }
    }

    fn at_segment(qq: i64) -> MessageSegment {
        let mut data = serde_json::Map::new();
        data.insert("qq".to_string(), serde_json::Value::String(qq.to_string()));
        MessageSegment {
            seg_type: "at".to_string(),
            data,
        }
    }

    #[test]
    fn test_text_mention_with_alias() {
        let event = group_event_with_text("@zw112233 what is faf");
        let aliases = vec!["zw112233".to_string()];
        assert_eq!(
            event.addressing(3462039501, &aliases),
            Addressing::TextMention("zw112233".to_string())
        );
        assert_eq!(event.prompt_text(3462039501, &aliases), "what is faf");
    }

    #[test]
    fn test_text_mention_with_qq() {
        let event = group_event_with_text("@3462039501 what is faf");
        assert_eq!(
            event.addressing(3462039501, &[]),
            Addressing::TextMention("3462039501".to_string())
        );
        assert_eq!(event.prompt_text(3462039501, &[]), "what is faf");
    }

    #[test]
    fn test_text_mention_case_insensitive() {
        let event = group_event_with_text("@ZW112233 what is faf");
        let aliases = vec!["zw112233".to_string()];
        assert!(matches!(
            event.addressing(3462039501, &aliases),
            Addressing::TextMention(_)
        ));
        assert_eq!(event.prompt_text(3462039501, &aliases), "what is faf");
    }

    #[test]
    fn test_real_at_segment() {
        let event =
            group_event_with_segments(vec![at_segment(3462039501), text_segment(" what is faf")]);
        assert_eq!(event.addressing(3462039501, &[]), Addressing::AtSegment);
        assert_eq!(event.prompt_text(3462039501, &[]), "what is faf");
    }

    #[test]
    fn test_not_addressed() {
        let event = group_event_with_text("what is faf");
        assert_eq!(event.addressing(3462039501, &[]), Addressing::None);
        assert_eq!(event.prompt_text(3462039501, &[]), "what is faf");
    }

    #[test]
    fn test_at_other_user_ignored() {
        let event =
            group_event_with_segments(vec![at_segment(999999999), text_segment(" what is faf")]);
        assert_eq!(event.addressing(3462039501, &[]), Addressing::None);
    }

    #[test]
    fn test_parse_addressed_group_message_text() {
        let json = serde_json::json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 925712027,
            "user_id": 123456789,
            "message_id": 42,
            "message": "@zw112233 what is faf",
            "sender": {
                "user_id": 123456789,
                "nickname": "Alice"
            }
        });

        let aliases = vec!["zw112233".to_string()];
        let event = OneBotEvent::from_json(json, 3462039501, &aliases, "/").unwrap();
        let OneBotEvent::MessageToBot(MessageToBotEvent::Group(group)) = event else {
            panic!("expected MessageToBot::Group");
        };
        assert_eq!(group.group_id, 925712027);
        assert_eq!(group.user_id, 123456789);
        assert_eq!(group.message_id, Some(42));
        assert_eq!(group.sender.nickname.as_deref(), Some("Alice"));
        assert_eq!(
            group.addressing(3462039501, &aliases),
            Addressing::TextMention("zw112233".to_string())
        );
        assert_eq!(group.prompt_text(3462039501, &aliases), "what is faf");
    }

    #[test]
    fn test_parse_addressed_group_message_segments() {
        let json = serde_json::json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 925712027,
            "user_id": 123456789,
            "message": [
                {"type": "at", "data": {"qq": "3462039501"}},
                {"type": "text", "data": {"text": " what is faf"}}
            ],
            "sender": {"user_id": 123456789}
        });

        let event = OneBotEvent::from_json(json, 3462039501, &[], "/").unwrap();
        let OneBotEvent::MessageToBot(MessageToBotEvent::Group(group)) = event else {
            panic!("expected MessageToBot::Group");
        };

        assert_eq!(group.addressing(3462039501, &[]), Addressing::AtSegment);
        assert_eq!(group.prompt_text(3462039501, &[]), "what is faf");
    }

    #[test]
    fn test_parse_non_addressed_group_message() {
        let json = serde_json::json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 925712027,
            "user_id": 123456789,
            "message": "what is faf",
            "sender": {"user_id": 123456789}
        });

        let event = OneBotEvent::from_json(json, 3462039501, &[], "/").unwrap();
        let OneBotEvent::GroupChat(group) = event else {
            panic!("expected GroupChat");
        };
        assert_eq!(group.group_id, 925712027);
        assert_eq!(group.addressing(3462039501, &[]), Addressing::None);
    }

    #[test]
    fn test_parse_private_message() {
        let json = serde_json::json!({
            "post_type": "message",
            "message_type": "private",
            "user_id": 123456789,
            "message": "hello bot",
            "sender": {"user_id": 123456789}
        });

        let event = OneBotEvent::from_json(json, 3462039501, &[], "/").unwrap();
        let OneBotEvent::MessageToBot(MessageToBotEvent::Private(private)) = event else {
            panic!("expected MessageToBot::Private");
        };
        assert_eq!(private.user_id, 123456789);
    }

    #[test]
    fn test_parse_status_command() {
        let json = serde_json::json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 925712027,
            "user_id": 123456789,
            "message": "/status",
            "sender": {"user_id": 123456789}
        });

        let event = OneBotEvent::from_json(json, 3462039501, &[], "/").unwrap();
        let OneBotEvent::SystemCommand(CommandEvent::Status {
            group_id,
            user_id,
            message_id,
        }) = event
        else {
            panic!("expected Command::Status");
        };
        assert_eq!(group_id, 925712027);
        assert_eq!(user_id, 123456789);
        assert_eq!(message_id, None);
    }

    #[test]
    fn test_parse_help_command() {
        let json = serde_json::json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 925712027,
            "user_id": 123456789,
            "message": "/help",
            "sender": {"user_id": 123456789}
        });

        let event = OneBotEvent::from_json(json, 3462039501, &[], "/").unwrap();
        assert!(matches!(
            event,
            OneBotEvent::SystemCommand(CommandEvent::Help {
                group_id: 925712027,
                user_id: 123456789,
                message_id: None
            })
        ));
    }

    #[test]
    fn test_parse_cancel_command() {
        for command in ["/cancel", "/c"] {
            let json = serde_json::json!({
                "post_type": "message",
                "message_type": "group",
                "group_id": 925712027,
                "user_id": 123456789,
                "message": command,
                "sender": {"user_id": 123456789}
            });

            let event = OneBotEvent::from_json(json, 3462039501, &[], "/").unwrap();
            assert!(
                matches!(
                    event,
                    OneBotEvent::SystemCommand(CommandEvent::Cancel {
                        group_id: 925712027,
                        user_id: 123456789,
                        message_id: None
                    })
                ),
                "expected Cancel for {command}"
            );
        }
    }

    #[test]
    fn test_parse_addressed_command() {
        let json = serde_json::json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 925712027,
            "user_id": 123456789,
            "message": "@3462039501 /status",
            "sender": {"user_id": 123456789}
        });

        let event = OneBotEvent::from_json(json, 3462039501, &[], "/").unwrap();
        let OneBotEvent::SystemCommand(CommandEvent::Status {
            group_id,
            user_id,
            message_id,
        }) = event
        else {
            panic!("expected Command::Status for addressed command");
        };
        assert_eq!(group_id, 925712027);
        assert_eq!(user_id, 123456789);
        assert_eq!(message_id, None);
    }

    #[test]
    fn test_parse_unknown_command() {
        let json = serde_json::json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 925712027,
            "user_id": 123456789,
            "message": "/unknown",
            "sender": {"user_id": 123456789}
        });

        let event = OneBotEvent::from_json(json, 3462039501, &[], "/").unwrap();
        let OneBotEvent::SystemCommand(CommandEvent::Unknown {
            group_id,
            user_id,
            message_id,
            command,
        }) = event
        else {
            panic!("expected Command::Unknown");
        };
        assert_eq!(message_id, None);
        assert_eq!(group_id, 925712027);
        assert_eq!(user_id, 123456789);
        assert_eq!(command, "unknown");
    }

    #[test]
    fn test_parse_meta_event() {
        let json = serde_json::json!({
            "post_type": "meta_event",
            "meta_event_type": "lifecycle",
            "sub_type": "connect"
        });

        let event = OneBotEvent::from_json(json, 0, &[], "/").unwrap();
        let OneBotEvent::Meta(MetaEvent::Lifecycle { sub_type }) = event else {
            panic!("expected lifecycle meta event");
        };
        assert_eq!(sub_type, "connect");
    }

    #[test]
    fn test_parse_unknown_event() {
        let json = serde_json::json!({
            "post_type": "unknown_thing",
            "data": 123
        });

        let event = OneBotEvent::from_json(json, 0, &[], "/").unwrap();
        assert!(matches!(event, OneBotEvent::Unknown(_)));
    }
}
