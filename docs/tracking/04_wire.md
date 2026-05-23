# 04 — Wire / Messaging

## Status: 🔄 Partial

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/wire/__init__.py` | Wire channel, send/receive | ~200 |
| `kimi_cli/wire/types.py` | Wire message types (TextPart, StatusUpdate, etc.) | ~400 |
| `kimi_cli/wire/file.py` | Wire file backend | ~100 |
| `kimi_cli/wire/protocol.py` | Wire protocol definitions | ~150 |
| `kimi_cli/wire/server.py` | Wire server (slash command routing) | ~200 |
| `kimi_cli/wire/root_hub.py` | Root hub for multi-agent | ~100 |
| `kimi_cli/wire/jsonrpc.py` | JSON-RPC layer | ~100 |
| `kimi_cli/wire/serde.py` | Serialization helpers | ~100 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/wire/mod.rs` | Wire types, `wire_send()` global | ~238 | 🔄 Types defined; global sender simplified |

## What's Done

- [x] Core wire types (`TextPart`, `StatusUpdate`, `TurnBegin`, `TurnEnd`, etc.)
- [x] `wire_send()` global function (simplified from Python's ContextVar-based wire)
- [x] Tool result / tool call types
- [x] Message / ContentPart types

## What's Missing

- [ ] Wire channel with backpressure (`Wire` class with soul_side/ui_side queues)
- [ ] Wire file backend (`wire.jsonl` streaming)
- [ ] Wire server for web/vis communication
- [ ] Root hub for multi-agent routing
- [ ] JSON-RPC protocol layer
- [ ] ContextVar equivalent for per-run wire isolation
