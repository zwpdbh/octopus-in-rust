use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Semantic decision produced by a hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action", content = "reason")]
pub enum HookAction {
    Allow,
    Block(String),
}

/// Result of a single hook execution (raw output + semantic decision).
#[derive(Debug, Clone)]
pub struct HookResult {
    pub action: HookAction,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

impl HookResult {
    pub fn allow() -> Self {
        Self {
            action: HookAction::Allow,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            timed_out: false,
        }
    }

    pub fn block(reason: impl Into<String>) -> Self {
        Self {
            action: HookAction::Block(reason.into()),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 2,
            timed_out: false,
        }
    }
}

/// Execute a single hook command. Fail-open: errors/timeouts -> allow.
pub async fn run_hook(
    command: &str,
    input_data: &HashMap<String, Value>,
    timeout_secs: u64,
    cwd: Option<&std::path::Path>,
) -> HookResult {
    let json_input = match serde_json::to_vec(input_data) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Hook failed to serialize input data: {}", e);
            return HookResult::allow();
        }
    };

    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Hook failed to spawn: {}: {}", command, e);
            return HookResult::allow();
        }
    };

    let result = tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async {
        let mut stdin = match child.stdin.take() {
            Some(s) => s,
            None => {
                tracing::warn!("Hook failed to open stdin");
                let _ = child.start_kill();
                return None;
            }
        };

        if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut stdin, &json_input).await {
            tracing::warn!("Hook failed to write stdin: {}", e);
            return None;
        }
        drop(stdin); // Close stdin so the child sees EOF

        match child.wait_with_output().await {
            Ok(o) => Some(o),
            Err(_) => None,
        }
    })
    .await;

    let output = match result {
        Ok(Some(o)) => o,
        Ok(None) => {
            return HookResult::allow();
        }
        Err(_) => {
            tracing::warn!("Hook timed out after {}s: {}", timeout_secs, command);
            return HookResult {
                action: HookAction::Allow,
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: true,
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code().unwrap_or(0);

    // Exit 2 = block
    if exit_code == 2 {
        return HookResult {
            action: HookAction::Block(stderr.trim().to_string()),
            stdout,
            stderr,
            exit_code: 2,
            timed_out: false,
        };
    }

    // Exit 0 + JSON stdout = structured decision
    if exit_code == 0 && !stdout.trim().is_empty() {
        if let Ok(raw) = serde_json::from_str::<Value>(&stdout) {
            if let Some(hook_output) = raw.get("hookSpecificOutput").and_then(|v| v.as_object()) {
                if hook_output
                    .get("permissionDecision")
                    .and_then(|v| v.as_str())
                    == Some("deny")
                {
                    let reason = hook_output
                        .get("permissionDecisionReason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return HookResult {
                        action: HookAction::Block(reason),
                        stdout,
                        stderr,
                        exit_code: 0,
                        timed_out: false,
                    };
                }
            }
        }
    }

    HookResult {
        action: HookAction::Allow,
        stdout,
        stderr,
        exit_code,
        timed_out: false,
    }
}
