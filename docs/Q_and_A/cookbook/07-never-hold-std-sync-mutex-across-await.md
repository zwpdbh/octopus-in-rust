# Cookbook: Never Hold a `std::sync::Mutex` Across `.await`

## Problem

You acquire a `std::sync::Mutex` in an async function, do some work, and then `.await`. Later, your program deadlocks, panics, or runs mysteriously slowly. You stare at the code and can't see why — the lock looks fine.

```rust
async fn handle(&self, request: Request) -> Response {
    let mut state = self.state.lock().unwrap(); // looks innocent
    state.counter += 1;
    let result = self.downstream.call(request).await; // ← DANGER
    state.last_result = result.clone();
    Response { result }
} // lock released here
```

The bug is invisible: the `MutexGuard` is **still alive** during `.await`. While your task is suspended waiting for the downstream, the lock stays held. Every other task that tries to lock `self.state` blocks — on a mutex that nobody is actively using.

## The Rule

> If you see `.lock().unwrap()` in an `async fn`, ask: **will this guard survive past the next `.await`?** If yes, wrap it in a scope.

## The Fix

Create a scope that ends **before** the first `.await`:

```rust
async fn handle(&self, request: Request) -> Response {
    // Scope 1: brief synchronous mutation
    {
        let mut state = self.state.lock().unwrap();
        state.counter += 1;
    } // ← lock dropped here

    // Now await safely — no lock held
    let result = self.downstream.call(request).await;

    // Scope 2: another brief synchronous mutation
    {
        let mut state = self.state.lock().unwrap();
        state.last_result = result.clone();
    } // ← lock dropped here

    Response { result }
}
```

## How to Spot the Problem

You don't need to trace every variable lifetime. Just look at the **shape** of the function:

| Pattern | Safe? |
|---|---|
| Lock → mutate → `drop(guard)` → `.await` | ✅ Safe |
| `{ let guard = lock(); mutate }` → `.await` | ✅ Safe |
| Lock → mutate → `.await` → still using guard | ❌ **DANGEROUS** |
| Lock → mutate → `.await` → drop guard at fn end | ❌ **DANGEROUS** |

The Rust compiler will **not** warn you. `std::sync::MutexGuard` is not async-aware. It drops at scope end like any other variable, regardless of `.await`.

## Why Not `drop(guard)` Inline?

You can:

```rust
let mut state = self.state.lock().unwrap();
state.counter += 1;
drop(state); // explicit
```

But `{}` is preferred because:

1. **Visually obvious** — the indentation screams "critical section"
2. **Harder to accidentally move** — if you refactor and add a line after `drop(state)`, you might re-introduce the bug
3. **Idiomatic** — every Rustacean recognizes the pattern immediately

## Why Not `tokio::sync::Mutex`?

`tokio::sync::Mutex` *can* be held across `.await`. But it is slower and should be reserved for when you genuinely need the lock for the entire async operation:

```rust
// Only when you MUST hold the lock across await
async fn sequential_work(&self) {
    let mut state = self.tokio_mutex.lock().await;
    state.queue.push(item);
    let response = self.client.send(&state.queue).await;
    state.queue.clear();
} // lock released after await — correct, but expensive
```

Use `std::sync::Mutex` + scoped blocks for brief operations. Use `tokio::sync::Mutex` only when the locked resource *must* stay protected for the entire duration of an async call.

## Real Example from the Codebase

**File:** `octopus-cli/src/soul/toolset.rs`

`handle_inner` locks `step_state` at least four times — never across an `.await`:

```rust
async fn handle_inner(&self, tool_call: &ToolCall) -> ToolResult {
    // 1. Same-step dedup check
    {
        let state = self.step_state.lock().unwrap();
        if let Some(original) = state.current_step_results.get(&call_key) {
            return original.clone();
        }
    }

    // 2. Cross-step dedup check
    let is_cross_step_dup = {
        let state = self.step_state.lock().unwrap();
        state.previous_step_calls.contains(&call_key)
    };

    // ... approval check (has its own .await) ...

    // 3. Cache the result
    {
        let mut state = self.step_state.lock().unwrap();
        state.current_step_results.insert(call_key.clone(), result.clone());
    }
}
```

Each lock is held for microseconds. The async parts (approval, tool execution) run completely unlocked.

## When to Use

- You have `std::sync::Mutex` inside an `async fn`
- The lock only protects a brief synchronous operation (HashMap get/insert, counter increment, etc.)
- There is any `.await` anywhere after the lock acquisition

## When NOT to Use

- The lock genuinely needs to protect a resource for the entire async operation (use `tokio::sync::Mutex` instead)
- The function is synchronous (no `.await`) — then the guard drops at scope end naturally

## The One-Line Summary

> In async code, treat every `.lock().unwrap()` as guilty until proven scoped.
