use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;

use regex::Regex;

use crate::config::HookDef;
use crate::hooks::event::HookEvent;
use crate::hooks::runner::{HookAction, HookResult, run_hook};

/// A client-side hook subscription registered via wire initialize.
#[derive(Debug, Clone)]
pub struct WireHookSubscription {
    pub id: String,
    pub event: HookEvent,
    pub matcher: String,
    pub timeout: u64,
}

/// Callback signatures for wire integration.
pub type OnTriggered = Box<dyn Fn(&HookEvent, &str, usize) + Send + Sync>;
pub type OnResolved = Box<dyn Fn(&HookEvent, &str, HookAction, u64) + Send + Sync>;
pub type OnWireHook = Box<
    dyn Fn(WireHookHandle) -> Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync,
>;

/// A pending wire hook request waiting for client response.
#[derive(Debug)]
pub struct WireHookHandle {
    pub id: String,
    pub subscription_id: String,
    pub event_name: String,
    pub target: String,
    pub input_data: serde_json::Value,
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

/// Loads hook definitions and executes matching hooks in parallel.
///
/// Supports two hook sources:
/// - Server-side (config.toml): shell commands executed locally
/// - Client-side (wire subscriptions): forwarded to client via HookRequest
pub struct HookEngine {
    hooks: Vec<HookDef>,
    wire_subs: Vec<WireHookSubscription>,
    cwd: Option<PathBuf>,
    on_triggered: Option<OnTriggered>,
    on_resolved: Option<OnResolved>,
    on_wire_hook: Option<OnWireHook>,
    by_event: HashMap<HookEvent, Vec<HookDef>>,
    wire_by_event: HashMap<HookEvent, Vec<WireHookSubscription>>,
}

impl HookEngine {
    pub fn new(hooks: Vec<HookDef>) -> Self {
        let mut engine = Self {
            hooks,
            wire_subs: Vec::new(),
            cwd: None,
            on_triggered: None,
            on_resolved: None,
            on_wire_hook: None,
            by_event: HashMap::new(),
            wire_by_event: HashMap::new(),
        };
        engine.rebuild_index();
        engine
    }

    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    pub fn set_callbacks(
        &mut self,
        on_triggered: Option<OnTriggered>,
        on_resolved: Option<OnResolved>,
        on_wire_hook: Option<OnWireHook>,
    ) {
        self.on_triggered = on_triggered;
        self.on_resolved = on_resolved;
        self.on_wire_hook = on_wire_hook;
    }

    pub fn add_hooks(&mut self, hooks: Vec<HookDef>) {
        self.hooks.extend(hooks);
        self.rebuild_index();
    }

    pub fn add_wire_subscriptions(&mut self, subs: Vec<WireHookSubscription>) {
        self.wire_subs.extend(subs);
        self.rebuild_index();
    }

    pub fn has_hooks(&self) -> bool {
        !self.hooks.is_empty() || !self.wire_subs.is_empty()
    }

    pub fn has_hooks_for(&self, event: &HookEvent) -> bool {
        self.by_event.contains_key(event) || self.wire_by_event.contains_key(event)
    }

    pub fn summary(&self) -> HashMap<String, usize> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (event, hooks) in &self.by_event {
            *counts.entry(event.to_string()).or_insert(0) += hooks.len();
        }
        for (event, subs) in &self.wire_by_event {
            *counts.entry(event.to_string()).or_insert(0) += subs.len();
        }
        counts
    }

    pub fn details(&self) -> HashMap<String, Vec<HashMap<String, String>>> {
        let mut result: HashMap<String, Vec<HashMap<String, String>>> = HashMap::new();
        for (event, hooks) in &self.by_event {
            let entries = result.entry(event.to_string()).or_default();
            for h in hooks {
                let mut entry = HashMap::new();
                entry.insert("matcher".to_string(), h.matcher.clone().unwrap_or_default());
                entry.insert("source".to_string(), "server".to_string());
                entry.insert("command".to_string(), h.command.clone());
                entries.push(entry);
            }
        }
        for (event, subs) in &self.wire_by_event {
            let entries = result.entry(event.to_string()).or_default();
            for s in subs {
                let mut entry = HashMap::new();
                entry.insert("matcher".to_string(), s.matcher.clone());
                entry.insert("source".to_string(), "wire".to_string());
                entry.insert("command".to_string(), "(client-side)".to_string());
                entries.push(entry);
            }
        }
        result
    }

