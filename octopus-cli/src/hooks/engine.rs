use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use regex::Regex;

use crate::config::HookDef;
use crate::hooks::event::{HookEvent, HookEventKind};
use crate::hooks::hook::{
    CommandHook, Hook, HookCallbacks, HookRunContext, OnWireHookDone, WireHook,
    WireHookSubscription,
};
use crate::hooks::runner::{HookAction, HookResult};

/// Loads hook definitions and executes matching hooks in parallel.
///
/// Supports two hook sources:
/// - Server-side (config.toml): shell commands executed locally
/// - Client-side (wire subscriptions): forwarded to client via HookRequest
pub struct HookEngine {
    by_event: HashMap<HookEventKind, Vec<Box<dyn Hook>>>,
    cwd: Option<PathBuf>,
    callbacks: HookCallbacks,
}

impl HookEngine {
    pub fn new(hooks: Vec<HookDef>) -> Self {
        let mut engine = Self {
            by_event: HashMap::new(),
            cwd: None,
            callbacks: HookCallbacks::default(),
        };
        engine.add_hooks(hooks);
        engine
    }

    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    pub fn set_callbacks(&mut self, callbacks: HookCallbacks) {
        self.callbacks = callbacks;
    }

    pub fn set_on_wire_hook_done(&mut self, cb: Option<OnWireHookDone>) {
        self.callbacks.on_wire_hook_done = cb;
    }

    pub fn add_hooks(&mut self, hooks: Vec<HookDef>) {
        for mut def in hooks {
            if def.compiled_matcher.is_none() {
                def.compile_matcher();
            }
            let hook = Box::new(CommandHook::new(&def));
            self.by_event.entry(hook.kind()).or_default().push(hook);
        }
    }

    pub fn add_wire_subscriptions(&mut self, mut subs: Vec<WireHookSubscription>) {
        for s in &mut subs {
            if s.compiled_matcher.is_none() {
                s.compiled_matcher = Regex::new(&s.matcher).ok();
            }
        }
        for s in subs {
            let hook = Box::new(WireHook::new(&s));
            self.by_event.entry(hook.kind()).or_default().push(hook);
        }
    }

    pub fn has_hooks(&self) -> bool {
        self.by_event.values().any(|v| !v.is_empty())
    }

    pub fn has_hooks_for(&self, kind: HookEventKind) -> bool {
        self.by_event.contains_key(&kind)
    }

    pub fn summary(&self) -> HashMap<String, usize> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for hook in self.by_event.values().flatten() {
            *counts.entry(hook.kind().to_string()).or_insert(0) += 1;
        }
        counts
    }

    pub fn details(&self) -> HashMap<String, Vec<HashMap<String, String>>> {
        let mut result: HashMap<String, Vec<HashMap<String, String>>> = HashMap::new();
        for hook in self.by_event.values().flatten() {
            let entries = result.entry(hook.kind().to_string()).or_default();
            let mut entry = HashMap::new();
            entry.insert(
                "matcher".to_string(),
                hook.matcher()
                    .map(|re| re.as_str().to_string())
                    .unwrap_or_default(),
            );
            entry.insert("source".to_string(), hook.source().to_string());
            if let Some(cmd) = hook.command() {
                entry.insert("command".to_string(), cmd.to_string());
            }
            entries.push(entry);
        }
        result
    }

    /// Run all matching hooks (server + wire) in parallel.
    pub async fn trigger(&self, event: HookEvent) -> Vec<HookResult> {
        let kind = event.kind();
        let matcher_value = event.matcher_value().unwrap_or("").to_string();
        let event = Arc::new(event);

        let mut seen_commands: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut matched: Vec<&Box<dyn Hook>> = Vec::new();
        for h in self.by_event.get(&kind).into_iter().flatten() {
            // Server-side hooks are deduplicated by command string.
            if let Some(cmd) = h.command() {
                if seen_commands.contains(cmd) {
                    continue;
                }
                seen_commands.insert(cmd.to_string());
            }
            match h.matcher() {
                None => matched.push(h),
                Some(re) if re.is_match(&matcher_value) => matched.push(h),
                _ => {}
            }
        }

        let total = matched.len();
        if total == 0 {
            return Vec::new();
        }

        if let Some(ref cb) = self.callbacks.on_triggered {
            cb(&event, &matcher_value, total);
        }

        let t0 = std::time::Instant::now();
        let mut tasks: Vec<tokio::task::JoinHandle<HookResult>> = Vec::new();

        let ctx = HookRunContext {
            cwd: self.cwd.clone(),
            callbacks: self.callbacks.clone(),
        };

        for hook in matched {
            let hook = hook.clone_box();
            let event = Arc::clone(&event);
            let ctx = ctx.clone();
            tasks.push(tokio::spawn(async move { hook.run(&event, &ctx).await }));
        }

        let results: Vec<HookResult> = match futures::future::try_join_all(tasks).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Hook engine task join error for {}: {}", kind, e);
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
                    kind,
                    matcher_value,
                    reason
                );
                break;
            }
        }

        if let Some(ref cb) = self.callbacks.on_resolved {
            cb(&event, &matcher_value, action, duration_ms);
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
    ) -> tokio::task::JoinHandle<Vec<HookResult>> {
        let engine = self.clone();
        tokio::spawn(async move { engine.trigger(event).await })
    }
}

