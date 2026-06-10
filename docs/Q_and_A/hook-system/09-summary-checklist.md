# 9. Summary & Checklist

This section condenses the entire tutorial into a quick-reference guide for implementing, debugging, and extending the hook system in **octopus-cli** (Rust).

## 9.1 Conceptual Summary

A **hook** is an interception point in the system lifecycle. The `HookEngine` matches events to registered handlers, executes them in parallel, and aggregates their decisions. The most critical hook is `PreToolUse`, which can **block** a tool call before it executes.

## 9.2 Architecture at a Glance

```
┌─────────────────────────────────────────────────────────────┐
│                         Core System                         │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────────┐  │
│  │ Session │  │  Turn   │  │  Tool   │  │  Subagent   │  │
│  │ Start   │  │  Loop   │  │  Call   │  │  Runner     │  │
│  └────┬────┘  └────┬────┘  └────┬────┘  └──────┬──────┘  │
│       │            │            │               │         │
│       └────────────┴────────────┴───────────────┘         │
│                         │                                   │
│              ┌──────────┴──────────┐                       │
│              │    HookEngine       │                       │
│              │  ┌───────┐ ┌──────┐ │                       │
│              │  │Server │ │ Wire │ │                       │
│              │  │Hooks  │ │ Subs │ │                       │
│              │  └───┬───┘ └──┬───┘ │                       │
│              └──────┼────────┼─────┘                       │
│                     │        │                              │
│              ┌──────┘        └──────┐                      │
│              ▼                      ▼                      │
│      ┌─────────────┐      ┌─────────────────┐             │
│      │ Subprocess  │      │  Wire Client    │             │
│      │ (shell cmd) │      │  (JSON-RPC)     │             │
│      └─────────────┘      └─────────────────┘             │
└─────────────────────────────────────────────────────────────┘
```

The **HookEngine** sits between the core business logic and external handlers. It is:
- **Synchronous** for blocking hooks (the core waits for a decision).
- **Asynchronous / fire-and-forget** for observer hooks (the core does not wait).

## 9.3 Quick Checklist: Adding a New Hook

- [ ] Add the event variant to `HookEvent` enum in `src/hooks/event.rs`.
- [ ] Add a constructor method `HookEvent::new_event_name(...)` in `event.rs`.
- [ ] Add the variant to `discriminant_serde::deserialize` so config files can reference it.
- [ ] Add the variant to the `Display` impl.
- [ ] Add the trigger site in the core system (e.g., `toolset.rs`, `kimisoul.rs`).
- [ ] Decide: is this hook **blocking** or **fire-and-forget**?
  - If blocking: check `results` for `HookAction::Block` and abort the operation.
  - If fire-and-forget: spawn the trigger in a detached task (don't `await`).
- [ ] Update `config.rs` if new fields are needed in `HookDef`.
- [ ] Write a test in `src/hooks/` covering:
  - [ ] Happy path (hook runs, returns allow).
  - [ ] Block path (hook returns block, operation aborts).
  - [ ] Fail-open path (hook crashes/times out, operation proceeds).
  - [ ] Regex matching (hook only triggers for matching `matcher_value`).

## 9.4 Quick Checklist: Porting from Python

- [ ] Replace `Literal["..."]` with `enum HookEvent`.
- [ ] Replace `dict[str, Any]` payload builders with `enum HookEvent` variants.
- [ ] Replace `asyncio.create_subprocess_shell` with `tokio::process::Command`.
- [ ] Replace `asyncio.wait_for` with `tokio::time::timeout`.
- [ ] Replace trial-and-error wire deserialization with `enum WireEvent`.
- [ ] Compile regex patterns once at startup (`HookDef::compile_matcher`), not per-trigger.
- [ ] Use `Arc<HookEvent>` if the payload is shared across multiple spawned tasks.
- [ ] Add `#[serde(tag = "hook_event_name", rename_all = "PascalCase")]` to `HookEvent` for JSON compatibility.
- [ ] Verify wire protocol JSON shape is identical to Python version for backward compatibility.
- [ ] Add `on_wire_hook_done` cleanup callback to prevent wire handle leaks.
- [ ] Use typed structs (`Deserialize`) for parsing hook stdout instead of `serde_json::Value` indexing.

## 9.5 Debugging Hooks

### "My hook isn't running"

1. Check that the hook's `event` matches the trigger site discriminant exactly.
2. Check that the `matcher` regex actually matches the `matcher_value`.
3. Verify the hook is loaded: log `engine.summary()` or inspect `engine.details()`.
4. For wire hooks: verify the subscription was sent during initialization.
5. Check that `compiled_matcher` is not `None` (invalid regex would log a warning at load time).

### "My hook blocks but the tool still runs"

1. Ensure the trigger site actually checks `results` for `HookAction::Block`.
2. Check that the `HookResult` action is exactly `HookAction::Block`.
3. Verify the aggregation logic: `any()` or `for r in results` must inspect every result.

### "My hook script times out"

1. Check `timeout` in `HookDef` (default 30s).
2. Ensure the script reads from stdin (if it doesn't, `write_all` may hang waiting for EOF).
3. Test the script manually: `echo '{}' | python script.py`.

### "Wire client receives hook but response is ignored"

1. Verify the response JSON-RPC `id` matches the request `id` exactly.
2. Check that the response is a valid `HookResponse` shape (no extra required fields).
3. Ensure the client responds before the timeout.
4. Check that `on_wire_hook_done` is registered (otherwise timeouts leak the handle).

## 9.6 Security Reminders

- **Fail-open by design**: a broken hook never accidentally blocks the system.
- **Shell injection**: `command` strings are passed to `/bin/sh -c`. Never construct `command` from user input.
- **Timeouts**: always set a reasonable timeout. A missing timeout is a DoS vector.
- **Wire trust**: wire clients can claim any `subscription_id`. The server does not authenticate subscriptions.

## 9.7 Further Reading

| Section | File | Topic |
|---------|------|-------|
| 2 | `02-what-is-a-hook.md` | Conceptual foundation |
| 3 | `03-architecture-overview.md` | Module map and lifecycle |
| 4 | `04-deep-dive-pretooluse.md` | Complete PreToolUse trace |
| 5 | `05-hook-engine-internals.md` | Matching, dedup, aggregation |
| 6 | `06-server-side-runner.md` | Subprocess protocol |
| 7 | `07-wire-side-hooks.md` | JSON-RPC wire protocol |
| 8 | `08-lessons-from-the-rewrite.md` | Before/after comparison |

---

*Tutorial complete. For questions or corrections, update the relevant section and bump the last-edited date below.*

**Last edited:** 2026-06-06
