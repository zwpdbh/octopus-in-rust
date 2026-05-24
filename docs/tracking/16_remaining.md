# 16 — Telemetry / Notifications / Plugins / Skills

## Status: 🔄 Partial

---

## Telemetry

### Status: ✅ Complete

### Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/telemetry/mod.rs` | Global state, `track!` macro, context | ~258 | ✅ |
| `octopus-cli/src/telemetry/sink.rs` | EventSink with buffering + enrichment | ~164 | ✅ |
| `octopus-cli/src/telemetry/transport.rs` | HTTP POST + retry + disk fallback | ~322 | ✅ |

### What's Done

- [x] `track!` macro with Python-like ergonomics
- [x] HTTP transport with exponential backoff
- [x] Disk fallback for failed events (~/.kimi/telemetry/)
- [x] Startup retry of disk events
- [x] Context enrichment (app, version, platform, model)
- [x] Wired call sites: tool_call, tool_call_dedup_detected, turn_interrupted, api_error, compaction_finished, compaction_failed

### What's Missing

- [ ] Crash reporting (panic hook)
- [ ] Opt-out configuration

---

## Notifications

### Status: 🔄 Partial

### Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/notifications/mod.rs` | Notification models | ~80 | ✅ |
| `octopus-cli/src/notifications/store.rs` | File-based JSON store | ~120 | ✅ |
| `octopus-cli/src/notifications/manager.rs` | Claim/ack/deliver/recover | ~176 | ✅ |
| `octopus-cli/src/notifications/llm.rs` | `build_notification_message()` | ~64 | ✅ |

### What's Done

- [x] Notification models (Event, Delivery, View, SinkState)
- [x] Persistent store (file-based JSON)
- [x] Delivery to LLM context in `_step()`
- [x] `build_notification_message()` with XML format
- [x] Ack on context restore
- [x] Dedupe key support

### What's Missing

- [ ] Delivery to wire (pump loop)
- [ ] LLM-generated notification summaries
- [ ] Background task output tailing in notification messages

---

## Plugins

### Status: ❌ Not Started

### What's Missing

- [ ] Plugin discovery / loading
- [ ] Plugin tool exposure
- [ ] `kimi plugin` CLI subcommand

---

## Skills

### Status: 🔄 Partial

### Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/skills/mod.rs` | Skill discovery + registry | ~180 | ✅ |

### What's Done

- [x] Skill discovery from `~/.kimi/skills/` and `<work_dir>/.kimi/skills/`
- [x] Subdirectory form (`<name>/SKILL.md`) and flat form (`<name>.md`)
- [x] YAML frontmatter parsing for name/description
- [x] `format_for_system_prompt()` for LLM context injection

### What's Missing

- [ ] `skill:*` slash command namespace
- [ ] Skill context injection at runtime (currently only injects into system prompt)
