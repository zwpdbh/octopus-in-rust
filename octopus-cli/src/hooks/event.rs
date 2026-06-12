use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Discriminant-only identifier for a hook event.
///
/// This enum is used as a registry/config key: it carries no runtime data,
/// implements `Hash`/`Eq`, and serializes to just the PascalCase variant name
/// (e.g. `"PreToolUse"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEventKind {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    UserPromptSubmit,
    Stop,
    StopFailure,
    PreCompact,
    PostCompact,
    Notification,
}

impl std::fmt::Display for HookEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            HookEventKind::PreToolUse => "PreToolUse",
            HookEventKind::PostToolUse => "PostToolUse",
            HookEventKind::PostToolUseFailure => "PostToolUseFailure",
            HookEventKind::UserPromptSubmit => "UserPromptSubmit",
            HookEventKind::Stop => "Stop",
            HookEventKind::StopFailure => "StopFailure",
            HookEventKind::PreCompact => "PreCompact",
            HookEventKind::PostCompact => "PostCompact",
            HookEventKind::Notification => "Notification",
        };
        write!(f, "{}", s)
    }
}

/// Concrete runtime payload for a hook event.
///
/// Unlike `HookEventKind`, this enum always carries the full data available at
/// the moment the event fires. It is serialized to the full JSON payload when
/// sent to hook scripts or wire clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "hook_event_name", rename_all = "PascalCase")]
pub enum HookEvent {
    PreToolUse {
        session_id: String,
        cwd: String,
        tool_name: String,
        tool_input: HashMap<String, Value>,
        tool_call_id: String,
    },
    PostToolUse {
        session_id: String,
        cwd: String,
        tool_name: String,
        tool_input: HashMap<String, Value>,
        tool_output: String,
        tool_call_id: String,
    },
    PostToolUseFailure {
        session_id: String,
        cwd: String,
        tool_name: String,
        tool_input: HashMap<String, Value>,
        error: String,
        tool_call_id: String,
    },
    UserPromptSubmit {
        session_id: String,
        cwd: String,
        prompt: String,
    },
    Stop {
        session_id: String,
        cwd: String,
        stop_hook_active: bool,
    },
    StopFailure {
        session_id: String,
        cwd: String,
        error_type: String,
        error_message: String,
    },
    PreCompact {
        session_id: String,
        cwd: String,
        trigger: String,
        token_count: usize,
    },
    PostCompact {
        session_id: String,
        cwd: String,
        trigger: String,
        estimated_token_count: usize,
    },
    Notification {
        session_id: String,
        cwd: String,
        sink: String,
        notification_type: String,
        title: String,
        body: String,
        severity: String,
    },
}

impl HookEvent {
    pub fn pre_tool_use(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        tool_name: impl Into<String>,
        tool_input: &HashMap<String, Value>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        HookEvent::PreToolUse {
            session_id: session_id.into(),
            cwd: cwd.into(),
            tool_name: tool_name.into(),
            tool_input: tool_input.clone(),
            tool_call_id: tool_call_id.into(),
        }
    }

    pub fn post_tool_use(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        tool_name: impl Into<String>,
        tool_input: &HashMap<String, Value>,
        tool_output: impl Into<String>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        HookEvent::PostToolUse {
            session_id: session_id.into(),
            cwd: cwd.into(),
            tool_name: tool_name.into(),
            tool_input: tool_input.clone(),
            tool_output: tool_output.into(),
            tool_call_id: tool_call_id.into(),
        }
    }

    pub fn post_tool_use_failure(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        tool_name: impl Into<String>,
        tool_input: &HashMap<String, Value>,
        error: impl Into<String>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        HookEvent::PostToolUseFailure {
            session_id: session_id.into(),
            cwd: cwd.into(),
            tool_name: tool_name.into(),
            tool_input: tool_input.clone(),
            error: error.into(),
            tool_call_id: tool_call_id.into(),
        }
    }

