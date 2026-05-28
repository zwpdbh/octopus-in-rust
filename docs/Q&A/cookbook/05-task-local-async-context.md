# Cookbook: Carry Ambient Async Context with `tokio::task_local!`

## Problem

You need "ambient" context deep inside an async call stack — for example, the current tool call ID or the active wire channel — but threading it through every function signature is invasive. The natural fallback is a `thread_local!` static:

```rust
// BAD: thread_local! in async code
thread_local! {
    static CURRENT_TOOL_CALL: RefCell<Option<WireToolCall>> = const { RefCell::new(None) };
}

pub fn set_current_tool_call(tc: Option<WireToolCall>) {
    CURRENT_TOOL_CALL.with(|c| *c.borrow_mut() = tc);
}

pub fn get_current_tool_call() -> Option<WireToolCall> {
    CURRENT_TOOL_CALL.with(|c| c.borrow().clone())
}
```

Used like:

```rust
set_current_tool_call(Some(wire_tc));
let return_value = tool.call_raw(arguments).await;
set_current_tool_call(None); // Easy to forget!
```

This has **three async-specific hazards**:

1. **Task migration.** Tokio tasks can move between OS threads at `.await` points. A `thread_local` value set on thread A may not exist when the task resumes on thread B.
2. **Manual cleanup.** If the future is cancelled, panics, or the author simply forgets the `set(None)` call, the context leaks into the next task that happens to run on the same thread.
3. **Re-entrancy panics.** `RefCell::borrow_mut()` panics if the same thread re-enters while the value is already borrowed. In async code, this can happen during recursive tool calls or nested scopes.

## Solution

Use `tokio::task_local!` instead. It binds the value to the **async task**, not the OS thread, and provides a scoped API that cleans up automatically:

```rust
// File: octopus-cli/src/soul/toolset.rs
tokio::task_local! {
    static CURRENT_TOOL_CALL: Option<WireToolCall>;
}

pub fn get_current_tool_call() -> Option<WireToolCall> {
    CURRENT_TOOL_CALL.try_with(|tc| tc.clone()).unwrap_or(None)
}
```

Set the value for a bounded async scope:

```rust
let return_value = CURRENT_TOOL_CALL
    .scope(Some(wire_tc), async {
        tool.call_raw(arguments).await
    })
    .await;
```

The same pattern for the wire channel:

```rust
// File: octopus-cli/src/wire/hub.rs
tokio::task_local! {
    static CURRENT_WIRE_SOUL_SIDE: Option<WireSoulSide>;
}

pub fn get_current_wire_soul_side() -> Option<WireSoulSide> {
    CURRENT_WIRE_SOUL_SIDE.try_with(|w| w.clone()).unwrap_or(None)
}

pub async fn with_wire_soul_side<F, T>(
    side: Option<WireSoulSide>,
    f: F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    CURRENT_WIRE_SOUL_SIDE.scope(side, f).await
}
```

## Why This Works

| Aspect | `thread_local!` + `RefCell` | `tokio::task_local!` |
|---|---|---|
| **Bound to** | OS thread | Async task (follows the `Future`) |
| **Survives `.await`** | ❌ Risky — task may migrate threads | ✅ Guaranteed |
| **Cleanup** | Manual `set(None)` | Automatic when `.scope()` future ends |
| **Panics** | `RefCell::borrow_mut()` can panic on re-entrancy | No `RefCell` needed; immutable access only |
| **Cancellation safety** | Context may leak if future is dropped | Context is always cleaned up on scope exit |

`tokio::task_local!` is implemented on top of `thread_local!`, but it adds a Tokio-aware wrapper. When you call `.scope(value, future)`, Tokio stores the value in the current thread-local before polling your future, clears it after the poll finishes, and — critically — moves the value with the task when it migrates across threads.

## Real Examples from the Codebase

### Tool call context

**File:** `octopus-cli/src/soul/toolset.rs`

The approval system needs the current `tool_call.id` to correlate approval requests with wire events. It calls `get_current_tool_call()` deep inside `call_raw()` → `approval.request()`.

Before:
```rust
set_current_tool_call(Some(wire_tc));
let return_value = tool.call_raw(arguments).await;
set_current_tool_call(None); // manual, error-prone
```

After:
```rust
let return_value = CURRENT_TOOL_CALL
    .scope(Some(wire_tc), async {
        tool.call_raw(arguments).await
    })
    .await;
```

### Wire soul side context

**File:** `octopus-cli/src/soul/kimisoul.rs`

Every `run()` sets the current wire channel so that any code anywhere can call `wire_send(...)` and the event reaches the right listener.

Before:
```rust
crate::wire::set_current_wire_soul_side(Some(soul_side.clone()));
let result = self.run_turn(text).await;
crate::wire::set_current_wire_soul_side(None);
```

After:
```rust
let result = crate::wire::with_wire_soul_side(Some(soul_side.clone()), async {
    self.run_turn(text).await
}).await;
```

### Approval source context

**File:** `octopus-cli/src/approval_runtime/runtime.rs`

The approval runtime uses the same pattern for propagating the source of an approval request (foreground turn, subagent, background task, etc.):

```rust
tokio::task_local! {
    static CURRENT_APPROVAL_SOURCE: ApprovalSource;
}

pub fn get_current_approval_source_or_none() -> Option<ApprovalSource> {
    CURRENT_APPROVAL_SOURCE.try_with(|s| s.clone()).ok()
}

pub async fn with_approval_source<T>(
    source: ApprovalSource,
    f: F,
) -> T {
    CURRENT_APPROVAL_SOURCE.scope(source, f).await
}
```

## When to Use

- You have **cross-cutting context** (request ID, user session, wire channel, tool call) that would need to be threaded through 5+ layers of function signatures.
- The context is **read-mostly** — set once at a boundary and read many times deep in the call stack.
- You are inside **Tokio async code** where tasks may migrate threads.
- You want **RAII cleanup** — the context automatically disappears when the scope ends, even on panic or cancellation.

## When NOT to Use

- The data is **large or expensive to clone** on every `.try_with()`. Task-local storage stores the owned value; every read clones it. For heavy data, store an `Arc<MyData>` instead.
- You are in **non-Tokio async** (e.g., `async-std` without Tokio compatibility). Use that runtime's equivalent, or fall back to explicit parameter passing.
- The context is **mutable** throughout the scope. Task-locals are immutable inside the scope; if you need mutation, wrap the value in a `Cell` or `RefCell` — but ask yourself whether a parameter would be clearer.
- You are in **synchronous code** outside any async runtime. Use `thread_local!` directly; `tokio::task_local!` requires a Tokio context.

## Relation to Other Patterns

- **Explicit parameters:** The zero-magic alternative. Prefer this when the call depth is shallow (< 3 layers) or when the context is needed by only a few functions.
- **`tracing::Span`:** `tracing` uses a similar task-local mechanism under the hood for the current span. If your context is primarily for logging/observability, consider attaching it to a span instead.
- **Request guards / Axum extractors:** Web frameworks solve the same problem by storing request extensions in the request object and extracting them at handler boundaries. Task-locals are the lower-level primitive when you don't have a request object.
