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
}

impl Event {
    pub fn text_message(&self) -> String {
        if !self.message.is_empty() {
            return self.message.clone();
        }
        self.raw_message.clone().unwrap_or_default()
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
        let message_text = match raw.message {
            Some(serde_json::Value::String(s)) => s,
            Some(serde_json::Value::Array(parts)) => {
                let mut out = String::new();
                for part in parts {
                    if let Some(text) = part
                        .get("data")
                        .and_then(|d| d.get("text"))
                        .and_then(|t| t.as_str())
                    {
                        out.push_str(text);
                    }
                }
                out
            }
            _ => String::new(),
        };

        Ok(Event {
            post_type: raw.post_type,
            message_type: raw.message_type,
            group_id: raw.group_id,
            user_id: raw.user_id,
            message: message_text,
            raw_message: raw.raw_message,
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
}
