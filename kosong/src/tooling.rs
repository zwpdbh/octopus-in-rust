use crate::message::ToolCall;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;

/// A tool definition.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Display block for UI (placeholder; octopus-cli does not use this yet).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DisplayBlock {
    Text { text: String },
    Brief { text: String },
}

/// The return value of a tool execution.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolReturnValue {
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub display: Vec<DisplayBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extras: Option<HashMap<String, Value>>,
}

impl ToolReturnValue {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            is_error: false,
            output: Some(Value::String(output.into())),
            message: None,
            display: Vec::new(),
            extras: None,
        }
    }

    pub fn ok_parts(output: Vec<crate::message::ContentPart>) -> Self {
        Self {
            is_error: false,
            output: Some(serde_json::to_value(output).unwrap_or(Value::Null)),
            message: None,
            display: Vec::new(),
            extras: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        let msg = message.into();
        Self {
            is_error: true,
            output: None,
            message: Some(msg.clone()),
            display: vec![DisplayBlock::Brief { text: msg }],
            extras: None,
        }
    }

    pub fn brief(&self) -> Option<&str> {
        self.display.iter().find_map(|b| match b {
            DisplayBlock::Brief { text } => Some(text.as_str()),
            _ => None,
        })
    }
}

/// The result of handling a tool call.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub return_value: ToolReturnValue,
}

/// Result of `Toolset::handle`: either immediate or pending.
pub enum HandleResult {
    Ready(ToolResult),
    Pending(tokio::task::JoinHandle<ToolResult>),
}

/// A collection of tools that can handle tool calls.
#[async_trait]
pub trait Toolset: Send + Sync {
    fn tools(&self) -> Vec<Tool>;
    fn handle(&self, tool_call: &ToolCall) -> HandleResult;
}

/// A tool that can be called with raw JSON arguments.
#[async_trait]
pub trait CallableTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    async fn call_raw(&self, arguments: Value) -> ToolReturnValue;
}

/// A type-safe callable tool using a Pydantic-like params model.
#[async_trait]
pub trait CallableTool2: Send + Sync {
    type Params: DeserializeOwned + schemars::JsonSchema + Send;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn call_typed(&self, params: Self::Params) -> ToolReturnValue;
}

/// Convert a `CallableTool2` into a `CallableTool` wrapper.
pub struct CallableTool2Adapter<T: CallableTool2> {
    pub inner: T,
}

#[async_trait]
impl<T: CallableTool2> CallableTool for CallableTool2Adapter<T> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        let schema = schemars::schema_for!(T::Params);
        serde_json::to_value(schema).unwrap_or(Value::Null)
    }

    async fn call_raw(&self, arguments: Value) -> ToolReturnValue {
        match serde_json::from_value::<T::Params>(arguments) {
            Ok(params) => self.inner.call_typed(params).await,
            Err(e) => ToolReturnValue::error(format!("JSON parse error: {e}")),
        }
    }
}
