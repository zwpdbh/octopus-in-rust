# Task: refactor-hook-event-split

## Goal

Remove the dual role of `HookEvent` (config index key vs. runtime payload) by splitting it into `HookEventKind` and `HookEvent`, and unify server-side and wire-side hooks under a single `Hook` trait.

## Background

`HookEvent` was serving two incompatible roles:

1. **Registry/config key**: loaded from `config.toml` as `event = "PreToolUse"`, with empty/default payload fields, and used as a `HashMap` key via discriminant-only equality/hash.
2. **Runtime payload**: fully populated when triggered and serialized to hook subprocesses and wire clients.

This forced `HookEngine::trigger(event, matcher_value)` to accept a separate `matcher_value` argument because the config-loaded event could not supply the match target. It also split the engine into two nearly identical code paths for server and wire hooks.

## Scope

### In Scope

- Introduce `HookEventKind` (discriminant-only enum) and make `HookEvent` a concrete runtime payload.
- Add `HookEvent::kind()` and `HookEvent::matcher_value()` helpers.
- Create `Hook` trait with `CommandHook` and `WireHook` implementations.
- Refactor `HookEngine` to store `Vec<Box<dyn Hook>>` indexed by `HookEventKind`.
- Change `trigger(event: HookEvent)` to derive kind/matcher internally.
- Update all call sites (`toolset.rs`, `kimisoul.rs`, `wire_server/mod.rs`, `config.rs`).
- Update hook-system documentation.

### Out of Scope

- New hook backends (HTTP, WASM, Python, etc.) — the trait makes these easier but they are not added here.
- `/hooks` command expansion beyond `hook_engine.details()`.
- Telemetry tracking for hook triggers.

## Acceptance Criteria

- [x] `HookEventKind` exists and is used for config/registry indexing.
- [x] `HookEvent` is concrete and no longer has discriminant-only equality/hash.
- [x] `HookEngine::trigger` signature is `trigger(event: HookEvent)`.
- [x] Server and wire hooks share the `Hook` trait execution path.
- [x] All existing tests pass; new tests added for `matcher_value()` and deduplication.
- [x] `cargo check --workspace` passes.
- [x] `cargo test -p octopus-cli` passes.
- [x] Hook-system docs updated.

## Implementation Notes

- `OnTriggered`, `OnResolved`, and `OnWireHook` changed from `Box<dyn Fn...>` to `Arc<dyn Fn...>` so they can be cloned into spawned hook tasks.
- `HookRunContext` bundles `cwd` and callbacks and is passed to `Hook::run()`.
- `CommandHook` deduplicates by command string via the `command()` trait method; `WireHook` returns `None` and is not deduplicated.
- Empty matcher strings are normalized to `None` so they match everything.

## Completed Steps

- [x] Define `HookEventKind` and update `HookEvent` in `src/hooks/event.rs`.
- [x] Create `src/hooks/hook.rs` with `Hook`, `CommandHook`, `WireHook`, and contexts.
- [x] Refactor `src/hooks/engine.rs` to use `HookEventKind` and `Box<dyn Hook>`.
- [x] Update `src/config.rs` to use `HookEventKind` in `HookDef`.
- [x] Update `src/wire_server/mod.rs` for `HookEventKind` and `Arc` callbacks.
- [x] Update `src/soul/toolset.rs` and `src/soul/kimisoul.rs` trigger call sites.
- [x] Add tests for `matcher_value()` and duplicate-command deduplication.
- [x] Update hook-system docs (`03`, `04`, `05`, `07`, `08`, `09`).

## Decisions Made

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-06-12 | Use lightweight factory pattern (`Hook` trait) rather than full reearth-flow-style factory registry | Only two hook backends exist; full factory registry would add complexity without benefit |
| 2026-06-12 | Keep deduplication by command string for server hooks only | Prevents accidental double-execution from config; wire subscriptions are intentionally distinct |
