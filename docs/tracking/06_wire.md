# Phase 6: Wire & Messaging

## Status: COMPLETE

## Implementation

### `wire/channel.rs` — Real-time wire channel

- **`Wire`** — spmc broadcast channel using `tokio::sync::broadcast`
  - `raw` queue: all messages as-is
  - `merged` queue: for file recording (merge optimization reserved for future)
  - `WireRecorder` task consumes merged queue → appends to `WireFile`
- **`WireSoulSide`** — `Clone`, non-blocking `send(msg)` publishes to both queues
- **`WireUISide`** — async `recv()` for UI consumers

### `wire/mod.rs` — Wire types + global dispatcher

- **`wire_send()`** — serializes event to JSON, sends to current `WireSoulSide`
- **Thread-local isolation** — `CURRENT_WIRE_SOUL_SIDE` per run (replaces old file-only isolation)
- **`RootWireHub`** — session-level broadcast hub for out-of-turn messages (approvals, notifications)
  - `subscribe()` / `publish()` / `shutdown()`
- **Wire types**: `TurnBegin`/`End`, `StepBegin`/`Interrupted`/`Retry`, `StatusUpdate`, `CompactionBegin`/`End`, `MCPLoadingBegin`/`End`, `BtwBegin`/`End`, `SteerInput`, `TextPart`, `Notification`, `HookTriggered`, `HookResolved`

### `wire/file.rs` — Wire file backend

- **`WireFile`** — append-only JSONL with metadata header
- **`WireRecorder`** — background task that consumes merged broadcast queue and persists to file
- File write is now sequential through the channel (was fire-and-forget concurrent)

### `notifications/wire.rs` — Notification → wire conversion

- **`to_wire_notification(view)`** — converts `NotificationView` → `Notification` wire event

### `soul/mod.rs` — Wire setup + notification pump

- **`run()`** creates `Wire` with `WireFile` backend, sets `WireSoulSide` as current
- **Notification pump** — background task that calls `notification_manager.deliver_pending("wire", 8, ...)` every 1 second
- Pump is aborted on turn completion; wire senders are dropped, causing recorder to flush and exit

## Architecture Diagram

```
┌─────────────┐     send()      ┌─────────────────┐
│   Soul      │ ──────────────► │  WireSoulSide   │
│ (wire_send) │                 │  (broadcast tx) │
└─────────────┘                 └────────┬────────┘
                                         │
                    ┌────────────────────┼────────────────────┐
                    │                    │                    │
                    ▼                    ▼                    ▼
              ┌─────────┐        ┌─────────────┐       ┌──────────┐
              │ raw rx  │        │ merged rx   │       │ merged rx│
              │ (UI)    │        │ (recorder)  │       │ (UI)     │
              └─────────┘        └──────┬──────┘       └──────────┘
                                        │
                                        ▼
                                  ┌───────────┐
                                  │ WireFile  │
                                  │ wire.jsonl│
                                  └───────────┘
```

## What's Still Deferred

- **Message merging** — Python buffers consecutive `MergeableMixin` messages; Rust sends each message individually
- **JSON-RPC wire server** — `WireServer` over stdio (large feature, needed for ACP/wire clients)
- **WireExternalTool** — external tool calls via wire requests/responses
- **QuestionRequest** — interactive question protocol over wire

These are blocked on Phase 7 (MCP) and Phase 10+ (full wire client protocol).
