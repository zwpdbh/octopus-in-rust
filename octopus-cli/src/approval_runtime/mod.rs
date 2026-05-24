use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::wire::{ApprovalRequestEvent, ApprovalResponseEvent, RootWireHub};

#[derive(Debug, Clone)]
pub struct ApprovalSource {
    pub kind: String,
    pub id: String,
    pub agent_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("Approval request cancelled")]
pub struct ApprovalCancelledError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum ApprovalResponse {
    Approve,
    ApproveForSession,
    Reject { feedback: String },
}

#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool_call_id: String,
    pub sender: String,
    pub action: String,
    pub description: String,
    pub feedback: String,
    pub display: Vec<crate::wire::DisplayBlock>,
    pub source: ApprovalSource,
    pub created_at: Instant,
    pub status: ApprovalStatus,
    pub resolved_at: Option<Instant>,
    pub response: Option<ApprovalResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    Resolved,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct ApprovalRuntime {
    inner: Arc<Mutex<ApprovalRuntimeInner>>,
}

#[derive(Debug, Default)]
struct ApprovalRuntimeInner {
    requests: HashMap<String, ApprovalRequest>,
    waiters: HashMap<String, oneshot::Sender<ApprovalResponse>>,
    hub: Option<RootWireHub>,
}

impl ApprovalRuntime {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ApprovalRuntimeInner::default())),
        }
    }

    pub fn bind_root_wire_hub(&self, hub: &RootWireHub) {
        self.inner.lock().unwrap().hub = Some(hub.clone());
    }

    pub fn create_request(
        &self,
        request_id: String,
        tool_call_id: String,
        sender: String,
        action: String,
        description: String,
        display: Vec<crate::wire::DisplayBlock>,
        source: ApprovalSource,
    ) {
        let req = ApprovalRequest {
            id: request_id.clone(),
            tool_call_id,
            sender: sender.clone(),
            action: action.clone(),
            description,
            feedback: String::new(),
            display,
            source,
            created_at: Instant::now(),
            status: ApprovalStatus::Pending,
            resolved_at: None,
            response: None,
        };

        // Publish to wire hub
        {
            let inner = self.inner.lock().unwrap();
            if let Some(hub) = inner.hub.as_ref() {
                let event = ApprovalRequestEvent {
                    id: req.id.clone(),
                    tool_call_id: req.tool_call_id.clone(),
                    sender,
                    action,
                    description: req.description.clone(),
                    source_kind: req.source.kind.clone(),
                    source_id: req.source.id.clone(),
                    display: req.display.clone(),
                };
                if let Ok(value) = serde_json::to_value(event) {
                    hub.publish(value);
                }
            }
        }

        let mut inner = self.inner.lock().unwrap();
        inner.requests.insert(request_id, req);
    }

    pub async fn wait_for_response(
        &self,
        request_id: &str,
        timeout: Option<Duration>,
    ) -> Result<ApprovalResponse, ApprovalCancelledError> {
        let rx = {
            let mut inner = self.inner.lock().unwrap();

            // Check if already resolved
            if let Some(req) = inner.requests.get(request_id) {
                if req.status == ApprovalStatus::Resolved {
                    return Ok(req.response.clone().unwrap_or(ApprovalResponse::Reject {
                        feedback: req.feedback.clone(),
                    }));
                }
                if req.status == ApprovalStatus::Cancelled {
                    return Err(ApprovalCancelledError);
                }
            }

            // Create a new waiter channel
            let (tx, rx) = oneshot::channel();
            inner.waiters.insert(request_id.to_string(), tx);
            rx
        };

        if let Some(duration) = timeout {
            match tokio::time::timeout(duration, rx).await {
                Ok(Ok(response)) => Ok(response),
                Ok(Err(_)) => {
                    self._cancel_request(request_id, "approval channel closed");
                    Err(ApprovalCancelledError)
                }
                Err(_) => {
                    self._cancel_request(request_id, "approval timed out");
                    Err(ApprovalCancelledError)
                }
            }
        } else {
            match rx.await {
                Ok(response) => Ok(response),
                Err(_) => {
                    self._cancel_request(request_id, "approval channel closed");
                    Err(ApprovalCancelledError)
                }
            }
        }
    }

    pub fn resolve(&self, request_id: &str, response: ApprovalResponse) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let request = match inner.requests.get_mut(request_id) {
            Some(r) if r.status == ApprovalStatus::Pending => r,
            _ => return false,
        };

        request.status = ApprovalStatus::Resolved;
        request.response = Some(response.clone());
        request.resolved_at = Some(Instant::now());

        let feedback = match &response {
            ApprovalResponse::Reject { feedback } => feedback.clone(),
            _ => String::new(),
        };
        request.feedback = feedback.clone();

        // Clone needed data before releasing mutable borrow
        let hub = inner.hub.clone();
        let request_id_owned = request_id.to_string();

        // Notify waiters
        if let Some(tx) = inner.waiters.remove(request_id) {
            let _ = tx.send(response.clone());
        }
        drop(inner);

        // Publish response to wire hub
        if let Some(hub) = hub {
            let event = ApprovalResponseEvent {
                request_id: request_id_owned,
                response: match response {
                    ApprovalResponse::Approve => "approve".to_string(),
                    ApprovalResponse::ApproveForSession => "approve_for_session".to_string(),
                    ApprovalResponse::Reject { .. } => "reject".to_string(),
                },
                feedback,
            };
            if let Ok(value) = serde_json::to_value(event) {
                hub.publish(value);
            }
        }

        true
    }

    pub fn cancel_by_source(&self, kind: &str, id: &str) {
        let request_ids: Vec<String> = {
            let inner = self.inner.lock().unwrap();
            inner
                .requests
                .iter()
                .filter(|(_, req)| {
                    req.status == ApprovalStatus::Pending
                        && req.source.kind == kind
                        && req.source.id == id
                })
                .map(|(id, _)| id.clone())
                .collect()
        };

        for request_id in request_ids {
            self._cancel_request(&request_id, "turn ended");
        }
    }

    pub fn get_request(&self, id: &str) -> Option<ApprovalRequest> {
        let inner = self.inner.lock().unwrap();
        inner.requests.get(id).cloned()
    }

    pub fn list_pending(&self) -> Vec<ApprovalRequest> {
        let inner = self.inner.lock().unwrap();
        inner
            .requests
            .values()
            .filter(|r| r.status == ApprovalStatus::Pending)
            .cloned()
            .collect()
    }

    fn _cancel_request(&self, request_id: &str, feedback: &str) {
        let mut inner = self.inner.lock().unwrap();
        let request = match inner.requests.get_mut(request_id) {
            Some(r) if r.status == ApprovalStatus::Pending => r,
            _ => return,
        };

        request.status = ApprovalStatus::Cancelled;
        request.feedback = feedback.to_string();
        request.resolved_at = Some(Instant::now());
        request.response = Some(ApprovalResponse::Reject {
            feedback: feedback.to_string(),
        });

        if let Some(tx) = inner.waiters.remove(request_id) {
            let _ = tx.send(ApprovalResponse::Reject {
                feedback: feedback.to_string(),
            });
        }
    }
}

impl Default for ApprovalRuntime {
    fn default() -> Self {
        Self::new()
    }
}
