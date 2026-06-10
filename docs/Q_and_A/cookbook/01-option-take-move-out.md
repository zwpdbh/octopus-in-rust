# Cookbook: Move a Field Out of a Struct with `Option::take()`

## Problem

In Rust, you cannot move a field out of a struct while the struct itself continues to exist:

```rust
pub struct Engine {
    soul: KimiSoul,
}

impl Engine {
    pub fn run(&mut self) {
        let soul = self.soul; // ERROR: cannot move out of `self.soul`
        // ...
    }
}
```

If the struct is borrowed mutably (`&mut self`), moving one field would leave the struct in a partially-invalid state. The compiler rejects this.

## Solution

Wrap the field in `Option`, even if it is logically always present, and use `Option::take()` to move it out:

```rust
pub struct Engine {
    soul: Option<KimiSoul>,
}

impl Engine {
    pub fn run(&mut self) {
        let soul = self.soul.take().expect("soul already consumed");
        // `self.soul` is now `None`, `self` remains valid
        // ...
    }
}
```

## Why This Works

`Option::take()` replaces `self.soul` with `None` and returns the original value. The struct stays valid because every field still has a well-defined value.

## Trade-off: Why Not Consume `self`?

You could consume the entire struct instead:

```rust
pub fn run(self) -> KimiSoul {
    self.soul
}
```

But then the caller loses access to the struct for any post-operation work (logging, cleanup, inspecting other fields). `Option::take()` lets you keep the struct alive while still transferring ownership of the inner value.

## When to Use

- A struct owns a heavy/resourceful object that must be handed off to exactly one consumer.
- The struct itself must remain usable after the hand-off (for cleanup, reporting, etc.).
- You want the compiler to enforce "consume exactly once" at runtime (via `None` check).
