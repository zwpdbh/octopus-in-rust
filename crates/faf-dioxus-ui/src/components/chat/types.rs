use serde::{Deserialize, Serialize};

/// A single item in a chat history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChatMessageItem {
    /// Message sent by the user.
    User { content: String },
    /// Message produced by the assistant. `is_streaming` is true while the
    /// response is still being generated.
    Assistant {
        content: String,
        #[serde(default)]
        thinking: String,
        is_streaming: bool,
        #[serde(default)]
        tool_calls: Vec<ToolCall>,
    },
}

/// A tool invocation recorded inside an assistant message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub result: Option<String>,
    #[serde(default)]
    pub is_error: bool,
}

/// A summary of a chat session shown in the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatHistoryItem {
    pub id: String,
    pub title: String,
}
