# 11 — Background Tasks

## Status: ⬜ Not Started

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
| `octopus-cli/src/background/mod.rs` | Background module placeholder | ~? | ⬜ Empty / stub |
| `octopus-cli/src/tools/background/mod.rs` | TaskOutput / TaskStop tools | ~106 | 🔄 Skeleton only |

## What's Missing

- [ ] Background task manager
- [ ] Task lifecycle (create, run, pause, resume, cancel)
- [ ] Task store (SQLite or file-based)
- [ ] Task summarization after completion
- [ ] `/task` browser UI
- [ ] Notification delivery to wire
- [ ] Subagent runner for background tasks
