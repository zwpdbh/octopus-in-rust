# 12 — Subagents

## Status: 🔄 Partial

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
| `octopus-cli/src/subagents/mod.rs` | Subagent module placeholder | ~43 | 🔄 Stubs only |
| `octopus-cli/src/tools/agent/mod.rs` | `AgentTool` — subagent execution | ~160 | ✅ |

## What's Done

- [x] `AgentTool` — creates new `KimiSoul` with fresh session
- [x] Foreground subagent — waits for result, returns to parent
- [x] Background subagent — `tokio::spawn`, returns immediately
- [x] Reuses parent config, LLM, approval state, work_dir
- [x] `AgentParams` for tool-call-based subagent invocation

## What's Missing

- [ ] Subagent definition / spec parsing from YAML
- [ ] Subagent registry (`SubagentStore`)
- [ ] Subagent builder
- [ ] Git context extraction
- [ ] Output formatting (XML-like tags)
- [ ] Background subagent → parent notification (wire event)
