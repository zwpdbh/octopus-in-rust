use async_trait::async_trait;
use llm_provider::tooling::{CallableTool2, ToolReturnValue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::soul::agent::{DenwaRenji, Dmail};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
impl CallableTool2 for SendDMailTool {
    type Params = SendDMailParams;

    fn name(&self) -> &str {
        "SendDMail"
    }

    fn description(&self) -> &str {
        "Send a D-Mail to a past checkpoint. Use this to communicate information back to a previous state of the conversation."
    }

    async fn call_typed(&self, params: SendDMailParams) -> ToolReturnValue {
        let dmail = Dmail {
            checkpoint_id: params.checkpoint_id,
            message: params.message,
        };

        let mut renji = self.denwa_renji.lock().unwrap();
        match renji.send_dmail(dmail) {
            Ok(()) => ToolReturnValue::ok(
                "If you see this message, the D-Mail was NOT sent successfully. \
                 This may be because some other tool that needs approval was rejected.",
            ),
            Err(e) => ToolReturnValue::error(format!("Failed to send D-Mail. Error: {}", e)),
        }
    }
}
