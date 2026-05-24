use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::oneshot;

// ============================================================================
// JSON-RPC types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

// ============================================================================
// MCP schema types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolSchema {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "inputSchema", skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { resource: Value },
}

impl CallToolResult {
    /// Convert tool result content to a single string for LLM consumption.
    pub fn to_text(&self) -> String {
        let mut parts = Vec::new();
        for item in &self.content {
            match item {
                ToolContent::Text { text } => parts.push(text.clone()),
                ToolContent::Image { .. } => parts.push("[Image content]".to_string()),
                ToolContent::Resource { resource } => {
                    parts.push(format!("[Resource: {}]", resource));
                }
            }
        }
        parts.join("\n")
    }
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("RPC error {code}: {message}")]
    Rpc { code: i32, message: String },
    #[error("Server closed connection")]
    ConnectionClosed,
    #[error("Request cancelled")]
    Cancelled,
    #[error("Other: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, McpClientError>;

// ============================================================================
// Client
// ============================================================================

#[derive(Clone)]
pub struct McpClient {
    inner: Arc<McpClientInner>,
}

impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient").finish_non_exhaustive()
    }
}

struct McpClientInner {
    stdin: Arc<tokio::sync::Mutex<BufWriter<ChildStdin>>>,
    request_counter: AtomicU64,
    pending: Arc<DashMap<u64, oneshot::Sender<JsonRpcResponse>>>,
    _child: tokio::sync::Mutex<Child>,
}

impl McpClient {
    /// Connect to an MCP server via stdio transport.
    pub async fn connect_stdio(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.envs(env);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // Spawn stderr logger
        tokio::spawn(log_stderr(stderr));

        let stdin = Arc::new(tokio::sync::Mutex::new(BufWriter::new(stdin)));
        let pending: Arc<DashMap<u64, oneshot::Sender<JsonRpcResponse>>> = Arc::new(DashMap::new());

        // Spawn stdout reader task
        let pending_clone = pending.clone();
        tokio::spawn(read_stdout(stdout, pending_clone));

        let client = Self {
            inner: Arc::new(McpClientInner {
                stdin,
                request_counter: AtomicU64::new(1),
                pending,
                _child: tokio::sync::Mutex::new(child),
            }),
        };

        // Perform MCP initialization handshake
        client.initialize().await?;

        Ok(client)
    }

    /// List tools available on the server.
    pub async fn list_tools(&self) -> Result<Vec<McpToolSchema>> {
        let result = self.send_request("tools/list", None).await?;
        let tools = result
            .get("tools")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![]));
        let tools: Vec<McpToolSchema> = serde_json::from_value(tools)?;
        Ok(tools)
    }

    /// Call a tool on the server.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<CallToolResult> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });
        let result = self.send_request("tools/call", Some(params)).await?;
        let call_result: CallToolResult = serde_json::from_value(result)?;
        Ok(call_result)
    }

    /// Shut down the client and kill the server process.
    pub async fn shutdown(&self) -> Result<()> {
        let mut child = self.inner._child.lock().await;
        let _ = child.kill().await;
        let _ = child.wait().await;
        Ok(())
    }

    // ------------------------------------------------------------------------
    // Private
    // ------------------------------------------------------------------------

    async fn initialize(&self) -> Result<()> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "octopus-cli",
                "version": env!("CARGO_PKG_VERSION"),
            }
        });

        let _result = self.send_request("initialize", Some(params)).await?;

        self.send_notification("notifications/initialized", None)
            .await?;

        Ok(())
    }

    async fn send_request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.inner.request_counter.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let (tx, rx) = oneshot::channel();
        self.inner.pending.insert(id, tx);

        let line = serde_json::to_string(&request)? + "\n";
        {
            let mut stdin = self.inner.stdin.lock().await;
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await?;
        }

        let response = rx.await.map_err(|_| McpClientError::Cancelled)?;

        if let Some(error) = response.error {
            return Err(McpClientError::Rpc {
                code: error.code,
                message: error.message,
            });
        }

        response
            .result
            .ok_or_else(|| McpClientError::Other("Empty response".to_string()))
    }

    async fn send_notification(&self, method: &str, params: Option<Value>) -> Result<()> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        };
        let line = serde_json::to_string(&notification)? + "\n";
        let mut stdin = self.inner.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }
}

// ============================================================================
// Background tasks
// ============================================================================

async fn read_stdout(
    stdout: ChildStdout,
    pending: Arc<DashMap<u64, oneshot::Sender<JsonRpcResponse>>>,
) {
    let mut reader = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<JsonRpcResponse>(&line) {
            Ok(response) => {
                if let Some((_, sender)) = pending.remove(&response.id) {
                    let _ = sender.send(response);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to parse MCP response: {}", e);
            }
        }
    }
    // Connection closed — dropping all pending senders causes
    // receivers to get Cancelled.
    pending.clear();
}

async fn log_stderr(stderr: tokio::process::ChildStderr) {
    let mut reader = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        tracing::debug!(target: "mcp_server_stderr", "{}", line);
    }
}
