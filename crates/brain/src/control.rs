use serde::{Deserialize, Serialize};

/// Request sent by the `qqbot` CLI to the running `qqbot-core` control socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlRequest {
    /// List the tools currently loaded in the running core.
    ListTools,
    /// Per-group runtime status (brain loaded, tools, etc.).
    GroupStatus,
    /// Health-check style no-op.
    Ping,
}

/// Per-group runtime status reported by `qqbot-core`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupRuntimeStatus {
    pub group_id: i64,
    /// True if a Brain has been created for this group.
    pub brain_ready: bool,
    /// Number of tools loaded for this group.
    pub tool_count: usize,
    /// Names of tools loaded for this group.
    pub tools: Vec<String>,
}

/// Response returned by the `qqbot-core` control socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlResponse {
    /// Tool names currently loaded in the runtime.
    Tools { names: Vec<String> },
    /// Per-group runtime status.
    Groups { groups: Vec<GroupRuntimeStatus> },
    /// Reply to a `Ping` request.
    Pong,
    /// Something went wrong while handling the request.
    Error { message: String },
}
