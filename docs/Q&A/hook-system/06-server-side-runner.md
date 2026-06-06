# 6. Server-Side Runner

The `run_hook()` function in `src/kimi_cli/hooks/runner.py` is the bridge between the Python `HookEngine` and arbitrary external programs. It turns a hook configuration into a subprocess, feeds it JSON, and interprets the result.

## 6.1 Signature

```python
async def run_hook(
    command: str,
    input_data: dict[str, Any],
    *,
    timeout: int = 30,
    cwd: str | None = None,
) -> HookResult:
```

| Parameter | Description |
|-----------|-------------|
| `command` | The shell command string from `HookDef.command`. |
| `input_data` | The JSON payload (e.g., `events.pre_tool_use(...)`). |
| `timeout` | Maximum seconds to wait for the subprocess. |
| `cwd` | Working directory for the subprocess (defaults to engine's cwd). |

## 6.2 Subprocess Creation

```python
proc = await asyncio.create_subprocess_shell(
    command,
    stdin=asyncio.subprocess.PIPE,
    stdout=asyncio.subprocess.PIPE,
    stderr=asyncio.subprocess.PIPE,
    cwd=cwd,
)
```

**Important:** `create_subprocess_shell` means the `command` string is passed to `/bin/sh -c`. This allows:
- Shell pipes: `"cat | jq .tool_name"`
- Environment variables: `"MY_VAR=1 python script.py"`
- Relative paths: `"python ./hooks/my_hook.py"`

But it also means:
- **Quote carefully**: if `command` contains user input, it must be shell-escaped.
- **Security**: a malicious `config.toml` could inject shell commands.

## 6.3 Communication Protocol

The runner writes JSON to stdin and reads stdout/stderr:

```python
stdin_data = json.dumps(input_data).encode()
stdout, stderr = await asyncio.wait_for(
    proc.communicate(input=stdin_data),
    timeout=timeout,
)
```

### stdin

A single JSON object, minified, followed by EOF.

```json
{"hook_event_name":"PreToolUse","session_id":"sess_abc","cwd":"/home/user","tool_name":"shell","tool_input":{"command":"ls"},"tool_call_id":"call_xyz"}
```

### stdout

Expected to be valid JSON when exit code is 0. Optional format:

```json
{
  "hookSpecificOutput": {
    "permissionDecision": "allow"
  }
}
```

### stderr

Used as the `reason` string when `exit_code == 2`.

## 6.4 Decision Logic

```python
# Exit code 2 → explicit block
if proc.returncode == 2:
    return HookResult(
        action="block",
        reason=stderr.decode().strip() or "Blocked"
    )

# Exit code 0 → check stdout JSON
if proc.returncode == 0:
    try:
        parsed = json.loads(stdout.decode())
        decision = parsed.get("hookSpecificOutput", {}).get("permissionDecision")
        if decision == "deny":
            return HookResult(action="block", reason="Denied by hook output")
    except (json.JSONDecodeError, AttributeError):
        pass
    return HookResult(action="allow")

# Any other exit code → allow (fail-open)
return HookResult(action="allow")
```

### Exit Code Semantics

| Exit Code | Meaning | Typical Use |
|-----------|---------|-------------|
| `0` | Success. Check stdout for `permissionDecision`. | Normal operation; may include `deny` in JSON. |
| `2` | Explicit block. Reason is in stderr. | Quick blocking without parsing JSON. |
| `1`, `3+` | Error. Treated as `allow` (fail-open). | Script bug; don't punish the user. |

## 6.5 Timeout Behavior

```python
try:
    stdout, stderr = await asyncio.wait_for(
        proc.communicate(input=...),
        timeout=timeout,
    )
except asyncio.TimeoutError:
    proc.kill()
    return HookResult(action="allow", reason="Hook timed out")
```

If a hook exceeds its timeout:
1. The subprocess is **killed** (`SIGKILL`).
2. The result is **`allow`**.
3. The reason string notes the timeout.

This prevents a hung hook from freezing the entire CLI.

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

## 6.7 Performance Considerations

| Concern | Reality |
|---------|---------|
| Subprocess overhead | Spawning a shell + Python interpreter takes ~50–100ms. |
| Parallelism | Multiple hooks run concurrently, so overhead is the max, not the sum. |
| JSON serialization | Negligible for typical payloads (< 10 KB). |
| File descriptors | Each subprocess gets its own stdin/stdout/stderr pipes. |

For high-frequency hooks (e.g., `PostToolUse` on every token), prefer **wire-side** hooks or long-running daemons that the shell command talks to via a socket.
