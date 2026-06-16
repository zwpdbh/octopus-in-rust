use serde::{Deserialize, Serialize};

/// Request sent by the `qqbot` CLI to the running `qqbot-core` control socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlRequest {
    /// List the tools currently loaded in the running core.
    ListTools,
    /// Health-check style no-op.
    Ping,
}

/// Response returned by the `qqbot-core` control socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlResponse {
    /// Tool names currently loaded in the runtime.
    Tools { names: Vec<String> },
    /// Reply to a `Ping` request.
    Pong,
    /// Something went wrong while handling the request.
    Error { message: String },
}
