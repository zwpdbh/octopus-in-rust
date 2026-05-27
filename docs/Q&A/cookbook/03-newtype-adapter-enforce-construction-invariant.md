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

Move the callback from a mutable setter into a **parameter of the single function that orchestrates everything**. The compiler then enforces that you cannot start the step — and therefore cannot spawn any tasks — without first providing the callback.

In our case, `kosong::step_with_callbacks` accepts `on_tool_result` as a parameter:

```rust
pub async fn step_with_callbacks(
    chat_provider: &dyn ChatProvider,
    system_prompt: &str,
    toolset: &dyn Toolset,
    history: &[Message],
    on_message_part: Option<&mut (dyn FnMut(StreamedMessagePart) + Send)>,
    on_tool_result: Option<Arc<dyn Fn(&ToolResult) + Send + Sync>>,
) -> Result<StepResult, ChatProviderError>;
```

The caller becomes a single-phase setup:

```rust
// Setup dedup state
self.toolset.begin_step(self.last_tool_calls.clone(), self.current_step_no, turn_id);

// The callback is REQUIRED at call time.
// You literally cannot call step_with_callbacks without deciding what it is.
let step_result = kosong::step_with_callbacks(
    provider.as_ref(),
    &self.agent.system_prompt,
    &KimiToolsetHandle(self.toolset.clone()),
    &kosong_history,
    Some(&mut on_message_part),
    Some(Arc::new(|result: &kosong::ToolResult| {
        let wire_result = kosong_to_wire_tool_result(result);
        wire_send(WireEvent::ToolResult(wire_result));
    })),
)
.await;

self.last_tool_calls = self.toolset.end_step();
```

The `KimiToolsetHandle` newtype wrapper exists for a different reason (Rust orphan rules: you cannot implement a foreign trait `kosong::Toolset` for a foreign type `Arc<KimiToolset>`). It is stateless and cheap to construct:

```rust
pub struct KimiToolsetHandle(pub Arc<KimiToolset>);

impl kosong::Toolset for KimiToolsetHandle {
    fn tools(&self) -> Vec<kosong::Tool> { /* ... */ }
    fn handle(&self, tool_call: &kosong::ToolCall) -> kosong::HandleResult { /* ... */ }
}
```

## Why This Works

By moving the callback from a mutable field on `KimiToolset` to a parameter of `kosong::step_with_callbacks`, we enforce a **temporal invariant** at the API level:

| Before (setter) | After (parameter) |
|---|---|
| `toolset.set_on_tool_result(Some(cb))` after spawning | `on_tool_result: Some(cb)` passed to `step_with_callbacks` |
| Race window: tasks spawn → callback set | Impossible: `step_with_callbacks` receives the callback before it spawns any tasks |

The `KimiToolsetHandle` newtype is still a useful pattern — it satisfies orphan rules while keeping the bridge minimal. But the critical safety property now comes from the function signature of `step_with_callbacks`, not from the adapter's constructor.

## When to Use

- You have a **two-phase initialization** where phase 2 must happen before phase 1's side effects become visible.
- The inner type is shared via `Arc` and called from spawned tasks where you cannot control completion order.
- You are bridging two domains (e.g., `wire` types ↔ `kosong` types) and need an adapter to satisfy orphan rules.
- You want to **remove mutable interior state** (`Mutex<Option<T>>`) from a type that conceptually should be configuration, not state.

## When NOT to Use

- If the callback changes dynamically between steps, a parameter-only approach forces you to pass it each step. (This is usually fine — the callback is cheap to clone.)
- If you truly need global mutable configuration, consider a `RwLock<Arc<Config>>` instead. But ask yourself: does it really need to be mutable?

## Relation to Other Patterns

- **Phantom types** (`KimiToolset<Idle>` vs `KimiToolset<Stepping>`) can enforce the same ordering, but they fight `dyn` trait objects. If `kosong::step_with_callbacks` took `&T` instead of `&dyn Toolset`, phantom types would be the more rigorous choice.
- **RAII guards** (`let _guard`) solve the opposite problem: keeping a value alive until scope end. This pattern solves the problem of ensuring a value exists *before* scope entry.
- **Builder pattern**: `StepBuilder::new(toolset).with_callback(cb).run()` achieves the same invariant with more flexibility. Use it when there are many optional parameters.

## Real Example from the Codebase

**File:** `octopus-cli/src/soul/kimisoul.rs`

`KimiToolset` no longer has an `on_tool_result` field or `set_on_tool_result` method. The wire-specific eager-callback concern lives entirely in the `step_with_callbacks` call site, which is invoked fresh for each step. The `KimiToolsetHandle` wrapper exists only to bridge `Arc<KimiToolset>` to `kosong::Toolset` without violating orphan rules.
