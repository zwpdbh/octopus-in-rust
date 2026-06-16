use async_trait::async_trait;
use kosong::Tool;
use kosong::message::Message;

use crate::core::errors::BrainError;

/// Builds the effective system prompt for a step.
#[async_trait]
pub trait SystemPromptPolicy: Send + Sync {
    async fn build_prompt(
        &self,
        base: &str,
        tools: &[Tool],
        history: &[Message],
    ) -> Result<String, BrainError>;
}

/// Default policy that returns `base` unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultSystemPromptPolicy;

#[async_trait]
impl SystemPromptPolicy for DefaultSystemPromptPolicy {
    async fn build_prompt(
        &self,
        base: &str,
        _tools: &[Tool],
        _history: &[Message],
    ) -> Result<String, BrainError> {
        Ok(base.to_string())
    }
}

/// Appends instruction fragments from the currently registered tools to the
/// base system prompt.
///
/// Tools declare fragments via `CallableTool::prompt_fragment()`. This policy
/// collects them, removes duplicates, and appends them under a
/// `### Tool usage instructions` heading. If no tool declares a fragment, the
/// base prompt is returned unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolAwareSystemPromptPolicy;

#[async_trait]
impl SystemPromptPolicy for ToolAwareSystemPromptPolicy {
    async fn build_prompt(
        &self,
        base: &str,
        tools: &[Tool],
        _history: &[Message],
    ) -> Result<String, BrainError> {
        let fragments: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.prompt_fragment.as_deref())
            .collect();

        if fragments.is_empty() {
            return Ok(base.to_string());
        }

        Ok(format!(
            "{}\n\n### Tool usage instructions\n\n{}",
            base,
            fragments.join("\n\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tool_aware_policy_appends_fragments() {
        let tools = vec![
            Tool {
                name: "qqbot_recent_messages".to_string(),
                description: "...".to_string(),
                parameters: serde_json::json!({}),
                prompt_fragment: Some("Call qqbot_recent_messages first.".to_string()),
            },
            Tool {
                name: "summary_format_conversation".to_string(),
                description: "...".to_string(),
                parameters: serde_json::json!({}),
                prompt_fragment: Some("Use summary_format_conversation to format.".to_string()),
            },
        ];

        let policy = ToolAwareSystemPromptPolicy;
        let prompt = policy
            .build_prompt("You are helpful.", &tools, &[])
            .await
            .unwrap();

        assert!(prompt.contains("You are helpful."));
        assert!(prompt.contains("Call qqbot_recent_messages first."));
        assert!(prompt.contains("Use summary_format_conversation to format."));
    }

    #[tokio::test]
    async fn test_tool_aware_policy_keeps_base_when_no_fragments() {
        let tools = vec![Tool {
            name: "other".to_string(),
            description: "...".to_string(),
            parameters: serde_json::json!({}),
            prompt_fragment: None,
        }];

        let policy = ToolAwareSystemPromptPolicy;
        let prompt = policy
            .build_prompt("Base prompt.", &tools, &[])
            .await
            .unwrap();
        assert_eq!(prompt, "Base prompt.");
    }
}
