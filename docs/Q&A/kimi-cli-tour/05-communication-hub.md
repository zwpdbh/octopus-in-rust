# Tour 5: The Communication Hub — Wires, Notifications, and Broadcasts

> *"A building is only as good as its intercom system. This one has a broadcast channel in every wall."*

Welcome to the **Communication Hub** — the third floor, where messages flow between rooms. This floor houses three systems:
1. **Wire Protocol** — the building's intercom (real-time event streaming)
2. **RootWireHub** — the central broadcast station
3. **Notification Manager** — the mailroom (persistent message delivery)

---

## 📡 The Intercom: Wire Protocol

File: `octopus-cli/src/wire/` — `mod.rs`, `channel.rs`, `file.rs`

The **wire** is a real-time event stream that connects the Control Room (`KimiSoul`) to every other room. Every LLM token, every tool call, every status update travels through the wire.

### The Broadcast Channel

```rust
// File: octopus-cli/src/wire/channel.rs
pub struct Wire {
    raw: broadcast::Sender<serde_json::Value>,      // All events
    merged: broadcast::Sender<serde_json::Value>,   // For file recording
    recorder: Option<WireRecorder>,
}

pub struct WireSoulSide {
    raw_tx: broadcast::Sender<serde_json::Value>,
    merged_tx: broadcast::Sender<serde_json::Value>,
}

pub struct WireUISide {
    raw_rx: broadcast::Receiver<serde_json::Value>,
}
```

🐍 **Python's way:** `asyncio.Queue` with custom `Wire` class. Soul pushes to queue, UI polls.

🦀 **Rust's way:** `tokio::sync::broadcast` — a multi-producer, multi-consumer channel. The soul sends once; every subscriber receives a copy.

✨ **Where Rust shines:** **Broadcast is zero-copy for receivers.** When the soul sends a message, Tokio clones the `Arc<serde_json::Value>` (one atomic increment) for each subscriber. In Python, every queue `put()` serializes the message for each consumer.

### The Global Dispatcher

```rust
// File: octopus-cli/src/wire/mod.rs
thread_local! {
    static CURRENT_WIRE_SOUL_SIDE: RefCell<Option<WireSoulSide>> = const { RefCell::new(None) };
}

pub fn wire_send<T: Serialize>(event: T) {
    CURRENT_WIRE_SOUL_SIDE.with(|cell| {
        if let Some(ref side) = *cell.borrow() {
            let _ = side.send(event);
        }
    });
}
```

This is the **global wire dispatcher**. Any code — deep inside a tool, inside the LLM provider, anywhere — can call `wire_send(MyEvent)` and it reaches the current wire channel.

🐍 **Python's way:** Pass the wire object down through every function call. Or use a global variable (which Python makes easy but dangerous).

🦀 **Rust's way:** `thread_local!` storage. Each turn sets the current wire soul side at startup. This is **safe** because `thread_local` is per-async-task (Tokio tasks are pinned to threads), and the `RefCell` ensures no concurrent access.

### The Wire File: Persistent Log

```rust
// File: octopus-cli/src/wire/file.rs
pub struct WireFile {
    path: PathBuf,
}

impl WireFile {
    pub async fn append(&mut self, record: &serde_json::Value) -> io::Result<()> {
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        let line = serde_json::to_string(record)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.sync_data().await?;
        Ok(())
    }
}
```

Every wire event is **appended to `wire.jsonl`** in the session directory. This is the building's **black box recorder** — you can replay any conversation by reading this file.

---

## 📢 The Broadcast Station: `RootWireHub`

File: `octopus-cli/src/wire/hub.rs` + `wire/types.rs`

While the wire connects soul → UI for a single turn, the `RootWireHub` connects **out-of-turn events**:

- Approval requests (sent while the soul is running)
- Notifications (background tasks completing)
- Status updates (MCP loading progress)

