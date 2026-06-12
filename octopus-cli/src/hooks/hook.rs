use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use regex::Regex;
use serde_json::Value;

use crate::config::HookDef;
use crate::hooks::event::{HookEvent, HookEventKind};
use crate::hooks::runner::{HookAction, HookResult, run_hook};

/// Callback fired when one or more hooks are triggered for an event.
pub type OnTriggered = Arc<dyn Fn(&HookEvent, &str, usize) + Send + Sync>;

/// Callback fired after all triggered hooks have resolved.
pub type OnResolved = Arc<dyn Fn(&HookEvent, &str, HookAction, u64) + Send + Sync>;

/// Callback that dispatches a wire hook request to the client.
pub type OnWireHook = Arc<
    dyn Fn(WireHookHandle) -> Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync,
>;

/// Callback fired when a wire hook request is complete (success, timeout, or drop).
pub type OnWireHookDone = Arc<dyn Fn(&str) + Send + Sync>;

/// All callbacks the engine may provide to hook implementations.
#[derive(Clone, Default)]
pub struct HookCallbacks {
    pub on_triggered: Option<OnTriggered>,
    pub on_resolved: Option<OnResolved>,
    pub on_wire_hook: Option<OnWireHook>,
    pub on_wire_hook_done: Option<OnWireHookDone>,
}

/// Per-trigger context passed to every hook implementation.
#[derive(Clone, Default)]
pub struct HookRunContext {
    pub cwd: Option<PathBuf>,
    pub callbacks: HookCallbacks,
}

/// A client-side hook subscription registered via wire `initialize`.
#[derive(Debug, Clone)]
pub struct WireHookSubscription {
    pub id: String,
    pub event: HookEventKind,
    pub matcher: String,
    /// Compiled regex from `matcher`, computed when the subscription is added.
    pub compiled_matcher: Option<Regex>,
    pub timeout: u64,
}

/// A pending wire hook request waiting for client response.
#[derive(Debug)]
pub struct WireHookHandle {
    pub id: String,
    pub subscription_id: String,
    pub event_name: String,
    pub target: String,
    pub input_data: Value,
    tx: Option<tokio::sync::oneshot::Sender<HookResult>>,
}

impl WireHookHandle {
    pub fn resolve(self, action: HookAction) {
        if let Some(tx) = self.tx {
            let result = match action {
                HookAction::Allow => HookResult::allow(),
                HookAction::Block(reason) => HookResult::block(reason),
            };
            let _ = tx.send(result);
        }
    }
}

/// Object-safe clone helper for `Box<dyn Hook>`.
pub trait HookClone {
    fn clone_box(&self) -> Box<dyn Hook>;
}

impl<T> HookClone for T
where
    T: Hook + Clone + 'static,
{
    fn clone_box(&self) -> Box<dyn Hook> {
        Box::new(self.clone())
    }
}

/// A single runnable hook, independent of whether it is backed by a local
/// shell command or a wire client.
#[async_trait::async_trait]
pub trait Hook: Send + Sync + std::fmt::Debug + HookClone {
    /// Which event kind this hook listens for.
    fn kind(&self) -> HookEventKind;

    /// Optional regex matcher; `None` means the hook matches every event of
    /// its kind.
    fn matcher(&self) -> Option<&Regex>;

    /// Human-readable source label for diagnostics ("server" or "wire").
    fn source(&self) -> &'static str;

    /// The shell command for server-side hooks, if any.
    fn command(&self) -> Option<&str> {
        None
    }

    /// Execute the hook for the given concrete event.
    async fn run(&self, event: &HookEvent, ctx: &HookRunContext) -> HookResult;
}

/// Server-side hook that runs a shell command.
#[derive(Debug, Clone)]
pub struct CommandHook {
    pub kind: HookEventKind,
    pub matcher: Option<Regex>,
    pub command: String,
    pub timeout: u64,
}

impl CommandHook {
    pub fn new(def: &HookDef) -> Self {
        Self {
            kind: def.event,
            matcher: def
                .matcher
                .as_ref()
                .filter(|s| !s.is_empty())
                .and_then(|_| def.compiled_matcher.clone()),
            command: def.command.clone(),
            timeout: def.timeout,
        }
    }
}

#[async_trait::async_trait]
impl Hook for CommandHook {
    fn kind(&self) -> HookEventKind {
        self.kind
    }

    fn matcher(&self) -> Option<&Regex> {
        self.matcher.as_ref()
    }

    fn source(&self) -> &'static str {
        "server"
    }

    fn command(&self) -> Option<&str> {
        Some(&self.command)
    }

    async fn run(&self, event: &HookEvent, ctx: &HookRunContext) -> HookResult {
        run_hook(&self.command, event, self.timeout, ctx.cwd.as_deref()).await
    }
}

/// Client-side hook that forwards the event to a wire client and waits for a
/// `HookResponse`.
#[derive(Debug, Clone)]
pub struct WireHook {
    pub kind: HookEventKind,
    pub matcher: Option<Regex>,
    pub subscription_id: String,
    pub timeout: u64,
}

impl WireHook {
    pub fn new(sub: &WireHookSubscription) -> Self {
        Self {
            kind: sub.event,
            matcher: if sub.matcher.is_empty() {
                None
            } else {
                sub.compiled_matcher.clone()
            },
            subscription_id: sub.id.clone(),
            timeout: sub.timeout,
        }
    }
}

#[async_trait::async_trait]
impl Hook for WireHook {
    fn kind(&self) -> HookEventKind {
        self.kind
    }

    fn matcher(&self) -> Option<&Regex> {
        self.matcher.as_ref()
    }

    fn source(&self) -> &'static str {
        "wire"
    }

    async fn run(&self, event: &HookEvent, ctx: &HookRunContext) -> HookResult {
        let on_wire_hook = match ctx.callbacks.on_wire_hook.as_ref() {
            Some(cb) => cb.clone(),
            None => return HookResult::allow(),
        };
        let on_done = ctx.callbacks.on_wire_hook_done.clone();

        let (tx, rx) = tokio::sync::oneshot::channel();
        let input_data = serde_json::to_value(event).unwrap_or_default();
        let handle = WireHookHandle {
            id: uuid::Uuid::new_v4().to_string(),
            subscription_id: self.subscription_id.clone(),
            event_name: event.kind().to_string(),
            target: event.matcher_value().unwrap_or("").to_string(),
            input_data,
            tx: Some(tx),
        };
        let handle_id = handle.id.clone();
        let target = handle.target.clone();
        let timeout_secs = self.timeout;

        let cb_future = on_wire_hook(handle);
        cb_future.await;

        let result =
            match tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => {
                    tracing::warn!("Wire hook resolver dropped without resolving");
                    HookResult::allow()
                }
                Err(_) => {
                    tracing::warn!("Wire hook timed out: {}", target);
                    HookResult {
                        action: HookAction::Allow,
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: 0,
                        timed_out: true,
                    }
                }
            };

        if let Some(ref cb) = on_done {
            cb(&handle_id);
        }
        result
    }
}
