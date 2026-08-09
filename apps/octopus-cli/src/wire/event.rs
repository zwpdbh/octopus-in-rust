use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::approval_runtime::ApprovalSourceKind;

// ============================================================================
// Core message types (shared between wire protocol and soul)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl Message {
    pub fn extract_text(&self, _sep: &str) -> String {
        self.content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(_sep)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    ImageUrl {
        #[serde(rename = "image_url")]
        image_url: MediaUrl,
    },
    AudioUrl {
        #[serde(rename = "audio_url")]
        audio_url: MediaUrl,
    },
    VideoUrl {
        #[serde(rename = "video_url")]
        video_url: MediaUrl,
    },
    Think {
        think: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: llm_provider::ToolCallType,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

// ============================================================================
// Tool result types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub return_value: ToolReturnValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolReturnValue {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<ToolOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brief: Option<String>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolOutput {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl ToolReturnValue {
    pub fn is_error(&self) -> bool {
        self.is_error
    }

    pub fn ok(output: Option<Vec<ContentPart>>, message: Option<String>) -> Self {
        Self {
            output: output.map(ToolOutput::Parts),
            message,
            brief: None,
            is_error: false,
        }
    }

    pub fn error(message: String, brief: String, output: Option<Vec<ContentPart>>) -> Self {
        Self {
            output: output.map(ToolOutput::Parts),
            message: Some(message),
            brief: Some(brief),
            is_error: true,
        }
    }
}

// ============================================================================
// Wire event types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnBegin {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_input: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnEnd {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepBegin {
    pub n: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepInterrupted {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRetry {
    pub n: usize,
    pub next_attempt: usize,
    pub max_attempts: usize,
    pub wait_s: f64,
    pub error_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatusUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_usage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yolo_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub afk_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_status: Option<MCPStatusSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub input: usize,
    pub output: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPStatusSnapshot {
    pub loading: bool,
    pub connected: usize,
    pub total: usize,
    pub tools: usize,
    pub servers: Vec<MCPServerSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServerSnapshot {
    pub name: String,
    pub status: String,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerInput {
    pub user_input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionBegin {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionEnd {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPLoadingBegin {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPLoadingEnd {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtwBegin {
    pub question: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtwEnd {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextPart {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub category: String,
    #[serde(rename = "type")]
    pub notification_type: String,
    pub source_kind: String,
    pub source_id: String,
    pub title: String,
    pub body: String,
    pub severity: String,
    pub created_at: f64,
    #[serde(default)]
    pub payload: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookTriggered {
    pub event: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub hook_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResolved {
    pub event: String,
    #[serde(default)]
    pub target: String,
    #[serde(default = "default_allow")]
    pub action: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub duration_ms: u64,
}

/// A request sent to the wire client asking it to resolve a client-side hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRequest {
    pub id: String,
    pub subscription_id: String,
    pub event: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub input_data: serde_json::Value,
}

fn default_allow() -> String {
    "allow".to_string()
}

// ============================================================================
// Approval types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestEvent {
    pub id: String,
    pub tool_call_id: String,
    pub sender: String,
    pub action: String,
    pub description: String,
    pub source_kind: ApprovalSourceKind,
    pub source_id: String,
    #[serde(default)]
    pub display: Vec<DisplayBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponseEvent {
    pub request_id: String,
    pub response: String,
    #[serde(default)]
    pub feedback: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayBlock {
    pub title: String,
    pub content: String,
}

// ============================================================================
// Wire event enum (strongly-typed channel payload)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireEvent {
    ContentPart(ContentPart),
    StatusUpdate(StatusUpdate),
    McpLoadingBegin(MCPLoadingBegin),
    McpLoadingEnd(MCPLoadingEnd),
    TextPart(TextPart),
    TurnBegin(TurnBegin),
    TurnEnd(TurnEnd),
    StepBegin(StepBegin),
    StepInterrupted(StepInterrupted),
    StepRetry(StepRetry),
    SteerInput(SteerInput),
    CompactionBegin(CompactionBegin),
    CompactionEnd(CompactionEnd),
    BtwBegin(BtwBegin),
    BtwEnd(BtwEnd),
    Notification(Notification),
    ApprovalRequest(ApprovalRequestEvent),
    ApprovalResponse(ApprovalResponseEvent),
    ToolResult(ToolResult),
    HookTriggered(HookTriggered),
    HookResolved(HookResolved),
}
