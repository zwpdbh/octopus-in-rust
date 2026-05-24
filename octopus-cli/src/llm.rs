use std::collections::HashSet;
use std::sync::Arc;

use crate::config::{Config, LLMModel, LLMProvider, ModelCapability, ProviderType};
use crate::exception::OctopusError;

#[derive(Debug, Clone)]
pub struct LLM {
    pub model_name: String,
    pub max_context_size: usize,
    pub capabilities: HashSet<ModelCapability>,
    pub model_config: Option<LLMModel>,
    pub provider_config: Option<LLMProvider>,
}

pub fn model_display_name(model_name: Option<&str>, model: Option<&LLMModel>) -> String {
    if let Some(m) = model {
        if let Some(ref dn) = m.display_name {
            return dn.clone();
        }
    }
    match model_name {
        None | Some("") => String::new(),
        Some("kimi-for-coding") | Some("kimi-code") => "kimi-for-coding".to_string(),
        Some(name) => name.to_string(),
    }
}

pub fn augment_provider_with_env_vars(
    provider: &mut LLMProvider,
    model: &mut LLMModel,
) -> Vec<(String, String)> {
    let mut applied = Vec::new();

    match provider.provider_type {
        ProviderType::Kimi => {
            if let Ok(base_url) = std::env::var("KIMI_BASE_URL") {
                provider.base_url = base_url.clone();
                applied.push(("KIMI_BASE_URL".to_string(), base_url));
            }
            if let Ok(api_key) = std::env::var("KIMI_API_KEY") {
                provider.api_key = Some(api_key.clone());
                applied.push(("KIMI_API_KEY".to_string(), "******".to_string()));
            }
            if let Ok(model_name) = std::env::var("KIMI_MODEL_NAME") {
                model.model = model_name.clone();
                applied.push(("KIMI_MODEL_NAME".to_string(), model_name));
            }
            if let Ok(size) = std::env::var("KIMI_MODEL_MAX_CONTEXT_SIZE") {
                if let Ok(sz) = size.parse() {
                    model.max_context_size = sz;
                    applied.push(("KIMI_MODEL_MAX_CONTEXT_SIZE".to_string(), size));
                }
            }
            if let Ok(caps) = std::env::var("KIMI_MODEL_CAPABILITIES") {
                let new_caps: Vec<ModelCapability> = caps
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter_map(|s| match s.as_str() {
                        "image_in" => Some(ModelCapability::ImageIn),
                        "video_in" => Some(ModelCapability::VideoIn),
                        "thinking" => Some(ModelCapability::Thinking),
                        "always_thinking" => Some(ModelCapability::AlwaysThinking),
                        _ => None,
                    })
                    .collect();
                if !new_caps.is_empty() {
                    model.capabilities = Some(new_caps);
                    applied.push(("KIMI_MODEL_CAPABILITIES".to_string(), caps));
                }
            }
        }
        ProviderType::OpenaiLegacy | ProviderType::OpenaiResponses => {
            if let Ok(base_url) = std::env::var("OPENAI_BASE_URL") {
                provider.base_url = base_url;
            }
            if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
                provider.api_key = Some(api_key);
            }
        }
        _ => {}
    }

    applied
}

pub fn derive_model_capabilities(model: &LLMModel) -> HashSet<ModelCapability> {
    let mut capabilities = model
        .capabilities
        .clone()
        .map(|c| c.into_iter().collect::<HashSet<_>>())
        .unwrap_or_default();

    let model_lower = model.model.to_lowercase();
    if model_lower.contains("thinking") || model_lower.contains("reason") {
        capabilities.insert(ModelCapability::Thinking);
        capabilities.insert(ModelCapability::AlwaysThinking);
    } else if model.model == "kimi-for-coding" || model.model == "kimi-code" {
        capabilities.insert(ModelCapability::Thinking);
        capabilities.insert(ModelCapability::ImageIn);
        capabilities.insert(ModelCapability::VideoIn);
    }

    capabilities
}

pub fn create_llm(
    provider: &LLMProvider,
    model: &LLMModel,
    _thinking: Option<bool>,
    _session_id: Option<&str>,
) -> Option<LLM> {
    let capabilities = derive_model_capabilities(model);
    Some(LLM {
        model_name: model.model.clone(),
        max_context_size: model.max_context_size,
        capabilities,
        model_config: Some(model.clone()),
        provider_config: Some(provider.clone()),
    })
}

