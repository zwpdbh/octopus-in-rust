use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    #[serde(rename = "post_type")]
    pub post_type: String,
    #[serde(rename = "message_type")]
    pub message_type: Option<String>,
    #[serde(rename = "group_id")]
    pub group_id: Option<i64>,
    #[serde(rename = "user_id")]
    pub user_id: Option<i64>,
    pub message: String,
    #[serde(rename = "raw_message")]
    pub raw_message: Option<String>,
    #[serde(skip)]
    pub message_segments: Vec<MessageSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSegment {
    #[serde(rename = "type")]
    pub seg_type: String,
    pub data: serde_json::Map<String, serde_json::Value>,
}

impl Event {
    pub fn text_message(&self) -> String {
        if !self.message.is_empty() {
            return self.message.clone();
        }
        self.raw_message.clone().unwrap_or_default()
    }

    /// Whether the bot was explicitly @-mentioned in this message.
    pub fn is_at(&self, bot_qq: i64) -> bool {
        self.message_segments.iter().any(|seg| {
            seg.seg_type == "at"
                && seg
                    .data
                    .get("qq")
                    .and_then(|v| match v {
                        serde_json::Value::String(s) => s.parse::<i64>().ok(),
                        serde_json::Value::Number(n) => n.as_i64(),
                        _ => None,
                    })
                    == Some(bot_qq)
        })
    }

    /// Text content with @ segments mentioning the bot removed.
    pub fn prompt_text(&self, bot_qq: i64) -> String {
        self.message_segments
            .iter()
            .filter_map(|seg| {
                if seg.seg_type == "text" {
                    seg.data
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else if seg.seg_type == "at" {
                    // Drop @ segments that target the bot; keep other @s.
                    let is_bot = seg
                        .data
                        .get("qq")
                        .and_then(|v| match v {
                            serde_json::Value::String(s) => s.parse::<i64>().ok(),
                            serde_json::Value::Number(n) => n.as_i64(),
                            _ => None,
                        })
                        == Some(bot_qq);
                    if is_bot {
                        Some(String::new())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<String>()
            .trim()
            .to_string()
    }
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawEvent {
            #[serde(rename = "post_type")]
            post_type: String,
            #[serde(rename = "message_type")]
            message_type: Option<String>,
            #[serde(rename = "group_id")]
            group_id: Option<i64>,
            #[serde(rename = "user_id")]
            user_id: Option<i64>,
            message: Option<serde_json::Value>,
            #[serde(rename = "raw_message")]
            raw_message: Option<String>,
        }

        let raw = RawEvent::deserialize(deserializer)?;
        let (message_text, segments) = match raw.message {
            Some(serde_json::Value::String(s)) => (s, Vec::new()),
            Some(serde_json::Value::Array(parts)) => {
                let mut out = String::new();
                let mut segs = Vec::with_capacity(parts.len());
                for part in parts {
                    if let Ok(seg) = serde_json::from_value::<MessageSegment>(part.clone()) {
                        if seg.seg_type == "text" {
                            if let Some(text) = seg.data.get("text").and_then(|t| t.as_str()) {
                                out.push_str(text);
                            }
                        }
                        segs.push(seg);
                    }
                }
                (out, segs)
            }
            _ => (String::new(), Vec::new()),
        };

        Ok(Event {
            post_type: raw.post_type,
            message_type: raw.message_type,
            group_id: raw.group_id,
            user_id: raw.user_id,
            message: message_text,
            raw_message: raw.raw_message,
            message_segments: segments,
        })
    }
}

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
        Self {
            action: "send_group_msg".to_string(),
            params: serde_json::json!({
                "group_id": group_id,
                "message": [
                    {"type": "at", "data": {"qq": user_id.to_string()}},
                    {"type": "text", "data": {"text": format!(" {}", text.into())}},
                ],
            }),
            echo,
        }
    }
}
