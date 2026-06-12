# Feature Checklist

> **Purpose**: Track features by priority across the octopus rewrite.  
> **P0** = required for parity with `kimi-cli`.  
> **P1** = important quality-of-life or architectural improvements.  
> **P2** = stretch goals.

---

## P0 — Required for 1:1 Parity

| Track | Feature | Status | Notes |
|-------|---------|--------|-------|
| Core | CLI argument parsing and command dispatch | 🔄 | |
| Core | Configuration loading (`config.toml`) | 🔄 | |
| Soul | Basic turn loop (`KimiSoul::run`) | 🔄 | |
| Soul | Slash command registry | 🔄 | |
| Tools | File read/write | 🔄 | |
| Tools | Shell execution | 🔄 | |
| Tools | Web fetch | 🔄 | |
| Wire | JSON-RPC wire protocol | 🔄 | |
| Session | Session state persistence | 🔄 | |
| Auth | OAuth login flow | ✅ | |
| Hooks | Event-driven hook engine | ✅ | Recently refactored; see `tasks/completed/refactor-hook-event-split.md` |
| MCP | MCP client integration | ✅ | |
| Telemetry | Event emission and sinks | ✅ | |
| Plugins | Example HTTP plugin + discovery | ✅ | |

---

## P1 — Important Improvements

| Track | Feature | Status | Notes |
|-------|---------|--------|-------|
| Hooks | Wire-side hook delivery to GUI clients | ❌ | Subscription plumbing exists; full round-trip pending |
| Hooks | `/hooks` slash command using `hook_engine.details()` | ❌ | Currently lists config hooks only |
| Hooks | Telemetry tracking for hook triggers | ❌ | |
| UI | Shell input history and keybindings | 🔄 | |
| UI | Print mode | 🔄 | |
| Background | Background task manager UI | 🔄 | |
| Notifications | Notification pump and sinks | 🔄 | |
| Skills | Skill registry and loading | 🔄 | |

---

## P2 — Stretch Goals

| Track | Feature | Status | Notes |
|-------|---------|--------|-------|
| Web | Real-time visualizer | ❌ | |
| ACP | ACP server mode | ❌ | |
| Utils | Port remaining Python utilities | 🔄 | Currently only ~80 LOC ported |

---

## Legend

- ✅ Complete
- 🔄 Partial / In Progress
- ❌ Not Started
