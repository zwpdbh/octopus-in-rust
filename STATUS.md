# Octopus Project Status

> **Purpose**: Single source of truth for current phase, active task, and blockers.  
> **Update cadence**: At the end of every session, or whenever task status changes.

---

## At a Glance

| Item | State |
|------|-------|
| **Phase** | Phase 1 — 1:1 Rust rewrite of `kimi-cli` (Brain crate layered; `qqbot-core` integration complete) |
| **Active Task** | Deploy qqbot to AliCloud ECS via `cargo xtask qqbot deploy` |
| **Last Completed** | Verified AliCloud ECS deployment: instance reachable, WebUI/noVNC open, `remote-status`/`remote-doctor` clean; deploy code now opens service ports and rewrites remote config paths |
| **Compilation** | `cargo check --workspace` ✅ |
| **Tests** | `cargo test --workspace` ✅ |
| **Blockers** | Bot needs manual QQ login via SnowLuma WebUI/noVNC before OneBot WebSocket handshake will succeed. Current AliCloud cash balance is ~99.6 CNY; top up to at least 100 CNY before creating a new instance. |

---

## Phase 1 Tracks

| # | Track | Status | Notes |
|---|-------|--------|-------|
| 01 | Core / CLI Entry | 🔄 | |
| 02 | Soul (Agent Core) | 🔄 | |
| 03 | Tools | 🔄 | |
| 04 | Wire / Messaging | 🔄 | |
| 05 | Session Management | 🔄 | |
| 06 | Config / Constants / Exceptions | 🔄 | |
| 07 | LLM Integration | 🔄 | |
| 08 | Auth / OAuth | ✅ | |
| 09 | UI / Shell | 🔄 | |
| 10 | Web / Visualizer | ❌ | |
| 11 | Background Tasks | 🔄 | |
| 12 | Subagents | ✅ | |
| 13 | Hooks | ✅ | Recent refactor completed; see `tasks/completed/refactor-hook-event-split.md` |
| 14 | MCP Client | ✅ | |
| 14 | ACP Server | ❌ | |
| 15 | Utils | 🔄 | |
| 16 | Telemetry | ✅ | |
| 16 | Notifications | 🔄 | |
| 16 | Plugins | ✅ | |
| 16 | Skills | 🔄 | |

Legend: ✅ Complete · 🔄 Partial · ❌ Not Started

---

## Active Task

Deploy qqbot to AliCloud ECS. See [`tasks/qqbot-aliyun-ecs-deployment.md`](./tasks/qqbot-aliyun-ecs-deployment.md) and [`docs/plans/15-qqbot-deployment.md`](./docs/plans/15-qqbot-deployment.md).

Current state:
- [x] Deployment plan approved.
- [x] xtask deploy module, AliCloud CLI wrappers, SSH helpers, provisioning, and remote install implemented.
- [x] systemd service template and remote setup script added.
- [x] `qqbot start --no-daemon` and installed-layout `qqbot-core` resolution added.
- [x] `cargo check`, `cargo test`, and targeted `cargo clippy` pass.
- [x] Real ECS deployment test passed (instance reachable, service ports open, status/doctor clean).
- [x] Deploy code now opens SnowLuma service ports from `allowed_service_cidr` and rewrites `config.toml` paths for the remote layout.
- [x] Added `cargo xtask qqbot deploy --fresh` for clean redeploys from a new machine.
- [ ] Bot needs manual QQ login via SnowLuma WebUI/noVNC.
- [ ] Move task to `tasks/completed/` after verification.

---

## Recent Decisions

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-06-12 | Split `HookEvent` into `HookEventKind` (registry key) + `HookEvent` (runtime payload) | Removed dual-role confusion; `trigger(event)` no longer needs a separate `matcher_value` |
| 2026-06-12 | Unify server and wire hooks under `Hook` trait | Single dispatch path; easier to add new hook backends later |
| 2026-06-12 | Add `_template.md` root bootstrap template | Enables copying the enforced project structure into future projects without referencing octopus directly |

---

## How to Verify State

```bash
./scripts/project-status.sh
```

---

## Cross-References

- Project constitution: [`AGENTS.md`](./AGENTS.md)
- Rewrite tracker: [`docs/tracking/index.md`](./docs/tracking/index.md)
- Strategic plans: [`docs/plans/00-index.md`](./docs/plans/00-index.md)
- Feature checklist: [`docs/plans/13-feature-checklist.md`](./docs/plans/13-feature-checklist.md)
