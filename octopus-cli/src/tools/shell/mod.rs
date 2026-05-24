use std::process::Stdio;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use crate::tools::Tool;

const MAX_FOREGROUND_TIMEOUT: u64 = 5 * 60;
const _MAX_BACKGROUND_TIMEOUT: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellParams {
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub run_in_background: bool,
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
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "Shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "Shell",
            "description": "Execute a shell command. Use bash syntax. Commands run in the working directory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute."
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds",
                        "default": 60
                    },
                    "run_in_background": {
                        "type": "boolean",
                        "description": "Run as a background task",
                        "default": false
                    },
                    "description": {
                        "type": "string",
                        "description": "Description for background task (required if run_in_background=true)",
                        "default": ""
                    }
                },
                "required": ["command"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: ShellParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        if params.command.is_empty() {
            return Err("Command cannot be empty.".to_string());
        }

        if params.run_in_background {
            let task_id = self
                .bg_manager
                .spawn(params.command.clone(), params.description.clone())
                .await?;
            return Ok(format!(
                "Background task started: `{}`\ntask_id: {}\nautomatic_notification: true\nnext_step: You will be automatically notified when it completes.",
                params.command, task_id
            ));
        }

        let timeout_secs = params.timeout.min(MAX_FOREGROUND_TIMEOUT);

        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(&params.command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn command: {}", e))?;

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
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                let _ = child.kill().await;
                return Err(format!("Command killed by timeout ({}s)", timeout_secs));
            }
        }

        let status = child
            .wait()
            .await
            .map_err(|e| format!("Failed to wait for command: {}", e))?;

        if status.success() {
            Ok(output)
        } else {
            let code = status.code().unwrap_or(-1);
            Err(format!(
                "Command failed with exit code {}.\nOutput:\n{}",
                code, output
            ))
        }
    }
}