    pub fn user_prompt_submit(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Self {
        HookEvent::UserPromptSubmit {
            session_id: session_id.into(),
            cwd: cwd.into(),
            prompt: prompt.into(),
        }
    }

    pub fn stop(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        stop_hook_active: bool,
    ) -> Self {
        HookEvent::Stop {
            session_id: session_id.into(),
            cwd: cwd.into(),
            stop_hook_active,
        }
    }

    pub fn stop_failure(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        error_type: impl Into<String>,
        error_message: impl Into<String>,
    ) -> Self {
        HookEvent::StopFailure {
            session_id: session_id.into(),
            cwd: cwd.into(),
            error_type: error_type.into(),
            error_message: error_message.into(),
        }
    }

    pub fn pre_compact(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        trigger: impl Into<String>,
        token_count: usize,
    ) -> Self {
        HookEvent::PreCompact {
            session_id: session_id.into(),
            cwd: cwd.into(),
            trigger: trigger.into(),
            token_count,
        }
    }

    pub fn post_compact(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        trigger: impl Into<String>,
        estimated_token_count: usize,
    ) -> Self {
        HookEvent::PostCompact {
            session_id: session_id.into(),
            cwd: cwd.into(),
            trigger: trigger.into(),
            estimated_token_count,
        }
    }

    pub fn notification(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        sink: impl Into<String>,
        notification_type: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        severity: impl Into<String>,
    ) -> Self {
        HookEvent::Notification {
            session_id: session_id.into(),
            cwd: cwd.into(),
            sink: sink.into(),
            notification_type: notification_type.into(),
            title: title.into(),
            body: body.into(),
            severity: severity.into(),
        }
    }

    /// Return the discriminant identifier for this event.
    pub fn kind(&self) -> HookEventKind {
        match self {
            HookEvent::PreToolUse { .. } => HookEventKind::PreToolUse,
            HookEvent::PostToolUse { .. } => HookEventKind::PostToolUse,
            HookEvent::PostToolUseFailure { .. } => HookEventKind::PostToolUseFailure,
            HookEvent::UserPromptSubmit { .. } => HookEventKind::UserPromptSubmit,
            HookEvent::Stop { .. } => HookEventKind::Stop,
            HookEvent::StopFailure { .. } => HookEventKind::StopFailure,
            HookEvent::PreCompact { .. } => HookEventKind::PreCompact,
            HookEvent::PostCompact { .. } => HookEventKind::PostCompact,
            HookEvent::Notification { .. } => HookEventKind::Notification,
        }
    }

    /// Return the value that should be matched against a hook's regex matcher.
    ///
    /// `None` means the event has no natural matcher field; an empty string is
    /// used in that case.
    pub fn matcher_value(&self) -> Option<&str> {
        match self {
            HookEvent::PreToolUse { tool_name, .. } => Some(tool_name),
            HookEvent::PostToolUse { tool_name, .. } => Some(tool_name),
            HookEvent::PostToolUseFailure { tool_name, .. } => Some(tool_name),
            HookEvent::UserPromptSubmit { prompt, .. } => Some(prompt),
            HookEvent::Stop { .. } => None,
            HookEvent::StopFailure { error_type, .. } => Some(error_type),
            HookEvent::PreCompact { trigger, .. } => Some(trigger),
            HookEvent::PostCompact { trigger, .. } => Some(trigger),
            HookEvent::Notification {
                notification_type, ..
            } => Some(notification_type),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_kind_equality_ignores_payload() {
        let a = HookEvent::PreToolUse {
            session_id: "sess-a".into(),
            cwd: "/a".into(),
            tool_name: "tool-a".into(),
            tool_input: {
                let mut m = HashMap::new();
                m.insert("key".into(), Value::String("val".into()));
                m
            },
            tool_call_id: "call-a".into(),
        };
        let b = HookEvent::PreToolUse {
            session_id: "sess-b".into(),
            cwd: "/b".into(),
            tool_name: "tool-b".into(),
            tool_input: HashMap::new(),
            tool_call_id: "call-b".into(),
        };
        assert_eq!(
            a.kind(),
            b.kind(),
            "same variant should produce same HookEventKind regardless of payload"
        );
    }

    #[test]
    fn test_kind_inequality() {
        let pre = HookEvent::pre_tool_use("s", "/", "t", &HashMap::new(), "c");
        let post = HookEvent::post_tool_use("s", "/", "t", &HashMap::new(), "out", "c");
        assert_ne!(pre.kind(), post.kind());
    }

    #[test]
    fn test_kind_hash_as_map_key() {
        let mut map: HashMap<HookEventKind, usize> = HashMap::new();
        let a = HookEvent::pre_tool_use("s", "/", "t", &HashMap::new(), "c");
        let b = HookEvent::PreToolUse {
            session_id: "other".into(),
            cwd: "/other".into(),
            tool_name: "other".into(),
            tool_input: HashMap::new(),
            tool_call_id: "other".into(),
        };
        map.insert(a.kind(), 42);
        assert_eq!(map.get(&b.kind()), Some(&42));
    }

    #[test]
    fn test_display_outputs_variant_name() {
        let e = HookEventKind::PostToolUseFailure;
        assert_eq!(e.to_string(), "PostToolUseFailure");
    }

    #[test]
    fn test_kind_serde_roundtrip() {
        let original = HookEvent::pre_tool_use(
            "real-session",
            "/real",
            "real-tool",
            &HashMap::new(),
            "real-call",
        );
        let json = serde_json::to_string(&original).unwrap();
        assert!(
            json.contains("real-session"),
            "default serde should serialize full payload: {}",
            json
        );

        // HookEventKind serializes to just the string name
        let kind_json = serde_json::to_string(&original.kind()).unwrap();
        assert_eq!(kind_json, "\"PreToolUse\"");

        // HookEventKind deserializes from just the string name
        let decoded: HookEventKind = serde_json::from_str(&kind_json).unwrap();
        assert_eq!(decoded, original.kind());
    }

    #[test]
    fn test_full_payload_serde_serialize() {
        let mut input = HashMap::new();
        input.insert("path".into(), Value::String("/tmp".into()));
        input.insert("content".into(), Value::String("hello".into()));

        let original = HookEvent::PreToolUse {
            session_id: "sess-1".into(),
            cwd: "/home/user".into(),
            tool_name: "WriteFile".into(),
            tool_input: input.clone(),
            tool_call_id: "call-123".into(),
        };

        let json = serde_json::to_string_pretty(&original).unwrap();
        assert!(json.contains("\"hook_event_name\""));
        assert!(json.contains("PreToolUse"));
        assert!(json.contains("sess-1"));
        assert!(json.contains("call-123"));
        assert!(json.contains("WriteFile"));
    }

    #[test]
    fn test_all_event_variants_deserialize_from_kind() {
        let variants = vec![
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
            "UserPromptSubmit",
            "Stop",
            "StopFailure",
            "PreCompact",
            "PostCompact",
            "Notification",
        ];
        for name in variants {
            let json = format!("\"{}\"", name);
            let kind: HookEventKind = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("failed to deserialize {}: {}", name, e));
            assert_eq!(kind.to_string(), name);
        }
    }

    #[test]
    fn test_invalid_kind_errors() {
        let result: Result<HookEventKind, _> = serde_json::from_str("\"UnknownEvent\"");
        assert!(
            result.is_err(),
            "unknown event kind should fail to deserialize"
        );
    }

    #[test]
    fn test_matcher_value_per_variant() {
        assert_eq!(
            HookEvent::pre_tool_use("s", "/", "WriteFile", &HashMap::new(), "c").matcher_value(),
            Some("WriteFile")
        );
        assert_eq!(
            HookEvent::post_tool_use("s", "/", "ReadFile", &HashMap::new(), "out", "c")
                .matcher_value(),
            Some("ReadFile")
        );
        assert_eq!(
            HookEvent::post_tool_use_failure("s", "/", "Shell", &HashMap::new(), "err", "c")
                .matcher_value(),
            Some("Shell")
        );
        assert_eq!(
            HookEvent::user_prompt_submit("s", "/", "hello").matcher_value(),
            Some("hello")
        );
        assert_eq!(HookEvent::stop("s", "/", false).matcher_value(), None);
        assert_eq!(
            HookEvent::stop_failure("s", "/", "IOError", "msg").matcher_value(),
            Some("IOError")
        );
        assert_eq!(
            HookEvent::pre_compact("s", "/", "budget", 100).matcher_value(),
            Some("budget")
        );
        assert_eq!(
            HookEvent::post_compact("s", "/", "budget", 80).matcher_value(),
            Some("budget")
        );
        assert_eq!(
            HookEvent::notification("s", "/", "llm", "error", "t", "b", "high").matcher_value(),
            Some("error")
        );
    }
}
