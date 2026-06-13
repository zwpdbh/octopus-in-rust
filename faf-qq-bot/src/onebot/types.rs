use serde::{Deserialize, Serialize};

/// Top-level OneBot 11 event received from the upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "post_type")]
    pub post_type: String,
    #[serde(flatten)]
    pub detail: EventDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventDetail {
    Message {
        #[serde(rename = "message_type")]
        message_type: String,
        #[serde(rename = "sub_type")]
        #[serde(default)]
        sub_type: String,
        #[serde(rename = "group_id")]
        #[serde(default)]
        group_id: Option<i64>,
        #[serde(rename = "user_id")]
        user_id: i64,
        #[serde(rename = "message_id")]
        message_id: i64,
        message: serde_json::Value,
        #[serde(rename = "raw_message")]
        raw_message: String,
        #[serde(default)]
        sender: serde_json::Value,
        time: i64,
    },
    Other(serde_json::Value),
}

impl Event {
    /// Extract a group message if this event represents one.
    pub fn as_group_message(&self) -> Option<GroupMessage> {
        match &self.detail {
            EventDetail::Message {
                message_type,
                group_id: Some(group_id),
                user_id,
                message_id,
                raw_message,
                sender,
                time,
                ..
            } if message_type == "group" => Some(GroupMessage {
                group_id: *group_id,
                user_id: *user_id,
                message_id: *message_id,
                raw_message: raw_message.clone(),
                sender_nickname: sender
                    .get("nickname")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                time: *time,
            }),
            _ => None,
        }
    }
}

/// Normalized group message used inside the bot.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GroupMessage {
    pub group_id: i64,
    pub user_id: i64,
    pub message_id: i64,
    pub raw_message: String,
    pub sender_nickname: String,
    pub time: i64,
}

/// OneBot 11 action sent upstream.
#[derive(Debug, Clone, Serialize)]
pub struct Action {
    pub action: String,
    pub params: serde_json::Value,
    #[serde(rename = "echo")]
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
}
