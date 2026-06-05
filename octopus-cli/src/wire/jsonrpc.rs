use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// Wire protocol version
// ============================================================================

pub const WIRE_PROTOCOL_VERSION: &str = "1.0";

// ============================================================================
// Base JSON-RPC envelope (used for initial routing)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCMessage {
    pub jsonrpc: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub params: Option<Value>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JSONRPCErrorObject>,
}

impl JSONRPCMessage {
    pub fn is_response(&self) -> bool {
        self.method.is_none() && self.id.is_some()
    }

    pub fn is_request(&self) -> bool {
        self.method.is_some() && self.id.is_some()
    }

    pub fn is_notification(&self) -> bool {
        self.method.is_some() && self.id.is_none()
    }
}

// ============================================================================
// Error object
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JSONRPCErrorObject {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

// ============================================================================
// Outbound responses
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCSuccessResponse {
    pub jsonrpc: String,
    pub id: String,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCErrorResponse {
    pub jsonrpc: String,
    pub id: String,
    pub error: JSONRPCErrorObject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCErrorResponseNullableID {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub error: JSONRPCErrorObject,
}

// ============================================================================
// Inbound request params
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    #[serde(default)]
    pub supports_question: bool,
    #[serde(default)]
    pub supports_plan_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireHookSubscription {
    pub id: String,
    pub event: String,
    #[serde(default)]
    pub matcher: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    pub protocol_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<ClientInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_tools: Option<Vec<ExternalTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Vec<WireHookSubscription>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ClientCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptParams {
    pub user_input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerParams {
    pub user_input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetPlanModeParams {
    pub enabled: bool,
}

// ============================================================================
// Inbound request messages
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCInitializeMessage {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: InitializeParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCPromptMessage {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: PromptParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCSteerMessage {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: SteerParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCReplayMessage {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCSetPlanModeMessage {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: SetPlanModeParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCCancelMessage {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// Any inbound request (not a response).
#[derive(Debug, Clone)]
pub enum JSONRPCInbound {
    Initialize(JSONRPCInitializeMessage),
    Prompt(JSONRPCPromptMessage),
    Steer(JSONRPCSteerMessage),
    Replay(JSONRPCReplayMessage),
    SetPlanMode(JSONRPCSetPlanModeMessage),
    Cancel(JSONRPCCancelMessage),
}

/// Any response from the client.
#[derive(Debug, Clone)]
pub enum JSONRPCClientResponse {
    Success(JSONRPCSuccessResponse),
    Error(JSONRPCErrorResponse),
}

// ============================================================================
// Outbound messages
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct JSONRPCEventMessage<T: Serialize> {
    pub jsonrpc: String,
    pub method: String,
    pub params: T,
}

impl<T: Serialize> JSONRPCEventMessage<T> {
    pub fn new(params: T) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: "event".to_string(),
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JSONRPCRequestMessage<T: Serialize> {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: T,
}

impl<T: Serialize> JSONRPCRequestMessage<T> {
    pub fn new(id: String, params: T) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: "request".to_string(),
            params,
        }
    }
}

// ============================================================================
// Error codes
// ============================================================================

pub struct ErrorCodes;

#[allow(dead_code)]
impl ErrorCodes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    pub const INVALID_STATE: i32 = -32000;
    pub const LLM_NOT_SET: i32 = -32001;
    pub const LLM_NOT_SUPPORTED: i32 = -32002;
    pub const CHAT_PROVIDER_ERROR: i32 = -32003;
    pub const AUTH_EXPIRED: i32 = -32004;
}

// ============================================================================
// Status constants
// ============================================================================

pub struct Statuses;

#[allow(dead_code)]
impl Statuses {
    pub const FINISHED: &str = "finished";
    pub const CANCELLED: &str = "cancelled";
    pub const MAX_STEPS_REACHED: &str = "max_steps_reached";
    pub const STEERED: &str = "steered";
}

// ============================================================================
// Hook response types (client → server)
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct HookResponse {
    pub request_id: String,
    #[serde(default = "default_allow_action")]
    pub action: String,
    #[serde(default)]
    pub reason: String,
}

fn default_allow_action() -> String {
    "allow".to_string()
}

// ============================================================================
// Approval response types (client → server)
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalResponseBody {
    pub request_id: String,
    pub response: String,
    #[serde(default)]
    pub feedback: String,
}
