# Cookbook: Hold a RAII Guard with `let _guard`

## Problem

In Rust, many types perform critical cleanup when they are dropped (unlocked, closed, restored, etc.). You need to keep the value alive for its side effects, but you never call methods on it directly. The compiler warns about "unused variables," but the obvious fix — replacing the binding with `_` — causes the value to drop immediately:

```rust
async fn refresh_tokens(&self) {
    let _ = self.refresh_lock.lock().await;  // WRONG: lock drops immediately!

    // ... token refresh happens UNLOCKED — data race!
}
```

## Solution

Use `let _guard` (or `let _timer`, `let _span`, etc.) to suppress the unused-variable warning **while preserving the binding's lifetime**:

```rust
// File: octopus-cli/src/auth/manager.rs
async fn refresh_tokens(&self, key: &str, token: &OAuthToken) {
    let _guard = self.refresh_lock.lock().await;

    // ... exclusive work ...

}  // ← _guard dropped here → lock released
```

The `_` prefix tells the compiler: *"I won't read this binding, but I need it alive for its `Drop` implementation."*

## Why This Works

Rust variables are dropped at the end of their enclosing scope (unless moved). `_guard` lives until the `}` above, so the `MutexGuard` stays alive and keeps the mutex locked. When `_guard` drops, `MutexGuard::drop()` automatically unlocks the mutex.

## Common Mistake: `let _ = ...`

This is the **single most common RAII footgun** in Rust:

| Pattern | Effect | Correct? |
|---------|--------|----------|
| `let guard = lock.lock()` | Lock held until `guard` drops; compiler warns "unused" | ✅ Semantics correct, noisy |
| `let _guard = lock.lock()` | Lock held until `_guard` drops; warning suppressed | ✅ Idiomatic |
| `let _ = lock.lock()` | Lock acquired and **immediately released** on the same line | ❌ Dangerous |

The plain `_` placeholder has **special semantics**: it does not create a binding, so the temporary is dropped right away.

## Real Examples from the Codebase

### Mutex guard

```rust
// File: octopus-cli/src/auth/manager.rs
let _guard = self.refresh_lock.lock().await;
```

### Tracing span

```rust
// Anywhere in the codebase
let _span = tracing::info_span!("processing_request", request_id).entered();
```

### Directory guard

```rust
// Hypothetical example
let _cd = std::env::set_current_dir(&project_dir)?;
// original directory restored when _cd drops
```

## When to Use

- **Mutex/RwLock guards:** You need exclusive access for a block of code.
- **Tracing spans:** You want a span to cover an entire scope.
- **Scope guards** (e.g., `tempfile::TempDir`, `scopeguard::defer`): You need cleanup to run at scope exit.
- **Any type where `Drop` is the primary API:** The value exists solely to trigger side effects on destruction.

## When Explicit `drop()` Is Actually Necessary

`let _guard` is about **suppressing warnings while keeping a value alive**. But sometimes you need the opposite: **dropping a value early, before the end of the scope**.

### Example: releasing a `MutexGuard` before async work

```rust
// File: octopus-cli/src/approval_runtime/runtime.rs
pub fn resolve(&self, request_id: &str, response: ApprovalResponse) -> bool {
    let mut inner = self.inner.lock().unwrap();  // ← acquire lock
    // ... mutate state under lock ...
    let hub = inner.hub.clone();
    let request_id_owned = request_id.to_string();
    drop(inner);  // ← release lock NOW, before async work

    // Publish response to wire hub (may await)
    if let Some(hub) = hub {
        hub.publish(...).await;
    }
}
```

`inner` is a `MutexGuard`. Holding it across `hub.publish(...).await` would **block every other thread** that tries to interact with the approval runtime for the entire duration of the wire publish. The explicit `drop(inner)` releases the lock before the async boundary.

### Example: closing a pipe to signal EOF

```rust
// File: octopus-cli/src/hooks/runner.rs
if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut stdin, &json_input).await {
    tracing::warn!("Hook failed to write stdin: {}", e);
    return None;
}
drop(stdin); // Close stdin so the child sees EOF

match child.wait_with_output().await {
    // ...
}
```

If `stdin` is not dropped, the child process hangs forever waiting for more input. Auto-drop would happen only after `child.wait_with_output().await` returns — which it never would, because the child is waiting for the stdin pipe to close.

### How to spot the problem

Ask these questions when you see a value alive near an `.await`:

1. **Is the type a guard?** (`MutexGuard`, `RwLockWriteGuard`, `parking_lot::RawMutex`, etc.)
2. **Did it come from `.lock()` or `.write()`?** If yes, it holds a lock.
3. **Will `.await` take a long time or yield to the executor?** If yes, the lock stays held while other tasks starve.
4. **Does the value own a resource another process is waiting on?** (pipe, socket, temp file)

If any answer is "yes," you likely need an explicit `drop()` before the await point.

### The anti-example: when explicit `drop` is just documentation

```rust
// File: octopus-cli/src/soul/kimisoul.rs
// Dropping `wire` drops the broadcast senders, which causes the
// recorder task to exit cleanly after flushing.
drop(wire);
```

Here `wire` would be auto-dropped at scope end anyway. The explicit call is a **code comment** — it signals to readers that this destructor has externally-visible side effects (terminating a background task). Removing it changes nothing; keeping it helps readability.

| Pattern | Purpose |
|---------|---------|
| `let _guard = lock.lock()` | Keep guard alive for scope-end drop; suppress unused warning |
| `drop(guard)` before scope end | **Early release** — timing matters (lock, pipe, etc.) |
| `drop(value)` at scope end | **Documentation** — auto-drop would work, but author wants you to notice |

## When NOT to Use

If you **do** call methods on the guard, drop the underscore and use the binding normally:

```rust
let guard = self.state.lock().await;
guard.counter += 1;  // using the guard directly — no underscore needed
```
