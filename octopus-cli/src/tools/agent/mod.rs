use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::soul::approval::ApprovalState;
use crate::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentParams {
    pub description: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default = "default_subagent_type")]
    pub subagent_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
    #[serde(default)]
    pub run_in_background: bool,
    #[serde(default)]
    pub timeout: Option<u64>,
}

fn default_subagent_type() -> String {
    "coder".to_string()
}

pub struct AgentTool {
    config: crate::config::Config,
    llm: Option<crate::llm::LLM>,
    approval_state: ApprovalState,
    work_dir: PathBuf,
}

impl AgentTool {
    pub fn new(
        config: crate::config::Config,
        llm: Option<crate::llm::LLM>,
        approval_state: ApprovalState,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            config,
            llm,
            approval_state,
            work_dir,
        }
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Launch a subagent to work on a focused task."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "Agent",
            "description": "Launch a subagent to work on a focused task in the background.",
            "parameters": {
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "Short description of the task (3-5 words)" },
                    "prompt": { "type": "string", "description": "Complete prompt for the subagent" },
                    "subagent_type": { "type": "string", "default": "coder", "description": "Built-in agent type" },
                    "model": { "type": "string", "description": "Optional model override" },
                    "resume": { "type": "string", "description": "Optional agent ID to resume" },
                    "run_in_background": { "type": "boolean", "default": false, "description": "Run in background" },
                    "timeout": { "type": "integer", "description": "Timeout in seconds" }
                },
                "required": ["description"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: AgentParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        if params.run_in_background {
            // For background subagents, spawn a task and return immediately
            let config = self.config.clone();
            let llm = self.llm.clone();
            let approval_state = self.approval_state.clone();
            let work_dir = self.work_dir.clone();
            let description = params.description.clone();
            let prompt = params.prompt.clone();

            tokio::spawn(async move {
                let result = run_subagent(config, llm, approval_state, work_dir, &prompt).await;
                match result {
                    Ok(response) => {
                        tracing::info!("Background subagent '{}' completed", description);
                        // TODO: send notification to parent
                        tracing::info!("Subagent result: {}", response);
                    }
                    Err(e) => {
                        tracing::error!("Background subagent '{}' failed: {}", description, e);
                    }
                }
            });

            return Ok(format!(
                "Subagent '{}' launched in the background.\nautomatic_notification: true\nnext_step: You will be notified when it completes.",
                params.description
            ));
        }

        // Foreground subagent
        let result = run_subagent(
            self.config.clone(),
            self.llm.clone(),
            self.approval_state.clone(),
            self.work_dir.clone(),
            &params.prompt,
        )
        .await?;

        Ok(result)
    }
}

async fn run_subagent(
    config: crate::config::Config,
    llm: Option<crate::llm::LLM>,
    approval_state: ApprovalState,
    work_dir: PathBuf,
    prompt: &str,
) -> Result<String, String> {
    let session = crate::session::Session::create(&work_dir, None)
        .await
        .map_err(|e| format!("Failed to create subagent session: {}", e))?;

    let mut subagent = crate::soul::KimiSoul::new(config, session, llm, approval_state);

    let result = subagent.run(prompt).await;

    match result {
        Ok(response) => Ok(response),
        Err(e) => Err(format!("Subagent failed: {}", e)),
    }
}
