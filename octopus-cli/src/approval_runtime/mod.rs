use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::wire::RootWireHub;

#[derive(Debug, Clone)]
pub struct ApprovalSource {
    pub kind: String,
    pub id: String,
    pub agent_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("Approval request cancelled")]
pub struct ApprovalCancelledError;

#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool_call_id: String,
    pub sender: String,
    pub action: String,
    pub description: String,
    pub feedback: String,
    pub display: Vec<DisplayBlock>,
    pub source: ApprovalSource,
}

#[derive(Debug, Clone)]
pub struct DisplayBlock {
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ApprovalRuntime {
    inner: Arc<Mutex<ApprovalRuntimeInner>>,
}

#[derive(Debug, Default)]
struct ApprovalRuntimeInner {
    requests: HashMap<String, ApprovalRequest>,
}

impl ApprovalRuntime {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ApprovalRuntimeInner::default())),
        }
    }

    pub fn bind_root_wire_hub(&self, _hub: &RootWireHub) {}

    pub fn create_request(
        &self,
        request_id: String,
        tool_call_id: String,
        sender: String,
        action: String,
        description: String,
        display: Vec<DisplayBlock>,
        source: ApprovalSource,
    ) {
        let req = ApprovalRequest {
            id: request_id.clone(),
            tool_call_id,
            sender,
            action,
            description,
            feedback: String::new(),
            display,
            source,
        };
        let mut inner = self.inner.lock().unwrap();
        inner.requests.insert(request_id, req);
    }

    pub async fn wait_for_response(
        &self,
        _request_id: &str,
    ) -> Result<(String, String), ApprovalCancelledError> {
        // TODO: implement real approval flow
        // For now, stub: return approved with no feedback
        Ok(("approve".to_string(), "".to_string()))
    }

    pub fn cancel_by_source(&self, _kind: &str, _id: &str) {
        // TODO: implement cancellation
    }

    pub fn get_request(&self, id: &str) -> Option<ApprovalRequest> {
        let inner = self.inner.lock().unwrap();
        inner.requests.get(id).cloned()
    }

    pub fn list_pending(&self) -> Vec<ApprovalRequest> {
        let inner = self.inner.lock().unwrap();
        inner.requests.values().cloned().collect()
    }

    pub fn resolve(&self, id: &str, response: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(req) = inner.requests.get_mut(id) {
            req.feedback = response.to_string();
        }
    }
}

impl Default for ApprovalRuntime {
    fn default() -> Self {
        Self::new()
    }
}
