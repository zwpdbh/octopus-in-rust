use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::approval_runtime::{ApprovalSource, with_approval_source};
use crate::soul::agent::{Agent, load_agent};
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
    parent_runtime: crate::soul::agent::AppRuntime,
}

impl AgentTool {
    pub fn new(parent_runtime: crate::soul::agent::AppRuntime) -> Self {
        Self { parent_runtime }
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

        let parent_runtime = self.parent_runtime.clone();

        if params.run_in_background {
            let description = params.description.clone();
            let prompt = params.prompt.clone();
            let subagent_type = params.subagent_type.clone();
            let model_override = params.model.clone();
            tokio::spawn(async move {
                let result = run_subagent_with_market(
                    parent_runtime,
                    &subagent_type,
                    &description,
                    &prompt,
                    model_override.as_deref(),
                )
                .await;
                match result {
                    Ok(response) => {
                        tracing::info!("Background subagent '{}' completed", description);
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
        let result = run_subagent_with_market(
            parent_runtime,
            &params.subagent_type,
            &params.description,
            &params.prompt,
            params.model.as_deref(),
        )
        .await?;

        Ok(result)
    }
}

async fn run_subagent_with_market(
    parent_runtime: crate::soul::agent::AppRuntime,
    subagent_type: &str,
    description: &str,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<String, String> {
    let config = parent_runtime.config.clone();
    let llm = parent_runtime.llm.clone();
    let approval_state = parent_runtime.approval.state();
    let work_dir = parent_runtime.session.work_dir.clone();
    let subagent_store = parent_runtime.subagent_store.clone();

    // Look up the subagent type in the LaborMarket
    let type_def = parent_runtime.labor_market.get_builtin_type(subagent_type);

    match type_def {
        Some(def) => {
            // Registered subagent type: load from its agent spec
            let session = crate::session::Session::create(&work_dir, None)
                .await
                .map_err(|e| format!("Failed to create subagent session: {}", e))?;

            // Register in SubagentStore if available
            if let Some(ref store) = subagent_store {
                store.register(
                    session.id.clone(),
                    description.to_string(),
                    subagent_type.to_string(),
                );
            }

            let builtin_args = crate::soul::agent::BuiltinSystemPromptArgs {
                kimi_now: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                kimi_work_dir: session.work_dir.clone(),
                kimi_work_dir_ls: String::new(),
                kimi_agents_md: String::new(),
                kimi_skills: String::new(),
                kimi_additional_dirs_info: String::new(),
                kimi_os: std::env::consts::OS.to_string(),
                kimi_shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            };

            let approval = crate::soul::approval::Approval::with_state(approval_state.clone());
            let subagent_runtime = parent_runtime.copy_for_subagent(
                session.clone(),
                llm.clone(),
                approval,
                builtin_args,
                Some(def.name.clone()),
            );

            let agent = load_agent(&def.agent_file, subagent_runtime, vec![])
                .await
                .map_err(|e| format!("Failed to load subagent spec '{}': {}", def.name, e))?;

            // Resolve the subagent's LLM: user override > type default > inherit parent
            let model_alias = model_override.or(def.default_model.as_deref());
            let subagent_llm =
                crate::llm::clone_llm_with_model_alias(llm.as_ref(), &config, model_alias)
                    .map_err(|e| format!("Failed to resolve subagent model: {}", e))?;

            let mut subagent = crate::soul::KimiSoul::new(
                config,
                session.clone(),
                subagent_llm,
                approval_state,
                agent,
                Some(def.tool_policy.clone()),
            );

            // Set the subagent as the current approval source so nested tool calls
            // and rejection messages know they're inside a subagent.
            let subagent_source = ApprovalSource {
                kind: "foreground_turn".to_string(),
                id: session.id.clone(),
                agent_id: Some(session.id.clone()),
            };
            let result: Result<String, crate::exception::OctopusError> =
                with_approval_source(subagent_source.clone(), subagent.run(prompt)).await;

            // Cancel any pending approval requests belonging to this subagent.
            subagent
                .approval
                .runtime()
                .cancel_by_source(&subagent_source.kind, &subagent_source.id);

            // Update SubagentStore
            if let Some(ref store) = subagent_store {
                match &result {
                    Ok(response) => store.complete(&session.id, response.clone()),
                    Err(e) => store.fail(&session.id, e.to_string()),
                }
            }

            match result {
                Ok(response) => Ok(response),
                Err(e) => Err(format!("Subagent failed: {}", e)),
            }
        }
        None => {
            // Fallback: unregistered type, create a basic subagent
            tracing::warn!(
                "Subagent type '{}' not found in LaborMarket, using basic fallback",
                subagent_type
            );
            run_subagent_basic(
                config,
                llm,
                approval_state,
                work_dir,
                subagent_store,
                description,
                prompt,
            )
            .await
        }
    }
}

async fn run_subagent_basic(
    config: crate::config::Config,
    llm: Option<crate::llm::LLM>,
    approval_state: ApprovalState,
    work_dir: PathBuf,
    subagent_store: Option<crate::subagents::SubagentStore>,
    description: &str,
    prompt: &str,
) -> Result<String, String> {
    let session = crate::session::Session::create(&work_dir, None)
        .await
        .map_err(|e| format!("Failed to create subagent session: {}", e))?;

    if let Some(ref store) = subagent_store {
        store.register(
            session.id.clone(),
            description.to_string(),
            "basic".to_string(),
        );
    }

    let agent = Agent::new_basic(
        "subagent".to_string(),
        "You are a helpful assistant.".to_string(),
        config.clone(),
        session.clone(),
        llm.clone(),
        approval_state.clone(),
        vec![], // Subagents don't inherit MCP configs
    );
    let mut subagent =
        crate::soul::KimiSoul::new(config, session.clone(), llm, approval_state, agent, None);

    let result = subagent.run(prompt).await;

    if let Some(ref store) = subagent_store {
        match &result {
            Ok(response) => store.complete(&session.id, response.clone()),
            Err(e) => store.fail(&session.id, e.to_string()),
        }
    }

    match result {
        Ok(response) => Ok(response),
        Err(e) => Err(format!("Subagent failed: {}", e)),
    }
}
