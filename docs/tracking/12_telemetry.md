# Phase 12: Telemetry

## Status: COMPLETE

## Implementation

### `telemetry/mod.rs` — Global state & `track()` API

- **Global state** (`OnceLock<Mutex<TelemetryState>>`): zero-dependency module-level state
  - Event queue (max 1000 events before sink attach)
  - Device ID, session ID, client info
  - `session_started` dedup set
- **`track!` macro**: Python-like ergonomics
  - `track!("event")` — no properties
  - `track!("event", key = value, ...)` — with properties
- **`track_event()`**: fire-and-forget, buffers if no sink attached
- **`set_context()` / `set_client_info()`**: identity setup
- **`attach_sink()`**: drains queued events into new sink, flushes old sink sync
- **`disable()` / `flush()` / `flush_sync()`**: lifecycle controls
- **`get_or_create_device_id()`**: persistent device UUID in `~/.kimi/device_id`

### `telemetry/sink.rs` — `EventSink`

- **Buffer**: `Mutex<Vec<Map<String, Value>>>` — thread-safe, non-blocking `accept()`
- **Context enrichment**: app_name, version, runtime, platform, arch, ci, terminal, ui_mode, model
- **Threshold flush**: async flush when buffer reaches 50 events
- **Periodic flush**: background task every 30s
- **`flush_sync()`**: saves to disk fallback (no HTTP — safe for signal handlers)

### `telemetry/transport.rs` — `AsyncTransport`

- **HTTP POST** to `https://telemetry-logs.kimi.com/v1/event`
- **Retry**: exponential backoff `[1s, 4s, 16s]` on transient errors (5xx, 429, network)
- **401 fallback**: retry without auth token (anonymous)
- **4xx drop**: non-retryable client errors are dropped (avoids stuck disk spool)
- **Disk fallback**: `~/.kimi/telemetry/failed_<uuid>.jsonl` — 7-day TTL
- **Startup retry**: `retry_disk_events()` scans and resends persisted events
- **Payload**: `kfc_` server prefix, `property_*` / `context_*` flattening, primitive validation

## Wired Call Sites

| Event | Location | Properties |
|-------|----------|------------|
| `tool_call` | `soul/toolset.rs` | tool_name, outcome, duration_ms |
| `tool_call_dedup_detected` | `soul/toolset.rs` | session_id, turn_id, step_no, tool_name, dup_type, args_hash |
| `turn_interrupted` | `soul/mod.rs` | step_no, session_id |
| `api_error` | `soul/mod.rs` | error_type, status_code, duration_ms |
| `compaction_finished` | `soul/mod.rs` | trigger_type, before_tokens, after_tokens, duration_ms, retry_count |
| `compaction_failed` | `soul/mod.rs` | trigger_type, before_tokens, duration_ms, retry_count, error_type |

## Unblocked Previous Phases

- **Phase 4**: Tool execution timing + telemetry ✅
- **Phase 2**: Compaction telemetry ✅
- **Phase 1**: Turn interruption telemetry ✅
