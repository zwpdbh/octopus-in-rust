# 16 — Telemetry / Notifications / Plugins / Skills

## Status: ⬜ Not Started

## Telemetry

### Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/telemetry/__init__.py` | Telemetry entry | ~100 |
| `kimi_cli/transport.py` | Transport layer | ~100 |
| `kimi_cli/sink.py` | Event sink | ~100 |
| `kimi_cli/crash.py` | Crash reporting | ~100 |

### Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/telemetry/mod.rs` | Telemetry placeholder | ~? | ⬜ Empty / stub |

### What's Missing

- [ ] Event tracking (`track()` calls)
- [ ] Telemetry transport (HTTP batching)
- [ ] Crash reporting (panic hook)
- [ ] Opt-out configuration

---

## Notifications

### Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/notifications/__init__.py` | Notifications package | ~50 |
| `kimi_cli/notifications/manager.py` | Notification manager | ~200 |
| `kimi_cli/notifications/store.py` | Notification store | ~150 |
| `kimi_cli/notifications/models.py` | Notification models | ~100 |
| `kimi_cli/notifications/notifier.py` | Notifier | ~100 |
| `kimi_cli/notifications/wire.py` | Wire delivery | ~50 |
| `kimi_cli/notifications/llm.py` | LLM notification helpers | ~50 |

### Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/notifications/mod.rs` | Notifications placeholder | ~? | ⬜ Empty / stub |

### What's Missing

- [ ] Notification manager
- [ ] Persistent store
- [ ] Delivery to wire (pump loop)
- [ ] LLM-generated notification summaries

---

## Plugins

### Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/plugin/__init__.py` | Plugin package | ~50 |
| `kimi_cli/plugin/manager.py` | Plugin manager | ~200 |
| `kimi_cli/plugin/tool.py` | Plugin tool exposure | ~100 |
| `kimi_cli/cli/plugin.py` | `kimi plugin` command | ~100 |

### Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/plugin/mod.rs` | Plugin placeholder | ~? | ⬜ Empty / stub |

### What's Missing

- [ ] Plugin discovery / loading
- [ ] Plugin tool exposure
- [ ] `kimi plugin` CLI subcommand

---

## Skills

### Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/skill/__init__.py` | Skill package | ~100 |
| `kimi_cli/skills/kimi-cli-help/SKILL.md` | CLI help skill | ~200 |

### Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/skills/mod.rs` | Skills placeholder | ~? | ⬜ Empty / stub |

### What's Missing

- [ ] Skill discovery (`SKILL.md` parsing)
- [ ] Skill registry
- [ ] `skill:*` slash command namespace
- [ ] Skill context injection
