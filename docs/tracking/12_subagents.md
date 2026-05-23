# 12 — Subagents

## Status: ⬜ Not Started

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/subagents/__init__.py` | Subagent package | ~50 |
| `kimi_cli/subagents/core.py` | Subagent core logic | ~300 |
| `kimi_cli/subagents/builder.py` | Subagent builder | ~200 |
| `kimi_cli/subagents/runner.py` | Subagent runner | ~200 |
| `kimi_cli/subagents/registry.py` | Subagent registry | ~100 |
| `kimi_cli/subagents/models.py` | Subagent models | ~100 |
| `kimi_cli/subagents/output.py` | Output formatting | ~100 |
| `kimi_cli/subagents/git_context.py` | Git context helper | ~100 |
| `kimi_cli/subagents/store.py` | Subagent store | ~100 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/subagents/mod.rs` | Subagent module placeholder | ~? | ⬜ Empty / stub |

## What's Missing

- [ ] Subagent definition / spec parsing
- [ ] Subagent registry
- [ ] Subagent runner with isolated context
- [ ] Git context extraction
- [ ] Output formatting (XML-like tags)
- [ ] Store for subagent results