    fn rebuild_index(&mut self) {
        self.by_event.clear();
        for h in &self.hooks {
            self.by_event
                .entry(h.event.clone())
                .or_default()
                .push(h.clone());
        }
        self.wire_by_event.clear();
        for s in &self.wire_subs {
            self.wire_by_event
                .entry(s.event.clone())
                .or_default()
                .push(s.clone());
        }
    }

    fn match_regex(&self, pattern: &str, value: &str) -> bool {
        if pattern.is_empty() {
            return true;
        }
        match Regex::new(pattern) {
            Ok(re) => re.is_match(value),
            Err(e) => {
                tracing::warn!("Invalid regex in hook matcher '{}': {}", pattern, e);
                false
            }
        }
    }

    /// Run all matching hooks (server + wire) in parallel.
    pub async fn trigger(&self, event: HookEvent, matcher_value: &str) -> Vec<HookResult> {
        // Match server-side hooks
        let mut seen_commands: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut server_matched: Vec<&HookDef> = Vec::new();
        for h in self.by_event.get(&event).into_iter().flatten() {
            if !self.match_regex(h.matcher.as_deref().unwrap_or(""), matcher_value) {
                continue;
            }
            if seen_commands.contains(&h.command) {
                continue;
            }
            seen_commands.insert(h.command.clone());
            server_matched.push(h);
        }

        // Match wire subscriptions
        let wire_matched: Vec<&WireHookSubscription> = self
            .wire_by_event
            .get(&event)
            .into_iter()
            .flatten()
            .filter(|s| self.match_regex(&s.matcher, matcher_value))
            .collect();

        let total = server_matched.len() + wire_matched.len();
        if total == 0 {
            return Vec::new();
        }

        // Emit triggered callback
        if let Some(ref cb) = self.on_triggered {
            cb(&event, matcher_value, total);
        }

        let t0 = std::time::Instant::now();
        let mut tasks: Vec<tokio::task::JoinHandle<HookResult>> = Vec::new();

        // Server-side: run shell commands
        for h in server_matched {
            let command = h.command.clone();
            let event = event.clone();
            let timeout = h.timeout;
            let cwd = self.cwd.clone();
            tasks.push(tokio::spawn(async move {
                run_hook(&command, &event, timeout, cwd.as_deref()).await
            }));
        }

        // Wire-side: dispatch to client via callback
        for s in wire_matched {
            if let Some(ref cb) = self.on_wire_hook {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let handle = WireHookHandle {
                    id: uuid::Uuid::new_v4().to_string(),
                    subscription_id: s.id.clone(),
                    event_name: event.to_string(),
                    target: matcher_value.to_string(),
                    input_data: serde_json::to_value(&event).unwrap_or_default(),
                    tx: Some(tx),
                };
                let timeout_secs = s.timeout;
                let target = matcher_value.to_string();
                let cb_future = cb(handle);
                tasks.push(tokio::spawn(async move {
                    // First let the callback send the request to the client.
                    cb_future.await;
                    // Then wait for the client response with a timeout.
                    match tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), rx)
                        .await
                    {
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
                    }
                }));
            } else {
                tasks.push(tokio::spawn(async move { HookResult::allow() }));
            }
        }

        let results: Vec<HookResult> = match futures::future::try_join_all(tasks).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Hook engine task join error for {}: {}", event, e);
                return Vec::new();
            }
        };

        let duration_ms = t0.elapsed().as_millis() as u64;

        // Aggregate: block if any hook blocked
        let mut action = HookAction::Allow;
        for r in &results {
            if let HookAction::Block(ref reason) = r.action {
                action = HookAction::Block(reason.clone());
                tracing::warn!(
                    "Hook blocked {} (matcher={}): {}",
                    event,
                    matcher_value,
                    reason
                );
                break;
            }
        }

        // Emit resolved callback
        if let Some(ref cb) = self.on_resolved {
            cb(&event, matcher_value, action, duration_ms);
        }

        results
    }

    /// Trigger a hook in the background (fire-and-forget).
    ///
    /// The returned task must be awaited or stored; otherwise the hook
    /// may be cancelled when the caller drops the handle.
    pub fn fire_and_forget_trigger(
        &self,
        event: HookEvent,
        matcher_value: &str,
    ) -> tokio::task::JoinHandle<Vec<HookResult>> {
        let matcher_value = matcher_value.to_string();
        let engine = self.clone();
        tokio::spawn(async move { engine.trigger(event, &matcher_value).await })
    }
}

