use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::soul::agent::{DenwaRenji, Dmail};
use crate::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendDMailParams {
    pub message: String,
    pub checkpoint_id: usize,
}

pub struct SendDMailTool {
    denwa_renji: std::sync::Arc<std::sync::Mutex<DenwaRenji>>,
}

impl SendDMailTool {
    pub fn new(denwa_renji: std::sync::Arc<std::sync::Mutex<DenwaRenji>>) -> Self {
        Self { denwa_renji }
    }
}

#[async_trait]
impl Tool for SendDMailTool {
    fn name(&self) -> &str {
        "SendDMail"
    }

    fn description(&self) -> &str {
        "Send a D-Mail to a past checkpoint. Use this to communicate information back to a previous state of the conversation."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "SendDMail",
            "description": "Send a D-Mail to a past checkpoint. Use this to communicate information back to a previous state of the conversation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The message to send back to the past checkpoint."
                    },
                    "checkpoint_id": {
                        "type": "integer",
                        "description": "The checkpoint ID to send the message back to.",
                        "minimum": 0
                    }
                },
                "required": ["message", "checkpoint_id"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: SendDMailParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        let dmail = Dmail {
            checkpoint_id: params.checkpoint_id,
            message: params.message,
        };

        let mut renji = self.denwa_renji.lock().unwrap();
        match renji.send_dmail(dmail) {
            Ok(()) => Ok(
                "If you see this message, the D-Mail was NOT sent successfully. \
                 This may be because some other tool that needs approval was rejected."
                    .to_string(),
            ),
            Err(e) => Err(format!("Failed to send D-Mail. Error: {}", e)),
        }
    }
}
