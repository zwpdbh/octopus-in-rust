# 13 — Hooks

## Status: ✅ Complete

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
| `octopus-cli/src/hooks/mod.rs` | Re-exports | ~15 | ✅ |
| `octopus-cli/src/hooks/event.rs` | `HookEventKind` + `HookEvent` payload builders | ~350 | ✅ |
| `octopus-cli/src/hooks/hook.rs` | `Hook` trait, `CommandHook`, `WireHook`, contexts | ~200 | ✅ |
| `octopus-cli/src/hooks/engine.rs` | `HookEngine` with regex matching + parallel dispatch | ~320 | ✅ |
| `octopus-cli/src/hooks/runner.rs` | `HookResult` + `run_hook` shell runner | ~180 | ✅ |
| `octopus-cli/src/config.rs` | Hook config in `Config` struct | ~440 | ✅ |

## What's Done

- [x] Hook config parsing in `Config` (TOML `[[hooks]]` tables)
- [x] `HookEventKind` / `HookEvent` split; `HookEvent` is a concrete runtime payload
- [x] `HookEngine` indexed by `HookEventKind`, storing `Box<dyn Hook>`
- [x] `CommandHook` for server-side shell commands
- [x] `WireHook` for wire client subscriptions
- [x] `trigger(event: HookEvent)` — kind and matcher value derived from payload
- [x] Regex matcher evaluation with pre-compiled `Regex`
- [x] Command-string deduplication for server-side hooks
- [x] Parallel async dispatch with `tokio::spawn`
- [x] `run_hook()` subprocess runner with JSON stdin, timeout handling, exit-code parsing
- [x] `HookResult` / `HookAction` (`allow` / `block`)
- [x] Event payload builders (`pre_tool_use`, `post_tool_use`, `post_tool_use_failure`, `user_prompt_submit`, `stop`, `stop_failure`, `pre_compact`, `post_compact`, `notification`)
- [x] `UserPromptSubmit` hook wired in `KimiSoul::run()`
- [x] `Stop` hook wired on normal turn completion
- [x] `StopFailure` hook wired on fatal step error
- [x] `PreToolUse` hook wired in `KimiToolset::handle()`
- [x] `PostToolUse` / `PostToolUseFailure` hooks wired in `KimiToolset::handle()`
- [x] `PreCompact` / `PostCompact` hooks wired in `compact_context()`
- [x] Wire-side hook subscription and `HookRequest`/`HookResponse` flow

## What's Missing

- [ ] `/hooks` slash command full implementation (currently lists config hooks only; should use `hook_engine.details()`)
- [ ] Telemetry tracking for hook triggers
