# Agent Guidelines for Octopus

## Rust Best Practices

### Use Enum + Associated Variants to Represent Status

**Rule:** Always represent states, statuses, and mutually exclusive conditions as `enum` types with associated data. Never use boolean flags, string constants, or integer codes to model status.

#### Why

- **Type safety:** Invalid states become unrepresentable.
- **Exhaustive checking:** The compiler forces handling of every variant via `match`.
- **Domain modeling:** Status and its associated data live together, not scattered across fields.
- **Refactoring safety:** Adding a new state causes compile errors in all relevant branches.

#### Bad

```rust
// Boolean flags — allows impossible states (is_active=true && is_deleted=true)
struct Order {
    is_active: bool,
    is_deleted: bool,
    is_pending: bool,
}

// String constants — typo-prone, not exhaustive
struct Task {
    status: String, // "pending", "running", "done"
}

// Integer codes — opaque and error-prone
struct Job {
    status_code: u8, // 0=pending, 1=running, 2=done
}
```

#### Good

```rust
enum OrderStatus {
    Active { placed_at: DateTime<Utc> },
    Deleted { reason: String, deleted_at: DateTime<Utc> },
    Pending { reservation_id: Uuid },
}

struct Order {
    id: Uuid,
    status: OrderStatus,
}
```

Use `match` exhaustively:

```rust
match order.status {
    OrderStatus::Active { placed_at } => { /* ... */ }
    OrderStatus::Deleted { reason, deleted_at } => { /* ... */ }
    OrderStatus::Pending { reservation_id } => { /* ... */ }
}
```

#### Migration Strategy

When you encounter existing code using boolean flags, strings, or integer codes for status:

1. Define the `enum` with variants representing all valid states.
2. Move associated data into the relevant variant.
3. Replace the old field(s) with a single `status: YourEnum` field.
4. Let the compiler guide you to fix all `match` or `if` sites.
5. Delete dead helper methods (`is_active()`, `set_pending()`, etc.).

#### Exception

Raw primitives are acceptable only for:
- Serialization boundaries (e.g., `From<YourEnum> for u8` for protocol encoding).
- External API interop where the format is fixed — convert to enum immediately at the boundary.
