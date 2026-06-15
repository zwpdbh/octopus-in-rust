use std::process::Stdio;

use async_trait::async_trait;
use kosong::tooling::{CallableTool2, ToolReturnValue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use crate::tools::ExecutionMode;

const MAX_FOREGROUND_TIMEOUT: u64 = 5 * 60;
const _MAX_BACKGROUND_TIMEOUT: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShellParams {
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    #[serde(default)]
    pub description: String,
}

fn default_timeout() -> u64 {
    60
}

pub struct ShellTool {
    bg_manager: crate::background::BackgroundTaskManager,
}

impl ShellTool {
    pub fn new(bg_manager: crate::background::BackgroundTaskManager) -> Self {
        Self { bg_manager }
    }
}

#[async_trait]
impl CallableTool2 for ShellTool {
    type Params = ShellParams;

    fn name(&self) -> &str {
        "Shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command."
    }

    async fn call_typed(&self, params: ShellParams) -> ToolReturnValue {
        if params.command.is_empty() {
            return ToolReturnValue::error("Command cannot be empty.");
        }

        match params.execution_mode {
            ExecutionMode::Background => {
                let task_id = match self
                    .bg_manager
                    .spawn(params.command.clone(), params.description.clone())
                    .await
                {
                    Ok(id) => id,
                    Err(e) => {
                        return ToolReturnValue::error(format!(
                            "Failed to spawn background task: {}",
                            e
                        ));
                    }
                };
                return ToolReturnValue::ok(format!(
                    "Background task started: `{}`\ntask_id: {}\nautomatic_notification: true\nnext_step: You will be automatically notified when it completes.",
                    params.command, task_id
                ));
            }
            ExecutionMode::Foreground => {}
        }

        let timeout_secs = params.timeout.min(MAX_FOREGROUND_TIMEOUT);

        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(&params.command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return ToolReturnValue::error(format!("Failed to spawn command: {}", e)),
        };

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        let mut output = String::new();

        let result = timeout(Duration::from_secs(timeout_secs), async {
            loop {
                tokio::select! {
                    line = stdout_reader.next_line() => {
                        match line {
                            Ok(Some(l)) => {
                                output.push_str(&l);
                                output.push('\n');
                            }
                            Ok(None) => break,
                            Err(e) => return Err(format!("Error reading stdout: {}", e)),
                        }
                    }
                    line = stderr_reader.next_line() => {
                        match line {
                            Ok(Some(l)) => {
                                output.push_str(&l);
                                output.push('\n');
                            }
                            Ok(None) => break,
                            Err(e) => return Err(format!("Error reading stderr: {}", e)),
                        }
                    }
                }
            }
            Ok(())
        })
        .await;

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return ToolReturnValue::error(e),
            Err(_) => {
                let _ = child.kill().await;
                return ToolReturnValue::error(format!(
                    "Command killed by timeout ({}s)",
                    timeout_secs
                ));
            }
        }

        let status = match child.wait().await {
            Ok(s) => s,
            Err(e) => return ToolReturnValue::error(format!("Failed to wait for command: {}", e)),
        };

        if status.success() {
            ToolReturnValue::ok(output)
        } else {
            let code = status.code().unwrap_or(-1);
            ToolReturnValue::error(format!(
                "Command failed with exit code {}.\nOutput:\n{}",
                code, output
            ))
        }
    }
}
