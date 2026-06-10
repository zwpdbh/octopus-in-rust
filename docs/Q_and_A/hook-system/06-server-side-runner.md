# 6. Server-Side Runner

The `run_hook()` function in `src/hooks/runner.rs` is the bridge between the Rust `HookEngine` and arbitrary external programs. It turns a hook configuration into a subprocess, feeds it JSON, and interprets the result using typed structs.

## 6.1 Signature

```rust
pub async fn run_hook(
    command: &str,
    event: &HookEvent,
    timeout_secs: u64,
    cwd: Option<&std::path::Path>,
) -> HookResult
```

| Parameter | Description |
|-----------|-------------|
| `command` | The shell command string from `HookDef.command`. |
| `event` | The typed `HookEvent` payload (serialized to JSON on stdin). |
| `timeout_secs` | Maximum seconds to wait for the subprocess. |
| `cwd` | Working directory for the subprocess. |

## 6.2 Subprocess Creation

```rust
// octopus-cli/src/hooks/runner.rs ~line 75 — Subprocess creation
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
```

**Important:** `Command::new("sh").arg("-c").arg(command)` passes the command string to `/bin/sh -c`. This allows shell pipes, environment variables, and relative paths — but it also means shell metacharacters in `command` are interpreted. The `command` comes verbatim from `config.toml`; no tool data is interpolated.

**Python comparison:** Python used `asyncio.create_subprocess_shell(command, ...)` — functionally identical. Both run the command through a shell.

## 6.3 Communication Protocol

The runner writes JSON to stdin and reads stdout/stderr:

```rust
// octopus-cli/src/hooks/runner.rs ~line 67 — Communication protocol
let json_input = match serde_json::to_vec(event) {
    Ok(v) => v,
    Err(e) => {
        tracing::warn!("Hook failed to serialize event: {}", e);
        return HookResult::allow();
    }
};
// ...
let mut stdin = match child.stdin.take() {
    Some(s) => s,
    None => {
        tracing::warn!("Hook failed to open stdin");
        let _ = child.start_kill();
        return HookResult::allow();
    }
};

if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut stdin, &json_input).await {
    tracing::warn!("Hook failed to write stdin: {}", e);
    return HookResult::allow();
}
drop(stdin); // Close stdin so the child sees EOF
```

### stdin

A single JSON object, produced by serializing the `HookEvent` enum:

```json
{"hook_event_name":"PreToolUse","session_id":"sess_abc","cwd":"/home/user","tool_name":"shell","tool_input":{"command":"ls"},"tool_call_id":"call_xyz"}
```

### stdout

Expected to be valid JSON when exit code is 0. The Rust runner deserializes it into a typed struct:

```rust
// octopus-cli/src/hooks/runner.rs ~line 6 — Typed stdout structs
#[derive(Debug, Deserialize)]
struct HookStdout {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: Option<HookSpecificOutput>,
}

#[derive(Debug, Deserialize)]
struct HookSpecificOutput {
    #[serde(rename = "permissionDecision")]
    permission_decision: Option<String>,
    #[serde(rename = "permissionDecisionReason")]
    permission_decision_reason: Option<String>,
}
```

**Python comparison:** The Python runner manually indexed a `dict`:

```python
parsed = json.loads(stdout)
if parsed.get("hookSpecificOutput", {}).get("permissionDecision") == "deny":
    ...
```

The Rust approach is strictly better:
- A typo in `#[serde(rename = ...)]` is a compile error.
- Adding a new required field forces updates at every construction site.
- The IDE can autocomplete field names.

### stderr

Used as the `reason` string when `exit_code == 2`.

## 6.4 Decision Logic

```rust
// octopus-cli/src/hooks/runner.rs ~line 134 — Decision logic
let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
let exit_code = output.status.code().unwrap_or(0);

// Exit 2 = block (reason from stderr)
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
    if let Ok(parsed) = serde_json::from_str::<HookStdout>(&stdout) {
        if let Some(ref output) = parsed.hook_specific_output {
            if output.permission_decision.as_deref() == Some("deny") {
                let reason = output
                    .permission_decision_reason
                    .clone()
                    .unwrap_or_default();
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

// Everything else = allow (fail-open)
HookResult {
    action: HookAction::Allow,
    stdout,
    stderr,
    exit_code,
    timed_out: false,
}
```

