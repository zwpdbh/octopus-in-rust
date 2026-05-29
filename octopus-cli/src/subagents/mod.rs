use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::session::Session;

/// Registry of built-in subagent types that can be spawned by the `Agent` tool.
///
/// Each type definition maps a name (e.g. `"researcher"`) to its agent spec
/// file, default model, tool policy, etc.
#[derive(Debug, Clone)]
pub struct LaborMarket {
    types: Arc<Mutex<HashMap<SubagentType, AgentTypeDefinition>>>,
}

impl LaborMarket {
    pub fn new() -> Self {
        Self {
            types: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a built-in subagent type.
    pub fn add_builtin_type(&self, def: AgentTypeDefinition) {
        let mut types = self.types.lock().unwrap();
        tracing::info!("Registered subagent type: {}", def.name);
        types.insert(def.name.clone(), def);
    }

    /// Look up a subagent type by name.
    pub fn get_builtin_type(&self, name: &SubagentType) -> Option<AgentTypeDefinition> {
        self.types.lock().unwrap().get(name).cloned()
    }

    /// Look up a subagent type, returning an error if not found.
    pub fn require_builtin_type(&self, name: &SubagentType) -> Result<AgentTypeDefinition, String> {
        self.get_builtin_type(name)
            .ok_or_else(|| format!("Builtin subagent type not found: {}", name))
    }

    /// List all registered subagent type names.
    pub fn list_builtin_types(&self) -> Vec<SubagentType> {
        self.types.lock().unwrap().keys().cloned().collect()
    }
}

impl Default for LaborMarket {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct SubagentStore {
    entries: Arc<Mutex<HashMap<String, SubagentEntry>>>,
}

impl SubagentStore {
    pub fn new(_session: &Session) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a new background subagent as running.
    pub fn register(&self, id: String, description: String, subagent_type: SubagentType) {
        let entry = SubagentEntry {
            id: id.clone(),
            description,
            subagent_type,
            status: SubagentStatus::Running,
            result: None,
            created_at: chrono::Utc::now(),
        };
        self.entries.lock().unwrap().insert(id, entry);
    }

    /// Mark a subagent as completed with its result.
    pub fn complete(&self, id: &str, result: String) {
        if let Some(entry) = self.entries.lock().unwrap().get_mut(id) {
            entry.status = SubagentStatus::Completed;
            entry.result = Some(result);
        }
    }

    /// Mark a subagent as failed with an error message.
    pub fn fail(&self, id: &str, error: String) {
        if let Some(entry) = self.entries.lock().unwrap().get_mut(id) {
            entry.status = SubagentStatus::Failed;
            entry.result = Some(error);
        }
    }

    /// Get a subagent entry by ID.
    pub fn get(&self, id: &str) -> Option<SubagentEntry> {
        self.entries.lock().unwrap().get(id).cloned()
    }

    /// List all tracked subagent entries.
    pub fn list(&self) -> Vec<SubagentEntry> {
        self.entries.lock().unwrap().values().cloned().collect()
    }

    /// Remove a subagent entry.
    pub fn remove(&self, id: &str) {
        self.entries.lock().unwrap().remove(id);
    }
}

#[derive(Debug, Clone)]
pub struct SubagentEntry {
    pub id: String,
    pub description: String,
    pub subagent_type: SubagentType,
    pub status: SubagentStatus,
    pub result: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Well-known built-in subagent types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KnownSubagentType {
    Coder,
    Explore,
    Plan,
}

impl KnownSubagentType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Coder => "coder",
            Self::Explore => "explore",
            Self::Plan => "plan",
        }
    }
}

impl std::fmt::Display for KnownSubagentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Subagent type used by the [`Agent`](crate::tools::agent::AgentTool) tool.
///
/// Supports the well-known built-in variants ([`KnownSubagentType`]) as well as
/// arbitrary custom types registered in the [`LaborMarket`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SubagentType {
    Known(KnownSubagentType),
    Other(String),
}

impl SubagentType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Known(k) => k.as_str(),
            Self::Other(s) => s.as_str(),
        }
    }
}

impl Default for SubagentType {
    fn default() -> Self {
        Self::Known(KnownSubagentType::Coder)
    }
}

impl std::fmt::Display for SubagentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<KnownSubagentType> for SubagentType {
    fn from(value: KnownSubagentType) -> Self {
        Self::Known(value)
    }
}

impl From<String> for SubagentType {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl From<&str> for SubagentType {
    fn from(value: &str) -> Self {
        match value {
            "coder" => Self::Known(KnownSubagentType::Coder),
            "explore" => Self::Known(KnownSubagentType::Explore),
            "plan" => Self::Known(KnownSubagentType::Plan),
            _ => Self::Other(value.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentStatus {
    Running,
    Completed,
    Failed,
}

/// Definition of a built-in subagent type loaded from an agent spec.
#[derive(Debug, Clone)]
pub struct AgentTypeDefinition {
    pub name: SubagentType,
    pub description: Option<String>,
    pub agent_file: PathBuf,
    pub when_to_use: Option<String>,
    pub default_model: Option<String>,
    pub tool_policy: ToolPolicy,
}

use crate::tools::tool_name::ToolName;

/// Determines which tools a subagent of this type may use.
#[derive(Debug, Clone)]
pub enum ToolPolicy {
    /// Only these specific tools are allowed.
    AllowList { tools: Vec<ToolName> },
    /// Inherit the parent's tool policy (all parent tools).
    Inherit,
}
