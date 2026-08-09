//! FAF Q&A module backed by `agent-core` and the `faf-units` plugin.

use std::{collections::HashSet, path::PathBuf, sync::Arc};

use agent_core::{
    Brain, BrainBuilder, BrainConfig, BrainError, BrainEvent, ExtismPluginSource,
    ToolAwareSystemPromptPolicy,
};
use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::{config::workspace_root, error::AppError, llm_factory::ProviderType, state::AppState};

/// Configuration for the Q&A agent, loaded from environment variables.
#[derive(Clone, Debug)]
pub struct QaConfig {
    /// LLM backend type and provider-specific data.
    pub provider_type: ProviderType,

    /// LLM base URL.
    pub base_url: String,

    /// API key for standard OpenAI-compatible providers.
    pub api_key: String,

    /// Model name, e.g. `gpt-4o` or `kimi-for-coding`.
    pub model: String,

    /// Directory containing `.wasm` plugins.
    pub plugins_dir: PathBuf,

    /// System prompt for the agent.
    pub system_prompt: String,

    /// Maximum reasoning steps per turn.
    pub max_steps_per_turn: usize,
}

impl QaConfig {
    /// Load Q&A configuration from environment variables.
    ///
    /// Variables:
    /// - `FAFCN_LLM_PROVIDER_TYPE` — `openai_compatible` (default) or `kimi_code`.
    /// - `FAFCN_LLM_BASE_URL` (default: `https://api.openai.com/v1`).
    /// - `FAFCN_LLM_API_KEY` — required for `openai_compatible`.
    /// - `FAFCN_LLM_MODEL` (default: `gpt-4o`).
    /// - `FAFCN_LLM_TOKEN_FILE` — required for `kimi_code`.
    /// - `FAFCN_PLUGINS_DIR` (default: `data/qqbot-data/plugins`).
    /// - `FAFCN_QA_SYSTEM_PROMPT` (optional).
    /// - `FAFCN_QA_MAX_STEPS` (default: `16`).
    pub fn from_env() -> anyhow::Result<Self> {
        let root = workspace_root();
        let default_plugins = root.join("data/qqbot-data/plugins");
        let default_token_file = std::env::var("HOME")
            .map(|home| PathBuf::from(home).join(".kimi/credentials/kimi-code.json"))
            .unwrap_or_else(|_| PathBuf::from(".kimi/credentials/kimi-code.json"));

        let provider_type = ProviderType::parse(
            &crate::env::var_or("FAFCN_LLM_PROVIDER_TYPE", "openai_compatible"),
            crate::env::path_or("FAFCN_LLM_TOKEN_FILE", default_token_file),
        );

        let model = crate::env::var_or("FAFCN_LLM_MODEL", "gpt-4o");
        let base_url = match &provider_type {
            ProviderType::KimiCode { .. } => {
                crate::env::var_or("FAFCN_LLM_BASE_URL", "https://api.kimi.com/coding/v1")
            }
            ProviderType::OpenAiCompatible => {
                crate::env::var_or("FAFCN_LLM_BASE_URL", "https://api.openai.com/v1")
            }
        };

        Ok(Self {
            provider_type,
            base_url,
            api_key: crate::env::var_or("FAFCN_LLM_API_KEY", ""),
            model,
            plugins_dir: crate::env::path_or("FAFCN_PLUGINS_DIR", default_plugins),
            system_prompt: crate::env::var_or(
                "FAFCN_QA_SYSTEM_PROMPT",
                "You are an expert assistant for the game Forged Alliance Forever. \
                 Answer questions about units, buildings, and economy using the tools available.",
            ),
            max_steps_per_turn: crate::env::var_or("FAFCN_QA_MAX_STEPS", "16").parse()?,
        })
    }
}

/// Build a `Brain` that loads only the `faf_units_plugin`.
pub async fn create_brain(config: &QaConfig) -> Result<Brain, BrainError> {
    let allowed: HashSet<String> = ["faf_units_plugin"].into_iter().map(String::from).collect();
    let tool_source = Arc::new(ExtismPluginSource::with_filter(
        &config.plugins_dir,
        allowed,
    ));

    let brain_config = BrainConfig {
        system_prompt: config.system_prompt.clone(),
        base_url: config.base_url.clone(),
        api_key: config.api_key.clone(),
        model: config.model.clone(),
        max_steps_per_turn: config.max_steps_per_turn,
        tool_sources: vec![tool_source],
        ..Default::default()
    };

    let factory = Arc::new(crate::llm_factory::FafcnProviderFactory::new(
        config.provider_type.clone(),
    ));

    BrainBuilder::default()
        .from_config(brain_config)
        .with_provider_factory(factory)
        .with_system_prompt_policy(Arc::new(ToolAwareSystemPromptPolicy))
        .build()
        .await
}

/// Incoming ask request.
#[derive(Debug, Deserialize)]
pub struct AskRequest {
    pub question: String,
}

/// Event emitted during a Q&A turn.
#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
pub enum QaEvent {
    ToolCall {
        name: String,
        arguments: serde_json::Value,
    },
    ToolResult {
        output: String,
        is_error: bool,
    },
}

/// Response returned by `POST /api/ask`.
#[derive(Debug, Serialize)]
pub struct QaResponse {
    pub answer: String,
    pub events: Vec<QaEvent>,
}

/// Run a single question through the agent and collect the answer.
pub async fn ask(config: &QaConfig, question: &str) -> Result<QaResponse, BrainError> {
    let mut brain = create_brain(config).await?;
    let result = brain
        .run_turn_to_completion(question.into())
        .await
        .map_err(|e| BrainError::Other(e.to_string()))?;

    let events = result
        .events
        .into_iter()
        .filter_map(|ev| match ev {
            BrainEvent::ToolCall {
                name, arguments, ..
            } => Some(QaEvent::ToolCall { name, arguments }),
            BrainEvent::ToolResult {
                output, is_error, ..
            } => Some(QaEvent::ToolResult { output, is_error }),
            _ => None,
        })
        .collect();

    Ok(QaResponse {
        answer: result.final_text,
        events,
    })
}

/// Axum handler for `POST /api/ask`.
pub async fn ask_handler(
    State(state): State<AppState>,
    Json(req): Json<AskRequest>,
) -> Result<impl IntoResponse, AppError> {
    let resp = ask(&state.qa_config, &req.question).await?;
    Ok(Json(resp))
}