pub fn clone_llm_with_model_alias(
    llm: Option<&LLM>,
    config: &Config,
    model_alias: Option<&str>,
) -> crate::exception::Result<Option<LLM>> {
    let alias = match model_alias {
        Some(a) => a,
        None => return Ok(llm.cloned()),
    };
    let model = config.models.get(alias).ok_or_else(|| {
        crate::exception::OctopusError::Other(format!("Unknown model alias: {}", alias))
    })?;
    let provider = config.providers.get(&model.provider).ok_or_else(|| {
        crate::exception::OctopusError::Other(format!("Provider not found: {}", model.provider))
    })?;
    Ok(create_llm(provider, model, None, None))
}

#[derive(Debug, Clone)]
pub struct ChatCompletion {
    pub id: Option<String>,
    pub message: crate::wire::Message,
    pub usage: Option<crate::wire::TokenUsage>,
    pub tool_calls: Vec<crate::wire::ToolCall>,
}

impl LLM {
    pub async fn complete(
        &self,
        system_prompt: Option<&str>,
        messages: &[crate::wire::Message],
        tools: Option<&[&dyn crate::tools::Tool]>,
    ) -> crate::exception::Result<ChatCompletion> {
        let provider = self.build_kosong_provider()?;
        let kosong_history: Vec<kosong::Message> =
            messages.iter().map(wire_to_kosong_message).collect();
        let kosong_tools: Vec<kosong::Tool> = tools
            .map(|ts| ts.iter().map(|t| wire_to_kosong_tool(*t)).collect())
            .unwrap_or_default();

        let result = kosong::generate(
            provider.as_ref(),
            system_prompt.unwrap_or(""),
            &kosong_tools,
            &kosong_history,
            None,
            None,
        )
        .await
        .map_err(|e| classify_kosong_error(e.to_string()))?;

        let tool_calls = result
            .message
            .tool_calls
            .clone()
            .map(|tcs| tcs.into_iter().map(kosong_to_wire_tool_call).collect())
            .unwrap_or_default();

        Ok(ChatCompletion {
            id: result.id,
            message: kosong_to_wire_message(result.message),
            usage: result.usage.map(kosong_to_wire_usage),
            tool_calls,
        })
    }

    fn build_kosong_provider(&self) -> crate::exception::Result<Arc<dyn kosong::ChatProvider>> {
        let provider_config = self
            .provider_config
            .as_ref()
            .ok_or_else(|| OctopusError::Other("Provider config not set".to_string()))?;

        match provider_config.provider_type {
            ProviderType::Kimi => {
                let mut kimi = kosong::provider::kimi::Kimi::new(&self.model_name)
                    .with_base_url(&provider_config.base_url);
                if let Some(ref key) = provider_config.api_key {
                    kimi = kimi.with_api_key(key);
                }
                Ok(Arc::new(kimi))
            }
            ProviderType::OpenaiLegacy | ProviderType::OpenaiResponses => {
                let mut openai =
                    kosong::provider::openai_legacy::OpenAILegacy::new(&self.model_name)
                        .with_base_url(&provider_config.base_url);
                if let Some(ref key) = provider_config.api_key {
                    openai = openai.with_api_key(key);
                }
                if let Some(ref key) = provider_config.reasoning_key {
                    openai = openai.with_reasoning_key(key);
                }
                Ok(Arc::new(openai))
            }
            ref other => Err(OctopusError::Other(format!(
                "Provider type {:?} not yet supported by kosong integration",
                other
            ))),
        }
    }
}

// ============================================================================
// Type conversions: wire <-> kosong
// ============================================================================

fn wire_to_kosong_message(msg: &crate::wire::Message) -> kosong::Message {
    kosong::Message {
        role: match msg.role.as_str() {
            "system" => kosong::Role::System,
            "user" => kosong::Role::User,
            "assistant" => kosong::Role::Assistant,
            "tool" => kosong::Role::Tool,
            _ => kosong::Role::User,
        },
        name: None,
        content: msg
            .content
            .iter()
            .map(wire_to_kosong_content_part)
            .collect(),
        tool_calls: msg
            .tool_calls
            .as_ref()
            .map(|tcs| tcs.iter().map(wire_to_kosong_tool_call).collect()),
        tool_call_id: msg.tool_call_id.clone(),
        partial: None,
    }
}

