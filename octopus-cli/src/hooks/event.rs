use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

/// Every extension point in the system where external hooks may intervene.
///
/// **Equality and hashing are based only on the discriminant (the variant),**
/// not on the payload data. This allows the same type to be used both as a
/// typed message payload and as a `HashMap` key in the engine.
#[derive(Debug, Clone, Serialize)]
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
    SessionStart {
        session_id: String,
        cwd: String,
        source: String,
    },
    SessionEnd {
        session_id: String,
        cwd: String,
        reason: String,
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

// Discriminant-only equality so HookEvent can be used as a HashMap key.
impl PartialEq for HookEvent {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Eq for HookEvent {}

impl std::hash::Hash for HookEvent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
    }
}

impl std::fmt::Display for HookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            HookEvent::PreToolUse { .. } => "PreToolUse",
            HookEvent::PostToolUse { .. } => "PostToolUse",
            HookEvent::PostToolUseFailure { .. } => "PostToolUseFailure",
            HookEvent::UserPromptSubmit { .. } => "UserPromptSubmit",
            HookEvent::Stop { .. } => "Stop",
            HookEvent::StopFailure { .. } => "StopFailure",
            HookEvent::SessionStart { .. } => "SessionStart",
            HookEvent::SessionEnd { .. } => "SessionEnd",
            HookEvent::PreCompact { .. } => "PreCompact",
            HookEvent::PostCompact { .. } => "PostCompact",
            HookEvent::Notification { .. } => "Notification",
        };
        write!(f, "{}", s)
    }
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

    pub fn session_start(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        HookEvent::SessionStart {
            session_id: session_id.into(),
            cwd: cwd.into(),
            source: source.into(),
        }
    }

    pub fn session_end(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        HookEvent::SessionEnd {
            session_id: session_id.into(),
            cwd: cwd.into(),
            reason: reason.into(),
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
}

/// Serde helpers so `HookEvent` can be stored as just a string in config files
/// (`event = "PreToolUse"`) while still serializing to the full JSON payload
/// when sent to hook scripts.
pub mod discriminant_serde {
    use super::HookEvent;
    use serde::Deserialize;
    use std::collections::HashMap;

    pub fn serialize<S>(event: &HookEvent, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&event.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HookEvent, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "PreToolUse" => Ok(HookEvent::PreToolUse {
                session_id: String::new(),
                cwd: String::new(),
                tool_name: String::new(),
                tool_input: HashMap::new(),
                tool_call_id: String::new(),
            }),
            "PostToolUse" => Ok(HookEvent::PostToolUse {
                session_id: String::new(),
                cwd: String::new(),
                tool_name: String::new(),
                tool_input: HashMap::new(),
                tool_output: String::new(),
                tool_call_id: String::new(),
            }),
            "PostToolUseFailure" => Ok(HookEvent::PostToolUseFailure {
                session_id: String::new(),
                cwd: String::new(),
                tool_name: String::new(),
                tool_input: HashMap::new(),
                error: String::new(),
                tool_call_id: String::new(),
            }),
            "UserPromptSubmit" => Ok(HookEvent::UserPromptSubmit {
                session_id: String::new(),
                cwd: String::new(),
                prompt: String::new(),
            }),
            "Stop" => Ok(HookEvent::Stop {
                session_id: String::new(),
                cwd: String::new(),
                stop_hook_active: false,
            }),
            "StopFailure" => Ok(HookEvent::StopFailure {
                session_id: String::new(),
                cwd: String::new(),
                error_type: String::new(),
                error_message: String::new(),
            }),
            "SessionStart" => Ok(HookEvent::SessionStart {
                session_id: String::new(),
                cwd: String::new(),
                source: String::new(),
            }),
            "SessionEnd" => Ok(HookEvent::SessionEnd {
                session_id: String::new(),
                cwd: String::new(),
                reason: String::new(),
            }),
            "PreCompact" => Ok(HookEvent::PreCompact {
                session_id: String::new(),
                cwd: String::new(),
                trigger: String::new(),
                token_count: 0,
            }),
            "PostCompact" => Ok(HookEvent::PostCompact {
                session_id: String::new(),
                cwd: String::new(),
                trigger: String::new(),
                estimated_token_count: 0,
            }),
            "Notification" => Ok(HookEvent::Notification {
                session_id: String::new(),
                cwd: String::new(),
                sink: String::new(),
                notification_type: String::new(),
                title: String::new(),
                body: String::new(),
                severity: String::new(),
            }),
            _ => Err(D::Error::custom(format!("unknown hook event: {}", s))),
        }
    }
}
