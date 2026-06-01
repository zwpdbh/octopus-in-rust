use serde::{Deserialize, Serialize};

/// Every extension point in the system where external hooks may intervene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    UserPromptSubmit,
    Stop,
    StopFailure,
    SessionStart,
    SessionEnd,
    PreCompact,
    PostCompact,
    Notification,
}

impl std::fmt::Display for HookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::PostToolUseFailure => "PostToolUseFailure",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::Stop => "Stop",
            HookEvent::StopFailure => "StopFailure",
            HookEvent::SessionStart => "SessionStart",
            HookEvent::SessionEnd => "SessionEnd",
            HookEvent::PreCompact => "PreCompact",
            HookEvent::PostCompact => "PostCompact",
            HookEvent::Notification => "Notification",
        };
        write!(f, "{}", s)
    }
}
