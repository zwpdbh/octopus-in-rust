//! FAF Q&A module backed by `agent-core` and the `faf-units` plugin.

use std::{collections::HashSet, path::PathBuf, sync::Arc};

use agent_core::{
    Brain, BrainBuilder, BrainConfig, BrainError, BrainEvent, ExtismPluginSource,
    ToolAwareSystemPromptPolicy,
};
use axum::http::StatusCode;
use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::{config::workspace_root, error::AppError, llm_factory::ProviderType, state::AppState};

/// Configuration for the Q&A agent, loaded from environment variables.
#[derive(Clone, Debug)]
pub struct QaConfig {
    /// LLM backend type and provider-specific credentials.
    pub provider_type: ProviderType,

    /// LLM base URL.
    pub base_url: String,

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
    /// - `FAFCN_LLM_BASE_URL` (provider-specific default).
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

        let provider_type = match crate::env::var_or("FAFCN_LLM_PROVIDER_TYPE", "openai_compatible")
            .trim()
            .to_lowercase()
            .as_str()
        {
            "kimi_code" | "kimi-code" => ProviderType::KimiCode {
                token_file: crate::env::path_or("FAFCN_LLM_TOKEN_FILE", default_token_file),
            },
            _ => ProviderType::OpenAiCompatible {
                api_key: crate::env::var_or("FAFCN_LLM_API_KEY", ""),
            },
        };

        let model = crate::env::var_or("FAFCN_LLM_MODEL", "gpt-4o");
        let base_url = match &provider_type {
            ProviderType::KimiCode { .. } => {
                crate::env::var_or("FAFCN_LLM_BASE_URL", "https://api.kimi.com/coding/v1")
            }
            ProviderType::OpenAiCompatible { .. } => {
                crate::env::var_or("FAFCN_LLM_BASE_URL", "https://api.openai.com/v1")
            }
        };

        let plugins_dir = crate::env::path_or("FAFCN_PLUGINS_DIR", default_plugins);
        let system_prompt = crate::env::var_or(
            "FAFCN_QA_SYSTEM_PROMPT",
            "You are an expert assistant for the game Forged Alliance Forever. \
             Answer questions about units, buildings, and economy using the tools available.",
        );
        let max_steps_per_turn: usize = crate::env::var_or("FAFCN_QA_MAX_STEPS", "16").parse()?;

        let config = Self {
            provider_type,
            base_url,
            model,
            plugins_dir,
            system_prompt,
            max_steps_per_turn,
        };

        // Log the resolved provider configuration.  Secrets are never emitted.
        tracing::info!(
            provider_type = %config.provider_type,
            base_url = %config.base_url,
            model = %config.model,
            plugins_dir = %config.plugins_dir.display(),
            api_key_set = config.provider_type.api_key().map_or(false, |k| !k.is_empty()),
            token_file = ?config.provider_type.token_file().map(|p| p.display().to_string()),
            max_steps_per_turn = config.max_steps_per_turn,
            "QaConfig initialized"
        );

        Ok(config)
    }

    /// API key for the OpenAI-compatible provider, if any.
    fn api_key(&self) -> String {
        self.provider_type.api_key().unwrap_or("").to_string()
    }
}

/// Build a `Brain` that loads only the `faf_units_plugin`.
#[tracing::instrument(skip(config))]
pub async fn create_brain(config: &QaConfig) -> Result<Brain, BrainError> {
    tracing::info!(
        provider = ?config.provider_type,
        base_url = %config.base_url,
        model = %config.model,
        plugins_dir = %config.plugins_dir.display(),
        "building Q&A brain"
    );
    let allowed: HashSet<String> = ["faf_units_plugin"].into_iter().map(String::from).collect();
    let tool_source = Arc::new(ExtismPluginSource::with_filter(
        &config.plugins_dir,
        allowed,
    ));

    let brain_config = BrainConfig {
        system_prompt: config.system_prompt.clone(),
        base_url: config.base_url.clone(),
        api_key: config.api_key(),
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
#[tracing::instrument(skip(config))]
pub async fn ask(config: &QaConfig, question: &str) -> Result<QaResponse, BrainError> {
    tracing::info!(%question, "running Q&A turn");
    let mut brain = create_brain(config).await?;
    let result = brain
        .run_turn_to_completion(question.into())
        .await
        .map_err(|e| BrainError::Other(e.to_string()))?;

    let events: Vec<QaEvent> = result
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

    tracing::info!(
        event_count = events.len(),
        answer_len = result.final_text.len(),
        "Q&A turn complete"
    );

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

/// Health check response for `GET /api/health/qa`.
#[derive(Debug, Serialize)]
pub struct QaHealthResponse {
    pub status: &'static str,
    pub provider_type: String,
    pub base_url: String,
    pub model: String,
    pub reply: String,
}

/// Axum handler for `GET /api/health/qa`.
///
/// Performs a tiny provider call to verify that authentication and connectivity
/// are working. Returns `503 Service Unavailable` if the provider rejects the
/// request or cannot be reached.
pub async fn health_handler(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let config = &state.qa_config;
    match crate::llm_factory::verify_provider_auth(config).await {
        Ok(reply) => {
            let body = serde_json::json!(QaHealthResponse {
                status: "ok",
                provider_type: config.provider_type.to_string(),
                base_url: config.base_url.clone(),
                model: config.model.clone(),
                reply,
            });
            Ok((StatusCode::OK, Json(body)))
        }
        Err(e) => {
            let body = serde_json::json!({
                "status": "error",
                "error": e.to_string(),
                "provider_type": config.provider_type.to_string(),
                "base_url": config.base_url,
                "model": config.model,
            });
            Ok((StatusCode::SERVICE_UNAVAILABLE, Json(body)))
        }
    }
}
