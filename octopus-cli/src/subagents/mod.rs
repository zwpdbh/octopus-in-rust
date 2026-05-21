use crate::session::Session;

#[derive(Debug, Clone)]
pub struct LaborMarket;

impl LaborMarket {
    pub fn new() -> Self {
        Self
    }

    pub fn add_builtin_type(&self, _def: AgentTypeDefinition) {}
}

impl Default for LaborMarket {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct SubagentStore;

impl SubagentStore {
    pub fn new(_session: &Session) -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
pub struct AgentTypeDefinition {
    pub name: String,
    pub description: Option<String>,
    pub agent_file: std::path::PathBuf,
    pub when_to_use: Option<String>,
    pub default_model: Option<String>,
    pub tool_policy: ToolPolicy,
}

#[derive(Debug, Clone)]
pub enum ToolPolicy {
    AllowList { tools: Vec<String> },
    Inherit,
}
