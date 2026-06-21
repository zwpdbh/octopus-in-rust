# Octopus — Rust Rewrite of `kimi-cli`

> **One-line pitch**: A ground-up rewrite of the Kimi CLI agent into a unified Rust workspace.  
> **Current Phase**: Phase 1 — 1:1 Python-to-Rust rewrite  
> **Last Updated**: 2026-06-12

---

## What This Project Is

`kimi-cli` is a Python AI-agent command-line interface. **Octopus** consolidates the core runtime, the LLM client crate, plugins, and documentation tooling into a single Rust workspace.

| Original | Language | Octopus Replacement |
|----------|----------|---------------------|
| `kimi-cli` (core runtime) | Python | `octopus-cli` (Tokio + clap) |
| `kimi-cli` LLM/model layer | Python | `kosong` (standalone LLM crate) |
| `kimi-cli` plugin loader | Python | `qqbot-plugins/example-http` + discovery in `octopus-cli` |
| Documentation cross-reference | Manual | `docref` (docref tool) |

**Strategy**: 1:1 port first, refactor later.

---

## LLM Agent Project Health Checklist

> **Purpose**: This is not a feature checklist. It is a **structural and management compliance checklist**.  
> Any LLM entering this project must verify these items before and during work.  
> If any item is violated, fix it before proceeding.

### 🔴 Pre-Flight Check (Do This First)

Run these checks **before writing any code** after a reconnect:

- [ ] **Constitution loaded**: Read [`AGENTS.md`](./AGENTS.md) for architecture decisions, naming rules, coding standards.
- [ ] **Status loaded**: Read [`STATUS.md`](./STATUS.md) for current phase, active task, blockers.
- [ ] **Task spec loaded**: If an active task exists, read `tasks/<active-task>.md` for detailed task context.
- [ ] **Real state verified**: Run `./scripts/project-status.sh` to confirm build/test/git state matches `STATUS.md`.

> ⚠️ **All four must pass. Do not code if any are missing.**

### 🟡 Structural Compliance Check (Verify These Files Exist)

These files form the project's management backbone. If any are missing, recreate them:

| File | Purpose | Must Exist |
|------|---------|------------|
| `AGENTS.md` | Project constitution (rules, conventions, locked decisions) | ✅ |
| `STATUS.md` | Current operational status (phase, active task, blockers) | ✅ |
| `tasks/<active>.md` | Detailed spec for the in-progress task | ✅ when active |
| `tasks/completed/` | Archive directory for finished tasks | ✅ |
| `scripts/project-status.sh` | Automated build/test/git state reporter | ✅ |
| `tasks/_template.md` | Template for new task specs | ✅ |
| `docs/tracking/index.md` | 1:1 rewrite tracker | ✅ |
| `docs/plans/00-index.md` | Strategic plan series index | ✅ |
| `docs/plans/13-feature-checklist.md` | P0/P1/P2 feature tracker | ✅ |

### 🟢 Ongoing Session Compliance (Check Throughout Work)

During every session, verify:

- [ ] **Task scope**: Only work on the active task in `STATUS.md`. Do not drift into other phases.
- [ ] **State updates**: Update task spec's "Completed Steps" in real time. Update `STATUS.md` at session end.
- [ ] **Compile gate**: `cargo check --workspace` passes after every meaningful change.
- [ ] **Test gate**: `cargo test -p octopus-cli` passes before declaring progress.
- [ ] **Crate boundaries**: Protocol/event types live in the right crate; `kosong` stays independent of `octopus-cli` specifics.
- [ ] **No schema changes**: Do NOT introduce breaking config or wire-protocol changes without documenting them.
- [ ] **Git hygiene**: Check `git status --short`. No unintended files modified.

### 🔵 Session End Compliance (Before Stopping)

Before ending any session, run this checklist:

- [ ] Task spec updated with all completed steps and decisions made?
- [ ] `STATUS.md` updated if task status, blockers, or phase changed?
- [ ] `./scripts/project-status.sh` output saved or noted in task spec?
- [ ] `cargo check --workspace` passes?
- [ ] `cargo test -p octopus-cli` passes (or new tests added + passing)?
- [ ] All modified files are intentional? (review `git status`)

---

## Project Status at a Glance

