# Octopus Project Status

> **Purpose**: Single source of truth for current phase, active task, and blockers.  
> **Update cadence**: At the end of every session, or whenever task status changes.

---

## At a Glance

| Item | State |
|------|-------|
| **Phase** | Phase 1 — 1:1 Rust rewrite of `kimi-cli` (Brain crate layered; `qqbot-core` integration complete) |
| **Active Task** | Create strategic plan for the `faf-party` plugin |
| **Last Completed** | Deployed qqbot to AliCloud ECS with group-specific health checks and periodic progress feedback |
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

Create strategic plan for the `faf-party` plugin. See [`docs/plans/16-faf-party-plugin.md`](./docs/plans/16-faf-party-plugin.md).

Current state:
- [x] Availability parsing semantics defined (Chinese expressions, 22:00 default end, past-time rollover).
- [x] Plugin/host architecture implemented (WASM parser + host timer/notifications).
- [x] Implementation steps and success criteria documented.
- [x] Plugin built and deployed to ECS.
- [ ] Open questions answered before next iteration.

## Previous Task

Deploy qqbot to AliCloud ECS. See [`tasks/qqbot-aliyun-ecs-deployment.md`](./tasks/qqbot-aliyun-ecs-deployment.md) and [`docs/plans/15-qqbot-deployment.md`](./docs/plans/15-qqbot-deployment.md).

State:
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
| 2026-06-18 | Fix per-step progress timer reset in `group_brain.rs` | Progress heartbeat now spans the whole turn, preventing multiple “Still checking...” messages 30s apart when tools are unchanged |
| 2026-06-18 | Implement `faf-party` plugin and host service | WASM parser extracts intent/availability; `FafPartyHostService` tracks candidates and notifies when 6 players overlap |
| 2026-06-18 | Add `faf_party_status` host tool | LLM can now query the current candidate list, enabling responses to "现在有多少人了？" |

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
