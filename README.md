# Octopus

[![CI](https://github.com/zwpdbh/octopus-in-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/zwpdbh/octopus-in-rust/actions/workflows/ci.yml)

> A personal Rust agent platform: a single workspace that grew from a faithful
> rewrite of [kimi-cli](https://github.com/MoonshotAI/kimi-cli) into a full
> agent ecosystem — a terminal coding agent, a reusable agent runtime, a QQ
> group bot, and a FAF game-companion web app.

**Stack**: Rust (edition 2024), Tokio, Axum, Dioxus 0.7 (WASM), ratatui, Extism (WASM plugins), serde, clap.

---

## What's Inside

### Applications (`apps/`)

| App | Binary | What it does |
|-----|--------|--------------|
| `octopus-cli` | `octopus` | Terminal AI coding agent — a faithful Rust rewrite of `kimi-cli`. ratatui TUI, JSON-RPC wire protocol over stdio, hand-rolled MCP client (child processes over stdio), hooks engine, approval runtime, OAuth device flow, and a tool-use agent loop with 10 tool namespaces (shell, file, web, plan, background tasks, ...). |
| `qqbot` | `qqbot` | Supervisor CLI for the QQ bot service: init/daemon/doctor/health/logs/status, manages SnowLuma (OneBot) and `qqbot-core`. Deployed on AliCloud ECS via Docker + systemd. |
| `qqbot-core` | — | QQ bot runtime: OneBot v11 over WebSocket with auto-reconnect, per-group LLM brains with group memory, Extism WASM plugin tools, Unix-domain control socket, SIGHUP reload. |
| `fafcn-server` | `fafcn-server` | Axum backend for the FAF community app: units API, build-order simulation streamed over WebSocket, LLM Q&A streamed over SSE (agent + `faf-units` WASM plugin). |
| `fafcn-web` | — | Dioxus 0.7 frontend compiled to WASM: unit browser, build-plan editor, live simulation view, streaming agent Q&A chat. |
| `faf-sim-cli` | `faf-sim` | CLI for the deterministic FAF build-queue economy simulator. |
| `breakout` | — | Breakout game in Bevy, used to explore debug-stepping. |

### Libraries (`crates/`)

| Crate | Purpose |
|-------|---------|
| `agent-core` | Reusable agent runtime (`Brain`): streaming turn events, multi-step tool-use loop, retry/recovery/checkpoint policies, approval runtime, system-prompt policies, Extism WASM plugin tools. Consumed by `octopus-cli`, `qqbot-core`, and `fafcn-server`. |
| `llm-provider` | LLM provider abstraction: OpenAI-compatible (legacy + responses) and Kimi subscription protocols, streaming messages, tool calls, token usage, mock/echo providers for tests. |
| `faf-blueprints` / `faf-game-engine` / `faf-units` | FAF domain layer: raw unit data parsing, unit computation helpers, high-level game features. |
| `faf-sim-protocol` / `faf-sim-service` | Simulation event protocol and streaming simulation service. |
| `faf-dioxus-ui` | Reusable Dioxus components: `chat_primitives` (presentational chat UI), `agent_chat` (config-driven streaming chat page: SSE client, session state, localStorage), markdown renderer, charts. |
| `faf-downloader` | CLI for downloading and persisting FAF unit data. |
| `qqbot-config` | Shared qqbot configuration types. |

### Plugins (`plugins/`)

Extism WASM plugins (`extism-pdk`, `#[plugin_fn]`) loaded by the agent runtime as sandboxed tools:

- `faf-units` — query and compare FAF units (used by the Q&A agent and qqbot).
- `faf-party` — parse FAF party availability and manage candidate state.
- `example-http` — reference plugin implementation.

### Tooling

- `xtask/` — workspace automation commands.
- `scripts/` — project status reporter, qqbot release/deploy helpers, systemd unit.

---

## Quick Start

```bash
# Verify the workspace compiles and tests pass
cargo check --workspace
cargo test --workspace

# Run the terminal agent
cargo run -p octopus-cli

# Run the FAF web app (backend on :3000, frontend via dx)
cargo run -p fafcn-server
cd apps/fafcn-web && dx serve

# QQ bot supervisor (see docs/Q_and_A/qqbot for setup)
cargo run -p qqbot -- --help

# Automated project state (build/test/git)
./scripts/project-status.sh
```

---

## Project Docs

| Need | File |
|------|------|
| Coding conventions and locked decisions | [`AGENTS.md`](./AGENTS.md) |
| Current phase, active task, blockers | [`STATUS.md`](./STATUS.md) |
| Task specs | `tasks/` (`tasks/completed/` for archive) |
| Rust patterns cookbook | [`docs/Q_and_A/cookbook/`](./docs/Q_and_A/cookbook/) |
| Hook system deep-dive | [`docs/Q_and_A/hook-system/`](./docs/Q_and_A/hook-system/) |
| kimi-cli architecture tour | [`docs/Q_and_A/kimi-cli-tour/`](./docs/Q_and_A/kimi-cli-tour/) |
| QQ bot operator guide | [`docs/Q_and_A/qqbot/`](./docs/Q_and_A/qqbot/) |

---

## License

MIT
