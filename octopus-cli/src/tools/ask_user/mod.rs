use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserParams {
    pub question: String,
    #[serde(default)]
    pub options: Vec<AskUserOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserOption {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub struct AskUserTool;

impl AskUserTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }

    fn description(&self) -> &str {
        "Ask the user a question with structured options."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "AskUserQuestion",
            "description": "Ask the user a question with structured options.",
            "parameters": {
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "The question to ask" },
                    "options": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": { "type": "string" },
                                "description": { "type": "string" }
                            },
                            "required": ["label"]
                        }
                    }
                },
                "required": ["question"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: AskUserParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        println!("\n[ASK USER] {}\n", params.question);
        for (i, opt) in params.options.iter().enumerate() {
            println!("  {}. {}", i + 1, opt.label);
        }
        println!();

        Ok("User was asked a question. In interactive mode, the user will respond. In non-interactive mode, this is auto-dismissed.".to_string())
    }
}
