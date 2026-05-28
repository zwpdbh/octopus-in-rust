# Cookbook: Mutate State Through `&self` with Interior Mutability

## Problem

In Rust, mutation requires an exclusive reference (`&mut self`). But many APIs and concurrency patterns only give you `&self`:

- A trait requires `fn handle(&self, ...)`
- You share an object via `Arc<T>` across threads or async tasks
- Multiple callers need to mutate shared state concurrently

Without interior mutability, you hit the borrow checker:

```rust
pub struct Toolset {
    results: HashMap<String, ToolResult>,
}

impl Toolset {
    // ERROR: cannot borrow `self.results` as mutable through `&self`
    pub fn record(&self, key: String, result: ToolResult) {
        self.results.insert(key, result);
    }
}
```

Changing `&self` to `&mut self` is often impossible — trait definitions, `Arc` sharing, and concurrent execution all require shared references.

## Solution

Wrap the mutable field in a synchronization primitive that moves borrow checking from **compile time** to **runtime**:

```rust
use std::sync::Mutex;

pub struct Toolset {
    results: Mutex<HashMap<String, ToolResult>>,
}

impl Toolset {
    pub fn record(&self, key: String, result: ToolResult) {
        let mut guard = self.results.lock().unwrap();
        guard.insert(key, result);
    }
}
```

The `Mutex` guarantees at runtime that only one thread mutates at a time. The compiler is happy because `self.results` itself is never mutated — only the data *inside* the `Mutex` is.

## Why This Works

Rust's ownership system has two layers:

| Layer | Enforced by | Cost | Guarantees |
|---|---|---|---|
| Compile-time | `&` vs `&mut` | Zero | Static, infallible |
| Runtime | `Mutex`, `RwLock`, `RefCell` | Locking / checks | Dynamic, panics on misuse |

Interior mutability opts into the runtime layer for specific fields while keeping compile-time safety everywhere else.

## Real Examples from the Codebase

### `Mutex` for concurrent tool execution

**File:** `octopus-cli/src/soul/toolset.rs`

`KimiToolset` is shared via `Arc<KimiToolset>` and multiple tools execute in parallel. The `handle` trait method takes `&self`, yet deduplication state must be mutated:

```rust
pub struct KimiToolset {
    step_state: std::sync::Mutex<StepState>,
    approval: std::sync::Mutex<Option<Approval>>,
    mcp_state: std::sync::Mutex<McpState>,
}

async fn handle_inner(&self, tool_call: &ToolCall) -> ToolResult {
    let call_key = (tool_call.function.name.clone(), args_str);

    {
        let mut state = self.step_state.lock().unwrap();
        state.current_step_results.insert(call_key.clone(), result);
    } // lock released before await

    // ... async work continues ...
}
```

Note the block scope: the lock is held only for the brief synchronous mutation, never across an `.await`.

### `RwLock` for read-heavy state

**File:** `octopus-cli/src/soul/approval.rs`

Approval state is checked on every tool call (read) but updated only when the user acts (write):

```rust
#[derive(Clone)]
pub struct Approval {
    state: Arc<std::sync::RwLock<ApprovalState>>,
}

impl Approval {
    pub fn is_auto_approve(&self) -> bool {
        self.state.read().unwrap().yolo
    }

    pub fn approve(&self, request_id: &str) {
        let mut state = self.state.write().unwrap();
        state.pending.remove(request_id);
    }
}
```

Multiple threads can call `is_auto_approve()` concurrently; `approve()` blocks them only when writing.

### `RefCell` for single-threaded runtime checks

**File:** `octopus-cli/src/approval_runtime/runtime.rs` (before task-local refactor)

When mutation happens within a single thread but the API exposes `&self`:

```rust
thread_local! {
    static CURRENT_APPROVAL_SOURCE: RefCell<Option<ApprovalSource>> = const { RefCell::new(None) };
}

pub fn set_current_approval_source(source: Option<ApprovalSource>) {
    CURRENT_APPROVAL_SOURCE.with(|s| *s.borrow_mut() = source);
}
```

`RefCell` panics at runtime if you try to borrow mutably while already borrowed — a single-threaded alternative to `Mutex`.

### `Cell` for cheap single-threaded copies

**Hypothetical example:**

```rust
pub struct IdGenerator {
    next: Cell<u64>,
}

impl IdGenerator {
    pub fn allocate(&self) -> u64 {
        let id = self.next.get();
        self.next.set(id + 1);
        id
    }
}
```

`Cell` is the cheapest option: no locking, no borrow checking, but only works for `Copy` types and single-threaded access.

## When to Use Which

| Primitive | Thread-safe? | `Copy` required? | Use when |
|---|---|---|---|
| `Cell<T>` | ❌ | ✅ | Single-threaded counters, IDs, flags |
| `RefCell<T>` | ❌ | ❌ | Single-threaded, needs `&mut` semantics dynamically |
| `Mutex<T>` | ✅ | ❌ | Multi-threaded, write-heavy or mixed |
| `RwLock<T>` | ✅ | ❌ | Multi-threaded, read-heavy |

## Critical Rule: Don't Hold Locks Across `.await`

**Wrong:**

```rust
async fn bad(&self) {
    let mut state = self.step_state.lock().unwrap(); // lock acquired
    state.insert(key, result);
    some_async_work().await;                         // lock held across await!
} // lock released here — blocks other tasks for the entire async operation
```

**Right:**

```rust
async fn good(&self) {
    {
        let mut state = self.step_state.lock().unwrap();
        state.insert(key, result);
    } // lock released immediately

    some_async_work().await; // other tasks can proceed
}
```

If you must hold a lock across `.await`, use `tokio::sync::Mutex` instead of `std::sync::Mutex`. It is slower but async-aware — it yields to the executor instead of blocking the OS thread.

## When NOT to Use

- **Simple ownership:** If you have `&mut self`, just mutate directly. No interior mutability needed.
- **Large read-only data:** Use `Arc<T>` for sharing without any lock if the data never changes.
- **Cache-friendly hot paths:** `Mutex` has overhead. For high-contention counters, consider `AtomicUsize` or lock-free structures.

## Relation to Other Patterns

- **`Arc<T>` + interior mutability (`Mutex`, `RwLock`)** — the canonical Rust pattern for shared mutable state across threads.
- **`tokio::task_local!`** — an alternative when the "state" is actually per-task context that doesn't need to be shared.
- **RAII guards (`let _guard`)** — keep the guard alive to hold the lock; drop it early to release.
