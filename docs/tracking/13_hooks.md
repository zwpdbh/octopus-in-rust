# 13 — Hooks

## Status: 🔄 Partial

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/hooks/__init__.py` | Hooks package | ~50 |
| `kimi_cli/hooks/engine.py` | Hook matching engine | ~200 |
| `kimi_cli/hooks/config.py` | Hook config schema | ~100 |
| `kimi_cli/hooks/events.py` | Event types | ~50 |
| `kimi_cli/hooks/runner.py` | Hook command runner | ~100 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/hooks/mod.rs` | HookEngine + WireHookSubscription | ~250 | ✅ Full engine with regex matching + parallel dispatch |
| `octopus-cli/src/hooks/runner.rs` | HookResult + run_hook shell runner | ~150 | ✅ Fail-open subprocess execution |
| `octopus-cli/src/hooks/events.rs` | Event payload builders | ~150 | ✅ All event types mirrored |
| `octopus-cli/src/config.rs` | Hook config in Config struct | ~440 | ✅ Config parsed with timeout default |

## What's Done

- [x] Hook config parsing in `Config` (TOML `[[hooks]]` tables)
- [x] `HookEngine` with event indexing, regex matcher evaluation, parallel async dispatch
- [x] `run_hook()` subprocess runner with JSON stdin, timeout handling, exit-code parsing
- [x] `HookResult` / `HookAction` (`allow` / `block`)
- [x] Event payload builders (`pre_tool_use`, `post_tool_use`, `post_tool_use_failure`, `user_prompt_submit`, `stop`, `stop_failure`, `pre_compact`, `post_compact`, `notification`)
- [x] `UserPromptSubmit` hook wired in `KimiSoul::run()`
- [x] `Stop` hook wired on normal turn completion
- [x] `StopFailure` hook wired on fatal step error
- [x] `PreToolUse` hook wired in `KimiToolset::handle()`
- [x] `PostToolUse` / `PostToolUseFailure` hooks wired in `KimiToolset::handle()`
- [x] `PreCompact` / `PostCompact` hooks wired in `compact_context()`

## What's Missing

- [ ] Wire-side hook delivery (client-side subscriptions via wire)
- [ ] `/hooks` full implementation (currently lists config hooks only; should use `hook_engine.details()`)
- [ ] Telemetry tracking for hook triggers
