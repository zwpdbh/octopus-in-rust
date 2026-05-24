pub mod events;
pub mod runner;

use std::collections::HashMap;
use std::path::PathBuf;

use regex::Regex;
use serde_json::Value;

use crate::config::HookDef;
use crate::hooks::runner::{HookAction, HookResult, run_hook};

/// A client-side hook subscription registered via wire initialize.
#[derive(Debug, Clone)]
pub struct WireHookSubscription {
    pub id: String,
    pub event: String,
    pub matcher: String,
    pub timeout: u64,
}

/// Callback signatures for wire integration.
pub type OnTriggered = Box<dyn Fn(&str, &str, usize) + Send + Sync>;
pub type OnResolved = Box<dyn Fn(&str, &str, HookAction, u64) + Send + Sync>;

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
    by_event: HashMap<String, Vec<HookDef>>,
    wire_by_event: HashMap<String, Vec<WireHookSubscription>>,
}

impl HookEngine {
    pub fn new(hooks: Vec<HookDef>) -> Self {
        let mut engine = Self {
            hooks,
            wire_subs: Vec::new(),
            cwd: None,
            on_triggered: None,
            on_resolved: None,
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
    ) {
        self.on_triggered = on_triggered;
        self.on_resolved = on_resolved;
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

    pub fn has_hooks_for(&self, event: &str) -> bool {
        self.by_event.contains_key(event) || self.wire_by_event.contains_key(event)
    }

    pub fn summary(&self) -> HashMap<String, usize> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (event, hooks) in &self.by_event {
            *counts.entry(event.clone()).or_insert(0) += hooks.len();
        }
        for (event, subs) in &self.wire_by_event {
            *counts.entry(event.clone()).or_insert(0) += subs.len();
        }
        counts
    }

    pub fn details(&self) -> HashMap<String, Vec<HashMap<String, String>>> {
        let mut result: HashMap<String, Vec<HashMap<String, String>>> = HashMap::new();
        for (event, hooks) in &self.by_event {
            let entries = result.entry(event.clone()).or_default();
            for h in hooks {
                let mut entry = HashMap::new();
                entry.insert("matcher".to_string(), h.matcher.clone().unwrap_or_default());
                entry.insert("source".to_string(), "server".to_string());
                entry.insert("command".to_string(), h.command.clone());
                entries.push(entry);
            }
        }
        for (event, subs) in &self.wire_by_event {
            let entries = result.entry(event.clone()).or_default();
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
    pub async fn trigger(
        &self,
        event: &str,
        matcher_value: &str,
        input_data: HashMap<String, Value>,
    ) -> Vec<HookResult> {
        // Match server-side hooks
        let mut seen_commands: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut server_matched: Vec<&HookDef> = Vec::new();
        for h in self.by_event.get(event).into_iter().flatten() {
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
            .get(event)
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
            cb(event, matcher_value, total);
        }

        let t0 = std::time::Instant::now();
        let mut tasks: Vec<tokio::task::JoinHandle<HookResult>> = Vec::new();

        // Server-side: run shell commands
        for h in server_matched {
            let command = h.command.clone();
            let input = input_data.clone();
            let timeout = h.timeout;
            let cwd = self.cwd.clone();
            tasks.push(tokio::spawn(async move {
                run_hook(&command, &input, timeout, cwd.as_deref()).await
            }));
        }

        // Wire-side: stubbed for now (no real wire client)
        for _s in wire_matched {
            tasks.push(tokio::spawn(async move {
                // Wire hooks are not yet supported without a real wire client.
                // Fail open.
                HookResult::allow()
            }));
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
            cb(event, matcher_value, action, duration_ms);
        }

        results
    }

    /// Trigger a hook in the background (fire-and-forget).
    ///
    /// The returned task must be awaited or stored; otherwise the hook
    /// may be cancelled when the caller drops the handle.
    pub fn fire_and_forget_trigger(
        &self,
        event: &str,
        matcher_value: &str,
        input_data: HashMap<String, Value>,
    ) -> tokio::task::JoinHandle<Vec<HookResult>> {
        let event = event.to_string();
        let matcher_value = matcher_value.to_string();
        let engine = self.clone();
        tokio::spawn(async move { engine.trigger(&event, &matcher_value, input_data).await })
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
