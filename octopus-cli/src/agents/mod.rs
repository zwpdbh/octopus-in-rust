use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub name: String,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
}

pub fn load_agent_spec(path: &PathBuf) -> Option<AgentSpec> {
    if !path.exists() {
        return None;
    }
    // TODO: implement agent spec loading from YAML
    Some(AgentSpec {
        name: "default".to_string(),
        system_prompt: "You are a helpful assistant.".to_string(),
        allowed_tools: Vec::new(),
    })
}
