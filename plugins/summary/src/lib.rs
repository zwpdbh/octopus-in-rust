use serde::{Deserialize, Serialize};
use serde_json;
use std::cell::RefCell;
use std::collections::HashMap;

// Legacy ABI (kept for deterministic commands like /status and /help):
//
// Host calls:
//   init() -> i32
//   on_message(event_ptr, event_len, out_ptr, out_cap) -> i32 (bytes written)
//   on_command(cmd_ptr, cmd_len, event_ptr, event_len, out_ptr, out_cap) -> i32
//
// Output is a UTF-8 JSON array of actions written into the host-provided buffer.

// New Brain tool ABI:
//
// Host calls:
//   register_tools(out_ptr, out_cap) -> i32
//   execute(input_ptr, input_len, out_ptr, out_cap) -> i32
//
// `register_tools` returns a JSON array of ToolDef.
// `execute` receives JSON { "tool": "...", "arguments": {...} } and returns
// a JSON string result.

#[derive(Debug, Clone, Deserialize)]
struct Event {
    #[serde(rename = "post_type")]
    post_type: String,
    #[serde(rename = "message_type")]
    message_type: Option<String>,
    #[serde(rename = "group_id")]
    group_id: Option<i64>,
    #[serde(rename = "user_id")]
    user_id: Option<i64>,
    message: Option<String>,
    raw_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum Action {
    #[serde(rename = "send_group_msg")]
    SendGroupMsg { group_id: i64, text: String },
    #[serde(rename = "log")]
    Log { level: String, message: String },
    #[serde(rename = "llm_request")]
    LlmRequest { group_id: i64, prompt: String },
}

thread_local! {
    static BUFFERS: RefCell<HashMap<i64, Vec<Message>>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Clone)]
struct Message {
    user_id: i64,
    text: String,
}

fn parse_event(ptr: *const u8, len: usize) -> Option<Event> {
    if ptr.is_null() || len == 0 {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    serde_json::from_slice(bytes).ok()
}

fn extract_text(event: &Event) -> String {
    event
        .message
        .clone()
        .or_else(|| event.raw_message.clone())
        .unwrap_or_default()
}

fn push_message(group_id: i64, user_id: i64, text: String) {
    BUFFERS.with(|b| {
        let mut map = b.borrow_mut();
        map.entry(group_id)
            .or_default()
            .push(Message { user_id, text });
    });
}

fn build_summary(group_id: i64) -> String {
    BUFFERS.with(|b| {
        let map = b.borrow();
        let msgs = match map.get(&group_id) {
            Some(v) if !v.is_empty() => v,
            _ => return "No messages to summarize yet.".to_string(),
        };
        let mut lines = vec!["Recent conversation:".to_string()];
        for m in msgs.iter().rev().take(50) {
            lines.push(format!("{}: {}", m.user_id, m.text));
        }
        lines.push("\nPlease summarize the above conversation.".to_string());
        lines.join("\n")
    })
}

fn write_output(actions: &[Action], out_ptr: *mut u8, out_cap: usize) -> i32 {
    let json = match serde_json::to_vec(actions) {
        Ok(v) => v,
        Err(_) => return -1,
    };
    if json.len() > out_cap {
        return -2;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(json.as_ptr(), out_ptr, json.len());
    }
    json.len() as i32
}

#[no_mangle]
pub extern "C" fn init() -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn on_message(
    event_ptr: *const u8,
    event_len: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> i32 {
    let event = match parse_event(event_ptr, event_len) {
        Some(e) => e,
        None => return write_output(&[], out_ptr, out_cap),
    };

    if event.post_type != "message" {
        return write_output(&[], out_ptr, out_cap);
    }

    if event.message_type.as_deref() == Some("group") {
        if let (Some(group_id), Some(user_id)) = (event.group_id, event.user_id) {
            let text = extract_text(&event);
            if !text.is_empty() {
                push_message(group_id, user_id, text);
            }
        }
    }

    write_output(&[], out_ptr, out_cap)
}

#[no_mangle]
pub extern "C" fn on_command(
    cmd_ptr: *const u8,
    cmd_len: usize,
    event_ptr: *const u8,
    _event_len: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> i32 {
    let cmd = if cmd_ptr.is_null() || cmd_len == 0 {
        String::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(cmd_ptr, cmd_len) };
        String::from_utf8_lossy(bytes).into_owned()
    };

    let event = match parse_event(event_ptr, _event_len) {
        Some(e) => e,
        None => return write_output(&[], out_ptr, out_cap),
    };

    let group_id = match event.group_id {
        Some(id) => id,
        None => return write_output(&[], out_ptr, out_cap),
    };

    let actions = match cmd.trim() {
        "summary" | "s" => {
            // Legacy path: not used when the Brain handles /summary, but kept
            // for hosts that do not yet use the Brain tool ABI.
            let prompt = build_summary(group_id);
            vec![
                Action::Log {
                    level: "info".to_string(),
                    message: format!("Summarizing group {}", group_id),
                },
                Action::SendGroupMsg {
                    group_id,
                    text: "Generating summary, please wait...".to_string(),
                },
                Action::LlmRequest { group_id, prompt },
            ]
        }
        "status" => {
            let count = BUFFERS.with(|b| {
                let map = b.borrow();
                map.get(&group_id).map(|v| v.len()).unwrap_or(0)
            });
            vec![Action::SendGroupMsg {
                group_id,
                text: format!("Buffered {} messages in this group.", count),
            }]
        }
        "help" => vec![Action::SendGroupMsg {
            group_id,
            text: "Available commands: /summary (or /s), /status, /help".to_string(),
        }],
        _ => vec![],
    };

    write_output(&actions, out_ptr, out_cap)
}

#[no_mangle]
pub extern "C" fn malloc(size: usize) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    let layout = match std::alloc::Layout::from_size_align(size, 1) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };
    unsafe { std::alloc::alloc(layout) }
}

