# 11 — Background Tasks

## Status: 🔄 Partial

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/background/__init__.py` | Background package | ~50 |
| `kimi_cli/background/manager.py` | Task manager | ~300 |
| `kimi_cli/background/worker.py` | Task worker | ~200 |
| `kimi_cli/background/store.py` | Task persistence | ~150 |
| `kimi_cli/background/models.py` | Task data models | ~100 |
| `kimi_cli/background/ids.py` | Task ID generation | ~50 |
| `kimi_cli/background/summary.py` | Task summarization | ~100 |
| `kimi_cli/background/agent_runner.py` | Agent runner for tasks | ~150 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/background/mod.rs` | BackgroundTaskManager with real process spawning | ~205 | ✅ |
| `octopus-cli/src/tools/background/mod.rs` | TaskOutput / TaskStop tools | ~160 | ✅ |
| `octopus-cli/src/tools/shell/mod.rs` | ShellTool with run_in_background support | ~169 | ✅ |

## What's Done

- [x] `BackgroundTaskManager` — spawn real `tokio::process::Child` processes
- [x] Output capture via reader tasks (`stdout` + `stderr`)
- [x] Task lifecycle: `spawn()`, `get_output()`, `stop()`, `list_tasks()`
- [x] `TaskOutputTool` — read task output with `block=true` support
- [x] `TaskStopTool` — kill running background tasks
- [x] `ShellTool` — `run_in_background=true` creates background tasks
- [x] Shutdown cleanup — `KimiSoul::shutdown()` kills all child processes

## What's Missing

- [ ] `/task` browser UI — interactive task list in TUI
- [ ] Task persistence (SQLite or file-based store)
- [ ] Task summarization after completion
- [ ] Notification delivery to wire on task completion