```rust
// File: octopus-cli/src/wire/hub.rs
#[derive(Clone)]
pub struct RootWireHub {
    tx: broadcast::Sender<serde_json::Value>,
}

impl RootWireHub {
    pub fn subscribe(&self) -> broadcast::Receiver<serde_json::Value> {
        self.tx.subscribe()
    }

    pub fn publish<T: Serialize>(&self, event: T) {
        let _ = self.tx.send(serde_json::to_value(event).unwrap());
    }
}
```

The hub is **session-scoped** — one per `KimiSoul`. Both the soul and the UI hold a clone of the hub. The soul publishes events; the UI subscribes and reacts.

---

## 📬 The Mailroom: Notification Manager

File: `octopus-cli/src/notifications/manager.rs` (~176 lines)

The Notification Manager handles **persistent, deliverable messages**. Unlike wire events (transient, broadcast), notifications are:
- **Stored on disk** (`~/.kimi/notifications/<session_id>/`)
- **Claimed by sinks** (llm, wire, notifier)
- **Acked after delivery** (so they're not redelivered)

### The Lifecycle

```
Event published
    → NotificationManager stores JSON on disk
        → Sink claims the notification (locks it)
            → Sink delivers it (e.g., injects into LLM context)
                → Sink acks it (marks as delivered)
```

```rust
// File: octopus-cli/src/notifications/manager.rs
pub fn claim_for_sink(&self, sink: &str, limit: usize) -> Vec<NotificationView> {
    for view in self.store.list_views().into_iter().rev() {
        let sink_state = view.delivery.sinks.get(sink)?;
        if matches!(sink_state.status, Pending) {
            // Lock it!
            let mut delivery = view.delivery.clone();
            delivery.sinks.get_mut(sink).unwrap().status = Claimed(now);
            self.store.write_delivery(&view.event.id, &delivery)?;
            claimed.push(view);
        }
    }
    claimed
}
```

🐍 **Python's way:** SQLite or file-based store with `asyncio` locks.

🦀 **Rust's way:** File-based JSON store. Each notification is a directory with `event.json` and `delivery.json`. Claim/ack is a file write.

✨ **Where Rust shines:** **Claim recovery.** If a process crashes while a notification is claimed but not acked, it stays claimed forever... unless we recover! The manager checks `claimed_at` timestamps on startup:

```rust
// File: octopus-cli/src/notifications/manager.rs
if now - claimed_at > stale_after {
    *sink_state = NotificationSinkState::pending();  // Reclaim!
}
```

This **at-least-once delivery** guarantee is robust against crashes — no SQLite transaction needed.

### LLM Delivery

Notifications are injected into the LLM context as XML:

```rust
// File: octopus-cli/src/notifications/llm.rs
pub fn build_notification_message(view: &NotificationView) -> Message {
    let lines = vec![
        format!(r#"<notification id="{}" category="{}" type="{}" ...>"#, ...),
        format!("Title: {}", event.title),
        format!("Severity: {}", event.severity),
        event.body.clone(),
        "</notification>".to_string(),
    ];
    Message {
        role: "user".to_string(),
        content: vec![ContentPart::Text { text: lines.join("\n") }],
    }
}
```

The LLM sees these as **system reminders** and can act on them — e.g., "A background task just finished with an error. Should I show you the output?"

---

## 🎁 Souvenir Shop: What to Remember

1. **Wire = transient broadcast, Notifications = persistent delivery.** Wire events disappear after the turn ends. Notifications survive crashes and are redelivered if lost.
2. **Thread-local wire isolation.** Each turn has its own wire channel. No accidental cross-talk between concurrent souls.
3. **Claim/ack is a distributed systems pattern.** The notification manager implements at-least-once delivery with stale recovery — all in ~176 lines of Rust.
4. **RootWireHub decouples the soul from the UI.** The soul doesn't know if the UI is TUI, web, or IDE. It just publishes events.

---

## 🚶 Next Stop

The Communication Hub connects all floors. Now let's descend to the **Workshop** — where background tasks and subagents are forged.

→ [Tour 6: The Workshop](./06-workshop.md)