impl Clone for HookEngine {
    fn clone(&self) -> Self {
        let mut by_event = HashMap::new();
        for (kind, hooks) in &self.by_event {
            let cloned = hooks.iter().map(|h| h.clone_box()).collect();
            by_event.insert(*kind, cloned);
        }
        Self {
            by_event,
            cwd: self.cwd.clone(),
            callbacks: self.callbacks.clone(),
        }
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

    /// Build a HookDef the same way config deserialization does:
    /// `event = "PreToolUse"` parses into `HookEventKind::PreToolUse`.
    fn config_hook_def(kind: HookEventKind, command: &str, matcher: Option<&str>) -> HookDef {
        let mut def = HookDef {
            event: kind,
            matcher: matcher.map(|s| s.to_string()),
            compiled_matcher: None,
            command: command.to_string(),
            timeout: 30,
        };
        def.compile_matcher();
        def
    }

    #[test]
    fn test_config_event_matches_runtime_event() {
        let def = config_hook_def(HookEventKind::PreToolUse, "echo ok", None);
        let engine = HookEngine::new(vec![def]);

        let runtime_event = HookEvent::pre_tool_use(
            "real-sess",
            "/real/cwd",
            "WriteFile",
            &HashMap::new(),
            "call-42",
        );

        assert!(
            engine.has_hooks_for(runtime_event.kind()),
            "config hook should match runtime event with same kind"
        );
    }

    #[test]
    fn test_different_events_do_not_match() {
        let engine = HookEngine::new(vec![config_hook_def(
            HookEventKind::PreToolUse,
            "echo ok",
            None,
        )]);

        let runtime_event = HookEvent::post_tool_use(
            "real-sess",
            "/real/cwd",
            "WriteFile",
            &HashMap::new(),
            "done",
            "call-42",
        );

        assert!(
            !engine.has_hooks_for(runtime_event.kind()),
            "PreToolUse config hook should not match PostToolUse runtime event"
        );
    }

    #[test]
    fn test_summary_counts_by_kind() {
        let hooks = vec![
            config_hook_def(HookEventKind::PreToolUse, "echo 1", None),
            config_hook_def(HookEventKind::PreToolUse, "echo 2", None),
            config_hook_def(HookEventKind::PostToolUse, "echo 3", None),
        ];
        let engine = HookEngine::new(hooks);
        let summary = engine.summary();
        assert_eq!(summary.get("PreToolUse"), Some(&2));
        assert_eq!(summary.get("PostToolUse"), Some(&1));
    }

    #[test]
    fn test_matcher_filters_hooks() {
        let hooks = vec![
            config_hook_def(HookEventKind::PreToolUse, "echo A", Some("Read.*")),
            config_hook_def(HookEventKind::PreToolUse, "echo B", Some("Write.*")),
        ];
        let engine = HookEngine::new(hooks);

        let write_event = HookEvent::pre_tool_use("s", "/", "WriteFile", &HashMap::new(), "c");

        let matched = engine
            .by_event
            .get(&HookEventKind::PreToolUse)
            .unwrap()
            .iter()
            .filter(|h| match h.matcher() {
                None => true,
                Some(re) => re.is_match(write_event.matcher_value().unwrap_or("")),
            })
            .count();
        assert_eq!(matched, 1);
    }

    #[test]
    fn test_duplicate_commands_are_deduplicated() {
        let hooks = vec![
            config_hook_def(HookEventKind::PreToolUse, "echo same", None),
            config_hook_def(HookEventKind::PreToolUse, "echo same", None),
            config_hook_def(HookEventKind::PreToolUse, "echo different", None),
        ];
        let engine = HookEngine::new(hooks);

        let event = HookEvent::pre_tool_use("s", "/", "WriteFile", &HashMap::new(), "c");
        let results = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(engine.trigger(event));

        // "echo same" should run once due to deduplication;
        // "echo different" should run once.
        assert_eq!(results.len(), 2);
    }
}