### Exit Code Semantics

| Exit Code | Meaning | Typical Use |
|-----------|---------|-------------|
| `0` | Success. Check stdout for `permissionDecision`. | Normal operation; may include `deny` in JSON. |
| `2` | Explicit block. Reason is in stderr. | Quick blocking without parsing JSON. |
| `1`, `3+` | Error. Treated as `allow` (fail-open). | Script bug; don't punish the user. |

## 6.5 Timeout Behavior

```rust
// octopus-cli/src/hooks/runner.rs ~line 94 — Timeout behavior
let result = tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async {
    // ... write stdin, wait for output
    match child.wait_with_output().await {
        Ok(o) => Some(o),
        Err(_) => None,
    }
}).await;

match result {
    Ok(Some(o)) => o,
    Ok(None) => return HookResult::allow(),
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
}
```

If a hook exceeds its timeout:
1. The subprocess is **killed** implicitly when the timeout future drops.
2. The result is **`allow`**.
3. The `timed_out` flag is set for telemetry.

This prevents a hung hook from freezing the entire CLI.

**Python comparison:** Python used `asyncio.wait_for(proc.communicate(...), timeout=timeout)` and then `proc.kill()` on timeout. Same semantics.

## 6.6 Complete Example: A Shell Hook

### `config.toml`

```toml
[[hooks]]
event = "PreToolUse"
command = "python3 /home/user/hooks/audit.py"
matcher = "shell"
timeout = 5
```

### `/home/user/hooks/audit.py`

```python
#!/usr/bin/env python3
"""
Audit hook: logs every shell command to a file.
Blocks commands that contain 'sudo'.
"""
import sys
import json
import datetime

# 1. Read the payload from stdin
payload = json.load(sys.stdin)

# 2. Extract relevant fields
event = payload["hook_event_name"]
tool_name = payload["tool_name"]
tool_input = payload.get("tool_input", {})
command = tool_input.get("command", "")

# 3. Log to audit file
with open("/tmp/audit.log", "a") as f:
    f.write(f"[{datetime.datetime.now()}] {event}: {tool_name} -> {command}\n")

# 4. Decision logic
if "sudo" in command:
    sys.stderr.write("sudo is not allowed")
    sys.exit(2)  # Block

# 5. Allow
print(json.dumps({"hookSpecificOutput": {"permissionDecision": "allow"}}))
sys.exit(0)
```

### Test Run

```bash
$ echo '{"hook_event_name":"PreToolUse","tool_name":"shell","tool_input":{"command":"ls"}}' \
  | python3 /home/user/hooks/audit.py
{"hookSpecificOutput": {"permissionDecision": "allow"}}
$ echo $?
0

$ echo '{"hook_event_name":"PreToolUse","tool_name":"shell","tool_input":{"command":"sudo rm -rf /"}}' \
  | python3 /home/user/hooks/audit.py
sudo is not allowed
$ echo $?
2
```

This script works identically in both the Python and Rust versions because the JSON protocol and exit-code semantics are preserved across the rewrite.

## 6.7 Performance Considerations

| Concern | Reality |
|---------|---------|
| Subprocess overhead | Spawning a shell + Python interpreter takes ~50–100ms. |
| Parallelism | Multiple hooks run concurrently, so overhead is the max, not the sum. |
| JSON serialization | Negligible for typical payloads (< 10 KB). In Rust, the event is serialized once per trigger (or once per wire batch), not once per hook. |
| File descriptors | Each subprocess gets its own stdin/stdout/stderr pipes. |

For high-frequency hooks (e.g., `PostToolUse` on every token), prefer **wire-side** hooks or long-running daemons that the shell command talks to via a socket.
