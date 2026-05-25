# Agent Guidelines for Octopus

## Rust Best Practices

### Model States with Enums and Match on Them

**Rule:**
1. Represent mutually exclusive states, statuses, and conditions as `enum` variants with associated data.
2. Branch on those enums with `match` (or `if let`). Avoid `if-else` chains built from boolean flags, string comparisons, or integer codes.

Never use scattered booleans, `String` constants, or raw integers to model status.

#### Why

- **Type safety:** Invalid states become unrepresentable.
- **Exhaustiveness:** `match` forces you to handle every variant. Adding a state becomes a compile error, not a silent bug.
- **Readability:** A `match` arm pairs the state with its behavior; `if-else` chains make the reader reconstruct the state matrix.

#### Bad

```rust
// Boolean flags — impossible states allowed
struct Order {
    is_active: bool,
    is_deleted: bool,
}

// String constants — typo-prone
struct Task {
    status: String, // "pending", "running", "done"
}

// Reconstructing state from scattered booleans
let mode = if cli.print { "print" } else if cli.acp { "acp" } else { "shell" };
if mode == "acp" { ... }

// Independent if chains for related conditions
if cli.agent.is_some() && cli.agent_file.is_some() { ... }
if cli.config.is_some() && cli.config_file.is_some() { ... }
```

#### Good

```rust
enum OrderStatus {
    Active { placed_at: DateTime<Utc> },
    Deleted { reason: String, deleted_at: DateTime<Utc> },
}

struct Order {
    id: Uuid,
    status: OrderStatus,
}

match order.status {
    OrderStatus::Active { placed_at } => { /* ... */ }
    OrderStatus::Deleted { reason, deleted_at } => { /* ... */ }
}
```

Model mutually exclusive CLI modes as an enum and branch exhaustively:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Shell,
    Print,
    Acp,
    Wire,
}

match ui_mode {
    UiMode::Print => instance.run_print(...).await,
    UiMode::Acp   => instance.run_acp().await,
    UiMode::Wire  => instance.run_wire_stdio().await,
    UiMode::Shell => instance.run_shell(...).await,
}
```

For multiple related boolean conditions, precompute and match on tuples:

```rust
let agent_conflict  = cli.agent.is_some() && cli.agent_file.is_some();
let config_conflict = cli.config.is_some() && cli.config_file.is_some();

match (agent_conflict, config_conflict) {
    (false, false) => {}
    (true, _) => { /* handle agent conflict */ }
    (_, true) => { /* handle config conflict */ }
}
```

Use `matches!` for one-off predicates:

```rust
if matches!(ui_mode, UiMode::Acp | UiMode::Wire) { /* ... */ }
```

#### Migration Strategy

When refactoring existing code:

1. Define the `enum` covering all valid states.
2. Move associated data into the relevant variant.
3. Replace the old field(s) with a single `status: YourEnum` field.
4. Let the compiler guide you to fix all `match` or `if` sites.
5. Delete dead helper methods (`is_active()`, `set_pending()`, etc.).

#### Exception

Raw primitives are acceptable only at serialization boundaries or external API interop — convert to an enum immediately at the boundary.