impl Clone for HookEngine {
    fn clone(&self) -> Self {
        let mut engine = Self {
            hooks: self.hooks.clone(),
            wire_subs: self.wire_subs.clone(),
            cwd: self.cwd.clone(),
            on_triggered: None,
            on_resolved: None,
            on_wire_hook: None,
            by_event: HashMap::new(),
            wire_by_event: HashMap::new(),
        };
        engine.rebuild_index();
        engine
    }
}

impl Default for HookEngine {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Build a HookDef the same way config deserialization does: event has
    /// empty payload fields because the config file only stores the variant
    /// name (e.g. `event = "PreToolUse"`).
    fn config_hook_def(event: HookEvent, command: &str, matcher: Option<&str>) -> HookDef {
        HookDef {
            event,
            matcher: matcher.map(|s| s.to_string()),
            command: command.to_string(),
            timeout: 30,
        }
    }

    #[test]
    fn test_config_event_matches_runtime_event() {
        // This is what the engine sees after loading from config.toml:
        // event = "PreToolUse"  ->  HookEvent::PreToolUse with all empty strings
        let config_event = HookEvent::PreToolUse {
            session_id: String::new(),
            cwd: String::new(),
            tool_name: String::new(),
            tool_input: HashMap::new(),
            tool_call_id: String::new(),
        };
        let def = config_hook_def(config_event, "echo ok", None);
        let engine = HookEngine::new(vec![def]);

        // This is the real runtime event fired when a tool is about to run:
        let runtime_event = HookEvent::pre_tool_use(
            "real-sess",
            "/real/cwd",
            "WriteFile",
            &HashMap::new(),
            "call-42",
        );

        assert!(
            engine.has_hooks_for(&runtime_event),
            "config hook with empty payload should match runtime event with real payload"
        );
    }

    #[test]
    fn test_different_events_do_not_match() {
        let config_event = HookEvent::PreToolUse {
            session_id: String::new(),
            cwd: String::new(),
            tool_name: String::new(),
            tool_input: HashMap::new(),
            tool_call_id: String::new(),
        };
        let engine = HookEngine::new(vec![config_hook_def(config_event, "echo ok", None)]);

        let runtime_event = HookEvent::post_tool_use(
            "real-sess",
            "/real/cwd",
            "WriteFile",
            &HashMap::new(),
            "done",
            "call-42",
        );

        assert!(
            !engine.has_hooks_for(&runtime_event),
            "PreToolUse config hook should not match PostToolUse runtime event"
        );
    }

    #[test]
    fn test_summary_counts_by_discriminant() {
        let hooks = vec![
            config_hook_def(
                HookEvent::PreToolUse {
                    session_id: String::new(),
                    cwd: String::new(),
                    tool_name: String::new(),
                    tool_input: HashMap::new(),
                    tool_call_id: String::new(),
                },
                "echo 1",
                None,
            ),
            config_hook_def(
                HookEvent::PreToolUse {
                    session_id: "other".into(),
                    cwd: "/other".into(),
                    tool_name: "other".into(),
                    tool_input: HashMap::new(),
                    tool_call_id: "other".into(),
                },
                "echo 2",
                None,
            ),
            config_hook_def(
                HookEvent::PostToolUse {
                    session_id: String::new(),
                    cwd: String::new(),
                    tool_name: String::new(),
                    tool_input: HashMap::new(),
                    tool_output: String::new(),
                    tool_call_id: String::new(),
                },
                "echo 3",
                None,
            ),
        ];
        let engine = HookEngine::new(hooks);
        let summary = engine.summary();
        assert_eq!(summary.get("PreToolUse"), Some(&2));
        assert_eq!(summary.get("PostToolUse"), Some(&1));
    }
}
