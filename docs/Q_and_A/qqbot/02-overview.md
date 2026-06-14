# 1. What is qqbot?

`qqbot` is a self-hosted QQ group-bot solution for Linux. It bundles three moving parts:

| Component | Responsibility |
|-----------|----------------|
| `qqbot` CLI | Supervisor daemon: starts, monitors, and reloads everything. |
| SnowLuma (Docker) | The QQ protocol bridge. It runs the QQ client and exposes a OneBot 11 WebSocket API on `ws://127.0.0.1:3001`. |
| `qqbot-core` | The bot runtime. It connects to OneBot, dispatches group messages/commands to WASM plugins, and talks to the LLM. |

```text
┌─────────────────────────────────────────────┐
│                 qqbot                       │
│  (supervisor daemon: init/start/stop/      │
│   monitor/reload + plugin management)       │
└──────┬──────────────────────┬───────────────┘
       │                      │
       ▼                      ▼
┌──────────────┐      ┌─────────────────┐
│   SnowLuma   │      │   qqbot-core    │
│   (Docker)   │      │   (bot runtime) │
└──────┬───────┘      └────────┬────────┘
       │                       │
       │ OneBot 11             │ loads
       │ WebSocket             │ .wasm
       ▼                       ▼
┌─────────────────────────────────────────┐
│            Wasm plugins                 │
│  (summary, moderation, welcome, etc.)   │
└─────────────────────────────────────────┘
```

## Why this shape?

- **QQ protocol is handled once.** SnowLuma encapsulates the NTQQ client and the OneBot 11 protocol, so the rest of the code only speaks OneBot.
- **Business logic is pluggable.** Plugins are WASM modules. You can add, remove, or update them without rebuilding `qqbot` or `qqbot-core`.
- **Supervisor handles crashes.** `qqbot` runs as a daemon. If the SnowLuma container or `qqbot-core` exits, the supervisor restarts it.
- **Operations are CLI-driven.** Everything from first-time setup to health checks to plugin reloads is driven by `qqbot <subcommand>`.

## Data layout

Runtime data lives under `./data/` by default:

```text
./data/
├── qqbot-data/
│   ├── config.toml          # qqbot-core configuration
│   └── plugins/             # enabled WASM plugins
├── snowluma-data/           # SnowLuma/QQ session state
│   └── config/onebot.json   # OneBot server configuration
├── run/
│   ├── qqbot.pid            # supervisor daemon pid
│   └── qqbot-core.pid       # qqbot-core pid (for plugin reload)
└── logs/
    ├── core.log             # qqbot-core stdout/stderr
    ├── supervisor.log       # supervisor daemon logs
    └── snowluma.log         # SnowLuma container logs
```

The `data/` directory is gitignored because it contains login state and secrets.
