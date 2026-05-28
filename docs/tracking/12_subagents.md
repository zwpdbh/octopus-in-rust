# 12 — Subagents

## Status: ✅ Complete

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
| `octopus-cli/src/subagents/mod.rs` | `LaborMarket`, `SubagentStore`, `AgentTypeDefinition`, `ToolPolicy` | ~130 | ✅ |
| `octopus-cli/src/tools/agent/mod.rs` | `AgentTool` — full subagent execution pipeline | ~270 | ✅ |
| `octopus-cli/src/soul/agent.rs` | `load_agent()`, `AppRuntime::copy_for_subagent()` | ~570 | ✅ |

## What's Done

- [x] `AgentTool` — full subagent execution pipeline
- [x] Foreground subagent — waits for result, returns to parent
- [x] Background subagent — `tokio::spawn`, returns immediately, tracked in `SubagentStore`
- [x] `LaborMarket` — type registry populated from agent YAML specs
- [x] `AgentTypeDefinition` — name, description, agent_file, default_model, tool_policy
- [x] `ToolPolicy` — `AllowList { tools: Vec<ToolName> }` and `Inherit` variants, enforced via `KimiToolset::hide_all_except(&[ToolName])`
- [x] Model override resolution — `params.model` → `def.default_model` → parent LLM
- [x] `AppRuntime::copy_for_subagent()` — shares LaborMarket, skills, notifications, background_tasks, denwa_renji
- [x] `SubagentStore` — tracks all subagent spawns with `Running | Completed | Failed` status
- [x] `AgentParams` for tool-call-based subagent invocation (with `model`, `run_in_background`, `resume` fields)
- [x] Subagent spec parsing from YAML (via `agents/mod.rs` `load_agent_spec()`)
- [x] Subagent type registration in `LaborMarket` during `load_agent()`

## What's Missing

- [ ] Git context extraction (deferred — not in Python's core subagent flow)
- [ ] Output formatting with XML-like tags (deferred — Python's formatting is cosmetic)