fn wire_to_kosong_content_part(part: &crate::wire::ContentPart) -> kosong::ContentPart {
    match part {
        crate::wire::ContentPart::Text { text } => kosong::ContentPart::Text { text: text.clone() },
        crate::wire::ContentPart::ImageUrl { image_url } => kosong::ContentPart::ImageUrl {
            image_url: kosong::ImageUrl {
                url: image_url.url.clone(),
                detail: None,
            },
        },
        crate::wire::ContentPart::AudioUrl { audio_url } => kosong::ContentPart::AudioUrl {
            audio_url: kosong::AudioUrl {
                url: audio_url.url.clone(),
            },
        },
        crate::wire::ContentPart::VideoUrl { video_url } => kosong::ContentPart::VideoUrl {
            video_url: kosong::VideoUrl {
                url: video_url.url.clone(),
            },
        },
        crate::wire::ContentPart::Think { think } => kosong::ContentPart::Think {
            think: think.clone(),
            encrypted: None,
        },
    }
}

fn wire_to_kosong_tool_call(tc: &crate::wire::ToolCall) -> kosong::ToolCall {
    kosong::ToolCall {
        call_type: tc.call_type.clone(),
        id: tc.id.clone(),
        function: kosong::FunctionBody {
            name: tc.function.name.clone(),
            arguments: Some(tc.function.arguments.clone()),
        },
        extras: None,
    }
}

fn wire_to_kosong_tool(tool: &dyn crate::tools::Tool) -> kosong::Tool {
    kosong::Tool {
        name: tool.name().to_string(),
        description: tool.description().to_string(),
        parameters: tool.schema(),
    }
}

fn kosong_to_wire_message(msg: kosong::Message) -> crate::wire::Message {
    crate::wire::Message {
        role: match msg.role {
            kosong::Role::System => "system".to_string(),
            kosong::Role::User => "user".to_string(),
            kosong::Role::Assistant => "assistant".to_string(),
            kosong::Role::Tool => "tool".to_string(),
        },
        content: msg
            .content
            .into_iter()
            .map(kosong_to_wire_content_part)
            .collect(),
        tool_call_id: msg.tool_call_id,
        tool_calls: msg
            .tool_calls
            .map(|tcs| tcs.into_iter().map(kosong_to_wire_tool_call).collect()),
    }
}

fn kosong_to_wire_content_part(part: kosong::ContentPart) -> crate::wire::ContentPart {
    match part {
        kosong::ContentPart::Text { text } => crate::wire::ContentPart::Text { text },
        kosong::ContentPart::ImageUrl { image_url } => crate::wire::ContentPart::ImageUrl {
            image_url: crate::wire::MediaUrl { url: image_url.url },
        },
        kosong::ContentPart::AudioUrl { audio_url } => crate::wire::ContentPart::AudioUrl {
            audio_url: crate::wire::MediaUrl { url: audio_url.url },
        },
        kosong::ContentPart::VideoUrl { video_url } => crate::wire::ContentPart::VideoUrl {
            video_url: crate::wire::MediaUrl { url: video_url.url },
        },
        kosong::ContentPart::Think { think, .. } => crate::wire::ContentPart::Think { think },
    }
}

fn kosong_to_wire_tool_call(tc: kosong::ToolCall) -> crate::wire::ToolCall {
    crate::wire::ToolCall {
        id: tc.id,
        call_type: tc.call_type,
        function: crate::wire::ToolCallFunction {
            name: tc.function.name,
            arguments: tc.function.arguments.unwrap_or_default(),
        },
    }
}

fn kosong_to_wire_usage(usage: kosong::TokenUsage) -> crate::wire::TokenUsage {
    crate::wire::TokenUsage {
        input: usage.input(),
        output: usage.output,
        total: usage.total(),
    }
}

/// Classify a kosong error string into a specific OctopusError variant.
fn classify_kosong_error(msg: String) -> crate::exception::OctopusError {
    if msg.starts_with("API connection error:") {
        return crate::exception::OctopusError::APIConnection(
            crate::exception::APIConnectionError(msg),
        );
    }
    if msg.starts_with("API timeout error:") {
        return crate::exception::OctopusError::APITimeout(crate::exception::APITimeoutError(msg));
    }
    if msg.starts_with("API status error ") {
        // Parse "API status error {status_code}: {message}"
        let rest = &msg["API status error ".len()..];
        if let Some(colon_pos) = rest.find(':') {
            let status_part = &rest[..colon_pos];
            if let Ok(status_code) = status_part.parse::<u16>() {
                let message = rest[colon_pos + 2..].to_string(); // skip ": "
                return crate::exception::OctopusError::APIStatus(
                    crate::exception::APIStatusError {
                        status_code,
                        message,
                    },
                );
            }
        }
    }
    if msg == "API returned an empty response" {
        return crate::exception::OctopusError::APIEmptyResponse(
            crate::exception::APIEmptyResponseError,
        );
    }
    crate::exception::OctopusError::Other(msg)
}
