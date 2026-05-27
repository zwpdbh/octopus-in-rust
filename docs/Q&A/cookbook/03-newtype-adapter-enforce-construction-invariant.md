# Cookbook: Enforce "Setup Before Use" with a Newtype Adapter

## Problem

You need a callback (or some configuration) to be present before a group of async tasks starts, but the natural API is two-phase:

```rust
// BAD: two-phase initialization creates a race window
let handles: Vec<JoinHandle<ToolResult>> = Vec::new();

// 1. Tasks are spawned during the stream
llm.generate_streaming(
    ...,
    &mut |tc| {
        let handle = tokio::spawn(async move {
            toolset.handle(&tc).await  // ← may finish BEFORE step 2!
        });
        handles.push(handle);
    },
)
.await;

// 2. Callback is registered only after the stream ends
toolset.set_on_tool_result(Some(Box::new(|result| {
    wire_send(WireEvent::ToolResult(result.clone()));
})));

// 3. Now gather results
let results = futures::future::join_all(handles).await;
```

**The race:** a fast tool (same-step dedup, cached read, etc.) can finish in the spawned task at step 1 before `set_on_tool_result` runs at step 2. The eager wire event is lost.

This is a **logical ordering bug**, not a memory-safety bug. The Rust borrow checker cannot catch it because `tokio::spawn` creates a concurrent timeline that the compiler treats as independent.

## Solution

Move the callback from a mutable setter into the **constructor of a newtype adapter**. The compiler then enforces that you cannot obtain the adapter — and therefore cannot spawn any tasks — without first providing the callback.

```rust
/// Bridges KimiToolset to kosong::Toolset.
/// The callback is provided at construction time so it is guaranteed
/// to exist before any tool task is spawned.
pub struct KosongToolsetAdapter {
    inner: Arc<KimiToolset>,
    on_tool_result: Option<Arc<dyn Fn(&ToolResult) + Send + Sync>>,
}

impl KosongToolsetAdapter {
    pub fn new(
        inner: Arc<KimiToolset>,
        on_tool_result: Option<Arc<dyn Fn(&ToolResult) + Send + Sync>>,
    ) -> Self {
        Self { inner, on_tool_result }
    }
}

impl kosong::Toolset for KosongToolsetAdapter {
    fn handle(&self, tool_call: &kosong::ToolCall) -> kosong::HandleResult {
        let inner = self.inner.clone();
        let wire_tc = /* convert */;
        let cb = self.on_tool_result.clone();

        let handle = tokio::spawn(async move {
            let result = inner.handle(&wire_tc).await;

            // Callback is always present here — it was passed to ::new()
            if let Some(ref cb) = cb {
                cb(&result);
            }

            // ... convert back to kosong::ToolResult ...
        });

        kosong::HandleResult::Pending(handle)
    }
}
```

The caller becomes a single-phase setup:

```rust
// Setup dedup state
self.toolset.begin_step(self.last_tool_calls.clone(), self.current_step_no, turn_id);

// The adapter REQUIRES the callback at construction time.
// You literally cannot call kosong::step without deciding what it is.
let step_result = kosong::step(
    provider.as_ref(),
    &self.agent.system_prompt,
    &KosongToolsetAdapter::new(
        self.toolset.clone(),
        Some(Arc::new(|result: &ToolResult| {
            wire_send(WireEvent::ToolResult(result.clone()));
        })),
    ),
    &kosong_history,
    Some(&mut on_message_part),
)
.await;

self.last_tool_calls = self.toolset.end_step();
```

## Why This Works

Rust's orphan rules already encourage newtypes when you need to implement a foreign trait (`kosong::Toolset`) for a foreign type (`KimiToolset`). By adding the callback as a constructor parameter, we piggyback on that pattern to enforce a **temporal invariant** at the type level:

| Before (setter) | After (constructor) |
|---|---|
| `KosongToolsetAdapter::new(inner)` | `KosongToolsetAdapter::new(inner, callback)` |
| `set_on_tool_result(Some(cb))` later | No setter exists |
| Race window: tasks spawn → callback set | Impossible: no tasks can spawn without the adapter, and the adapter cannot exist without the callback |

## When to Use

- You have a **two-phase initialization** where phase 2 must happen before phase 1's side effects become visible.
- The inner type is shared via `Arc` and called from spawned tasks where you cannot control completion order.
- You are bridging two domains (e.g., `wire` types ↔ `kosong` types) and already need an adapter anyway.
- You want to **remove mutable interior state** (`Mutex<Option<T>>`) from a type that conceptually should be configuration, not state.

## When NOT to Use

- If the callback changes dynamically between steps, a constructor-only approach forces you to rebuild the adapter each step. (This is usually fine — the adapter is cheap.)
- If you truly need global mutable configuration, consider a `RwLock<Arc<Config>>` instead. But ask yourself: does it really need to be mutable?

## Relation to Other Patterns

- **Phantom types** (`KimiToolset<Idle>` vs `KimiToolset<Stepping>`) can enforce the same ordering, but they fight `dyn` trait objects. If `kosong::step` took `&T` instead of `&dyn Toolset`, phantom types would be the more rigorous choice.
- **RAII guards** (`let _guard`) solve the opposite problem: keeping a value alive until scope end. This pattern solves the problem of ensuring a value exists *before* scope entry.
- **Builder pattern**: `KosongToolsetAdapter::new(inner).with_callback(cb).build()` achieves the same invariant with more flexibility. Use it when there are many optional parameters.

## Real Example from the Codebase

**File:** `octopus-cli/src/soul/toolset.rs`

The `KosongToolsetAdapter` wraps `KimiToolset` to satisfy `kosong::Toolset`. After the refactor, `KimiToolset` no longer has an `on_tool_result` field or `set_on_tool_result` method. The wire-specific eager-callback concern lives entirely in the adapter, which is constructed fresh for each `kosong::step` call in `kimisoul.rs`.
