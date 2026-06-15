# Octopus Project Status

> **Purpose**: Single source of truth for current phase, active task, and blockers.  
> **Update cadence**: At the end of every session, or whenever task status changes.

---

## At a Glance

| Item | State |
|------|-------|
| **Phase** | Phase 1 — 1:1 Rust rewrite of `kimi-cli` (Brain crate layered; `qqbot-core` integration complete) |
| **Active Task** | Brain crate Phase 2 — migrate `octopus-cli` `KimiSoul` to consume `Brain` |
| **Last Completed** | `brain` crate layered into core/tools/session/hooks with streaming + approval traits; `qqbot-core` updated to `ToolSource` API |
| **Compilation** | `cargo check --workspace` ✅ |
| **Tests** | `cargo test -p octopus-cli` 23 passing |
| **Blockers** | None |

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

No active task is currently assigned. The next session should:

1. Read `docs/tracking/index.md` for the overall rewrite map.
2. Pick a track marked 🔄 or ❌.
3. Create `tasks/<short-name>.md` from `tasks/_template.md` before coding.
4. Update this file with the active task name and status.

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
