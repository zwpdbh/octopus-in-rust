use std::collections::HashMap;

use serde_json::Value;

fn base(event: &str, session_id: &str, cwd: &str) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    m.insert(
        "hook_event_name".to_string(),
        Value::String(event.to_string()),
    );
    m.insert(
        "session_id".to_string(),
        Value::String(session_id.to_string()),
    );
    m.insert("cwd".to_string(), Value::String(cwd.to_string()));
    m
}

pub fn pre_tool_use(
    session_id: &str,
    cwd: &str,
    tool_name: &str,
    tool_input: &HashMap<String, Value>,
    tool_call_id: &str,
) -> HashMap<String, Value> {
    let mut m = base("PreToolUse", session_id, cwd);
    m.insert(
        "tool_name".to_string(),
        Value::String(tool_name.to_string()),
    );
    m.insert(
        "tool_input".to_string(),
        Value::Object(tool_input.clone().into_iter().collect()),
    );
    m.insert(
        "tool_call_id".to_string(),
        Value::String(tool_call_id.to_string()),
    );
    m
}

pub fn post_tool_use(
    session_id: &str,
    cwd: &str,
    tool_name: &str,
    tool_input: &HashMap<String, Value>,
    tool_output: &str,
    tool_call_id: &str,
) -> HashMap<String, Value> {
    let mut m = base("PostToolUse", session_id, cwd);
    m.insert(
        "tool_name".to_string(),
        Value::String(tool_name.to_string()),
    );
    m.insert(
        "tool_input".to_string(),
        Value::Object(tool_input.clone().into_iter().collect()),
    );
    m.insert(
        "tool_output".to_string(),
        Value::String(tool_output.to_string()),
    );
    m.insert(
        "tool_call_id".to_string(),
        Value::String(tool_call_id.to_string()),
    );
    m
}

pub fn post_tool_use_failure(
    session_id: &str,
    cwd: &str,
    tool_name: &str,
    tool_input: &HashMap<String, Value>,
    error: &str,
    tool_call_id: &str,
) -> HashMap<String, Value> {
    let mut m = base("PostToolUseFailure", session_id, cwd);
    m.insert(
        "tool_name".to_string(),
        Value::String(tool_name.to_string()),
    );
    m.insert(
        "tool_input".to_string(),
        Value::Object(tool_input.clone().into_iter().collect()),
    );
    m.insert("error".to_string(), Value::String(error.to_string()));
    m.insert(
        "tool_call_id".to_string(),
        Value::String(tool_call_id.to_string()),
    );
    m
}

pub fn user_prompt_submit(session_id: &str, cwd: &str, prompt: &str) -> HashMap<String, Value> {
    let mut m = base("UserPromptSubmit", session_id, cwd);
    m.insert("prompt".to_string(), Value::String(prompt.to_string()));
    m
}

pub fn stop(session_id: &str, cwd: &str, stop_hook_active: bool) -> HashMap<String, Value> {
    let mut m = base("Stop", session_id, cwd);
    m.insert(
        "stop_hook_active".to_string(),
        Value::Bool(stop_hook_active),
    );
    m
}

pub fn stop_failure(
    session_id: &str,
    cwd: &str,
    error_type: &str,
    error_message: &str,
) -> HashMap<String, Value> {
    let mut m = base("StopFailure", session_id, cwd);
    m.insert(
        "error_type".to_string(),
        Value::String(error_type.to_string()),
    );
    m.insert(
        "error_message".to_string(),
        Value::String(error_message.to_string()),
    );
    m
}

pub fn session_start(session_id: &str, cwd: &str, source: &str) -> HashMap<String, Value> {
    let mut m = base("SessionStart", session_id, cwd);
    m.insert("source".to_string(), Value::String(source.to_string()));
    m
}

pub fn session_end(session_id: &str, cwd: &str, reason: &str) -> HashMap<String, Value> {
    let mut m = base("SessionEnd", session_id, cwd);
    m.insert("reason".to_string(), Value::String(reason.to_string()));
    m
}

pub fn pre_compact(
    session_id: &str,
    cwd: &str,
    trigger: &str,
    token_count: usize,
) -> HashMap<String, Value> {
    let mut m = base("PreCompact", session_id, cwd);
    m.insert("trigger".to_string(), Value::String(trigger.to_string()));
    m.insert("token_count".to_string(), Value::Number(token_count.into()));
    m
}

pub fn post_compact(
    session_id: &str,
    cwd: &str,
    trigger: &str,
    estimated_token_count: usize,
) -> HashMap<String, Value> {
    let mut m = base("PostCompact", session_id, cwd);
    m.insert("trigger".to_string(), Value::String(trigger.to_string()));
    m.insert(
        "estimated_token_count".to_string(),
        Value::Number(estimated_token_count.into()),
    );
    m
}

pub fn notification(
    session_id: &str,
    cwd: &str,
    sink: &str,
    notification_type: &str,
    title: &str,
    body: &str,
    severity: &str,
) -> HashMap<String, Value> {
    let mut m = base("Notification", session_id, cwd);
    m.insert("sink".to_string(), Value::String(sink.to_string()));
    m.insert(
        "notification_type".to_string(),
        Value::String(notification_type.to_string()),
    );
    m.insert("title".to_string(), Value::String(title.to_string()));
    m.insert("body".to_string(), Value::String(body.to_string()));
    m.insert("severity".to_string(), Value::String(severity.to_string()));
    m
}