```text
Phase 1 — 1:1 Python-to-Rust Rewrite
├── Core / CLI Entry        🔄
├── Soul (Agent Core)       🔄
├── Tools                   🔄
├── Wire / Messaging        🔄
├── Session Management      🔄
├── Config / Exceptions     🔄
├── LLM Integration         🔄
├── Auth / OAuth            ✅
├── UI / Shell              🔄
├── Web / Visualizer        ❌
├── Background Tasks        🔄
├── Subagents               ✅
├── Hooks                   ✅
├── MCP Client              ✅
├── ACP Server              ❌
├── Utils                   🔄
├── Telemetry               ✅
├── Notifications           🔄
└── Plugins                 ✅
```

- **Compilation**: `cargo check --workspace` ✅ passes
- **Tests**: `cargo test -p octopus-cli` 23 tests passing
- **Active Task**: *TBD — pending next priority* (see [`STATUS.md`](./STATUS.md))

For full operational status, read [`STATUS.md`](./STATUS.md).

---

## Architecture Decisions (Locked — Do Not Change Without Approval)

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust edition 2024 (plugins currently 2021) | Single toolchain, type safety across boundaries |
| Async runtime | Tokio | Used throughout `octopus-cli` and `kosong` |
| CLI framework | clap | Standard Rust CLI parsing |
| LLM crate | `kosong` | Standalone crate decoupled from CLI specifics |
| Plugin model | WASM-compatible dynamic crates | `qqbot-plugins/example-http` is the reference implementation |
| Config format | TOML | Matches `kimi-cli` and Rust ecosystem conventions |
| Wire protocol | JSON-RPC over stdio | Matches `kimi-cli` wire protocol |
| Rewrite strategy | 1:1 structure first | Port faithfully, then refactor |

Full conventions and coding standards: [`AGENTS.md`](./AGENTS.md).

---

## Quick Start

```bash
# Verify workspace compiles
cargo check --workspace

# Run octopus-cli tests
cargo test -p octopus-cli

# Get automated project state (compilation, tests, git, active tasks)
./scripts/project-status.sh
```

---

## Workspace Layout

```
octopus/
├── AGENTS.md              ← Project constitution (rules, conventions, decisions)
├── STATUS.md              ← Current operational status (phase, task, blockers)
├── README.md              ← This file (compliance checklist + overview)
├── Cargo.toml             ← Workspace manifest
│
├── octopus-cli/           ← Main CLI runtime
├── kosong/                ← Standalone LLM / model crate
├── qqbot-plugins/         ← Plugin crates
│   ├── example-http/      ← Reference plugin implementation
│   └── summary/           ← Default qqbot plugin: conversation summary
├── docref/                ← Documentation cross-reference tool
├── qqbot/                 ← QQ bot supervisor (SnowLuma + Wasm plugins)
└── qqbot-core/            ← QQ bot runtime with Wasm plugin host

├── tasks/                 ← Active task specs
│   ├── _template.md
│   └── completed/         ← Finished tasks
│
├── scripts/               ← Automation
│   └── project-status.sh  ← Build/test/git state reporter
│
├── docs/
│   ├── plans/             ← Strategic plans (00-index, 13-feature-checklist)
│   ├── tracking/          ← 1:1 rewrite tracker (index.md + per-area files)
│   ├── Q_and_A/           ← Deep-dive docs (e.g., hook-system)
│   └── todo/              ← Scratch notes and comparisons
│
└── .git/                  ← Repository metadata
```

---

## Reference Map

| Need | File |
|------|------|
| Coding conventions, error handling, naming rules | [`AGENTS.md`](./AGENTS.md) |
| What phase we're in, what's blocked | [`STATUS.md`](./STATUS.md) |
| Detailed spec for current task | `tasks/<active-task>.md` |
| 1:1 rewrite tracker | [`docs/tracking/index.md`](./docs/tracking/index.md) |
| Strategic plans and feature checklist | [`docs/plans/00-index.md`](./docs/plans/00-index.md) |
| Hook system deep-dive | [`docs/Q_and_A/hook-system/01-index.md`](./docs/Q_and_A/hook-system/01-index.md) |
| Bootstrap template for future projects | [`_template.md`](./_template.md) |

---

## License

MIT