#[no_mangle]
pub extern "C" fn free(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    let layout = match std::alloc::Layout::from_size_align(size, 1) {
        Ok(l) => l,
        Err(_) => return,
    };
    unsafe { std::alloc::dealloc(ptr, layout) }
}

// ============================================================================
// Brain tool ABI (Extism PDK)
// ============================================================================

use extism_pdk::*;

#[derive(Debug, Clone, Deserialize)]
struct FormatConversationArgs {
    /// Raw conversation text, typically one message per line (`user_id: text`).
    messages: String,
    /// Desired style: "bullet" (default) or "paragraph".
    #[serde(default)]
    style: String,
}

fn format_conversation(args: FormatConversationArgs) -> String {
    let lines: Vec<&str> = args.messages.lines().collect();
    if lines.is_empty() {
        return "(no messages provided)".to_string();
    }

    match args.style.as_str() {
        "paragraph" => {
            let joined = lines.join("; ");
            format!("Conversation summary: {}", joined)
        }
        _ => {
            let mut out = "Recent conversation:\n".to_string();
            for line in lines.iter().rev().take(50) {
                out.push_str(line);
                out.push('\n');
            }
            out
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ToolDef {
    name: String,
    description: String,
    prompt_fragment: Option<String>,
    parameters: serde_json::Value,
}

#[plugin_fn]
pub fn register_tools(_input: String) -> FnResult<String> {
    let tools = vec![ToolDef {
        name: "summary_format_conversation".to_string(),
        description: "Format a raw conversation log for summarization.".to_string(),
        prompt_fragment: Some(
            "You may also use summary_format_conversation to format the raw conversation before summarizing.".to_string(),
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "messages": {
                    "type": "string",
                    "description": "Raw conversation text, one message per line."
                },
                "style": {
                    "type": "string",
                    "description": "Output style: 'bullet' or 'paragraph'.",
                    "default": "bullet"
                }
            },
            "required": ["messages"]
        }),
    }];

    Ok(serde_json::to_string(&tools)?)
}

#[derive(Debug, Clone, Deserialize)]
struct ExecuteInput {
    tool: String,
    arguments: FormatConversationArgs,
}

#[plugin_fn]
pub fn execute(input: String) -> FnResult<String> {
    if input.is_empty() {
        return Ok("{\"error\":\"empty input\"}".to_string());
    }

    let parsed: ExecuteInput = match serde_json::from_str(&input) {
        Ok(i) => i,
        Err(e) => {
            return Ok(format!("{{\"error\":\"invalid input: {}\"}}", e));
        }
    };

    let result = match parsed.tool.as_str() {
        "summary_format_conversation" => format_conversation(parsed.arguments),
        _ => format!("Unknown tool: {}", parsed.tool),
    };

    Ok(serde_json::to_string(&result)?)
}
