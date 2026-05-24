use crate::approval_runtime::{
    ApprovalCancelledError, ApprovalResponse, ApprovalRuntime, ApprovalSource,
};
use crate::exception::ToolRejectedError;

#[derive(Debug, Clone)]
pub struct ApprovalState {
    pub yolo: bool,
    pub afk: bool,
    pub auto_approve_actions: Vec<String>,
}

impl ApprovalState {
    pub fn is_auto_approve(&self) -> bool {
        self.yolo || self.afk
    }

    pub fn is_afk(&self) -> bool {
        self.afk
    }

    pub fn is_afk_flag(&self) -> bool {
        self.afk
    }
}

impl Default for ApprovalState {
    fn default() -> Self {
        Self {
            yolo: false,
            afk: false,
            auto_approve_actions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApprovalResult {
    pub approved: bool,
    pub feedback: String,
}

impl ApprovalResult {
    pub fn new(approved: bool, feedback: impl Into<String>) -> Self {
        Self {
            approved,
            feedback: feedback.into(),
        }
    }

    pub fn rejection_error(&self) -> ToolRejectedError {
        if !self.feedback.is_empty() {
            ToolRejectedError::with_feedback(
                format!(
                    "The tool call is rejected by the user. User feedback: {}",
                    self.feedback
                ),
                format!("Rejected: {}", self.feedback),
            )
        } else {
            ToolRejectedError::new(
                "The tool call is rejected by the user. Try a different approach to complete your task, or explain the limitation in your summary if no alternative is available. Do not retry the same tool call, and do not attempt to bypass this restriction through indirect means.",
            )
        }
    }
}

#[derive(Clone)]
pub struct Approval {
    state: std::sync::Arc<std::sync::RwLock<ApprovalState>>,
    runtime: ApprovalRuntime,
    runtime_afk: std::sync::Arc<std::sync::RwLock<bool>>,
    on_change: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for Approval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Approval")
            .field("state", &self.state)
            .field("runtime", &self.runtime)
            .field("runtime_afk", &self.runtime_afk)
            .finish_non_exhaustive()
    }
}

impl Approval {
    pub fn new(yolo: bool) -> Self {
        let state = ApprovalState {
            yolo,
            ..Default::default()
        };
        Self {
            state: std::sync::Arc::new(std::sync::RwLock::new(state)),
            runtime: ApprovalRuntime::new(),
            runtime_afk: std::sync::Arc::new(std::sync::RwLock::new(false)),
            on_change: None,
        }
    }

    pub fn with_state(state: ApprovalState) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::RwLock::new(state)),
            runtime: ApprovalRuntime::new(),
            runtime_afk: std::sync::Arc::new(std::sync::RwLock::new(false)),
            on_change: None,
        }
    }

    pub fn share(&self) -> Self {
        Self {
            state: self.state.clone(),
            runtime: self.runtime.clone(),
            runtime_afk: self.runtime_afk.clone(),
            on_change: self.on_change.clone(),
        }
    }

    pub fn set_runtime(&mut self, runtime: ApprovalRuntime) {
        self.runtime = runtime;
    }

    pub fn runtime(&self) -> &ApprovalRuntime {
        &self.runtime
    }

    pub fn set_yolo(&self, yolo: bool) {
        let mut state = self.state.write().unwrap();
        state.yolo = yolo;
        drop(state);
        self.notify_change();
    }

    pub fn set_afk(&self, afk: bool) {
        let mut state = self.state.write().unwrap();
        state.afk = afk;
        if !afk {
            let mut rt_afk = self.runtime_afk.write().unwrap();
            *rt_afk = false;
        }
        drop(state);
        self.notify_change();
    }

    pub fn set_runtime_afk(&self, afk: bool) {
        let mut rt_afk = self.runtime_afk.write().unwrap();
        *rt_afk = afk;
    }

    pub fn is_auto_approve(&self) -> bool {
        let state = self.state.read().unwrap();
        let rt_afk = self.runtime_afk.read().unwrap();
        state.yolo || state.afk || *rt_afk
    }

    pub fn is_yolo(&self) -> bool {
        self.state.read().unwrap().yolo
    }

    pub fn is_yolo_flag(&self) -> bool {
        self.is_yolo()
    }

    pub fn is_afk(&self) -> bool {
        let state = self.state.read().unwrap();
        let rt_afk = self.runtime_afk.read().unwrap();
        state.afk || *rt_afk
    }

    pub fn is_afk_flag(&self) -> bool {
        self.state.read().unwrap().afk
    }

    pub fn is_runtime_afk(&self) -> bool {
        *self.runtime_afk.read().unwrap()
    }

    fn notify_change(&self) {
        if let Some(ref cb) = self.on_change {
            cb();
        }
    }

    pub async fn request(
        &self,
        sender: &str,
        action: &str,
        description: &str,
        display: Option<Vec<crate::approval_runtime::DisplayBlock>>,
    ) -> ApprovalResult {
        let tool_call = crate::soul::toolset::get_current_tool_call();

        if self.is_auto_approve() {
            return ApprovalResult::new(true, "");
        }

        let state = self.state.read().unwrap();
        if state.auto_approve_actions.contains(&action.to_string()) {
            return ApprovalResult::new(true, "");
        }
        drop(state);

        let request_id = uuid::Uuid::new_v4().to_string();
        let display_blocks = display.unwrap_or_default();
        let source = ApprovalSource {
            kind: "foreground_turn".to_string(),
            id: tool_call
                .as_ref()
                .map(|t| t.id.clone())
                .unwrap_or_else(|| request_id.clone()),
            agent_id: None,
        };

        self.runtime.create_request(
            request_id.clone(),
            tool_call.as_ref().map(|t| t.id.clone()).unwrap_or_default(),
            sender.to_string(),
            action.to_string(),
            description.to_string(),
            display_blocks,
            source,
        );

        match self.runtime.wait_for_response(&request_id).await {
            Ok(response) => match response {
                ApprovalResponse::Approve => ApprovalResult::new(true, ""),
                ApprovalResponse::ApproveForSession => {
                    let mut state = self.state.write().unwrap();
                    if !state.auto_approve_actions.contains(&action.to_string()) {
                        state.auto_approve_actions.push(action.to_string());
                    }
                    drop(state);
                    self.notify_change();
                    ApprovalResult::new(true, "")
                }
                ApprovalResponse::Reject { feedback } => ApprovalResult::new(false, feedback),
            },
            Err(ApprovalCancelledError) => {
                let record = self.runtime.get_request(&request_id);
                ApprovalResult::new(false, record.map(|r| r.feedback).unwrap_or_default())
            }
        }
    }
}
