# Brain Crate Implementation Tracker

Tracks the extraction of a reusable **Brain** crate from `octopus-cli` and its integration into `qqbot-core`.

- **Plan:** [`docs/plans/14-brain-architecture.md`](../../plans/14-brain-architecture.md)
- **Goal:** Both `octopus-cli` and `qqbot-core` share the same agent loop, tool registry, and LLM-provider logic.

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Done |
| 🔄 | In Progress |
| ⬜ | Not Started |
| ⏸️ | Blocked |

## High-level status

| Phase | Goal | Status |
|-------|------|--------|
| 0 | Decisions & design | ✅ |
| 1 | Extract + layer the Brain crate | ✅ |
| 2 | Make `octopus-cli` a consumer | ✅ |
| 3 | Plugin tool ABI + `qqbot-core` integration | ✅ |
| 4 | Configuration / auth unification | ⬜ |

## Decisions log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-06-14 | New crate extracted from `octopus-cli`; `octopus-cli` becomes a consumer. | Keeps existing CLI while making the core reusable. |
| 2026-06-14 | One long-lived Brain per allowed QQ group. | Natural chat-session boundary. |
| 2026-06-14 | All tools available + auto-approve by default. | Daemon has no interactive approval UI. |
| 2026-06-14 | QQ plugins live in `data/qqbot-data/plugins/`, not `~/.kimi/plugins/`. | Separate security posture and deploy flow from CLI plugins. |
| 2026-06-14 | Plugins expose tools via `register_tools` / `call_tool`; legacy `on_message` / `on_command` kept for deterministic commands. | Turns plugins into first-class agent tools. |
| 2026-06-15 | Layer `brain` into `core` / `tools` / `session` / `hooks`; add `ApprovalPolicy`, `ToolSource`, and streaming API. | Keeps `qqbot-core` minimal while letting `octopus-cli` opt into full features later. |
| 2026-06-15 | Add policy traits (`StepPolicy`, `CheckpointPolicy`, `SystemPromptPolicy`, `ToolResultTransformer`, `EventPolicy`) and move CLI-specific checkpoint/D-Mail, dynamic injection, and notification delivery into Brain policy bridges. | Lets `octopus-cli` keep its outer turn loop for steers/wire mapping while Brain owns the reasoning loop. |

## Task backlog

### Phase 1 — Extract + layer the Brain crate

- [x] Create `brain` workspace member.
- [x] Layer modules: `core`, `tools`, `session`, `hooks`.
- [x] Define agent-loop types and logic:
  - [x] `BrainEvent` enum (TextPart, ThinkingPart, ToolCall, ToolResult, ApprovalRequested, ApprovalResolved, TurnBegin, TurnEnd, StepBegin, StepEnd, StepInterrupted, StepRetry, CheckpointCreated, CheckpointReverted, ProviderRefreshing, ProviderRefreshRequested, ProviderRefreshed, Error).
  - [x] `Brain` struct with `run_turn` (streaming), `run_turn_to_completion`, and `run_step`.
  - [x] `ToolRegistry` abstraction + `ToolSource` trait.
  - [x] `ApprovalPolicy` / `ApprovalRuntime` traits with `AutoApprove` default.
  - [x] `MessageStore`, `CompactionPolicy`, `InjectionPolicy`, `HookPolicy` traits with no-op defaults.
  - [x] `StepPolicy`, `CheckpointPolicy`, `SystemPromptPolicy`, `ToolResultTransformer`, `EventPolicy` traits with no-op defaults.
  - [x] LLM-provider resolution via `kosong`.
- [x] `cargo check --workspace` passes.
- [x] `cargo test --workspace` passes.

### Phase 2 — Make `octopus-cli` a consumer

- [x] Delegate each reasoning step from `KimiSoul` to `Brain::run_step`, keeping a thin outer loop for steers and wire event mapping.
- [x] Map `BrainEvent`s to existing UI render paths.
- [x] Implement CLI-specific Brain policy bridges:
  - [x] `CliStepPolicy` — per-step dedup, notification delivery, D-Mail rewinds, tool-rejection stops.
  - [x] `CliCheckpointPolicy` — file-backed checkpoints and `DenwaRenji` synchronization.
  - [x] `CliInjectionPolicy` — dynamic plan-mode / AFK reminders.
  - [x] `CliCompactionPolicy`, `CliProviderFactory`, `CliRetryPolicy`, `CliRecoveryPolicy`.
- [x] Preserve interactive approval through `KimiToolset` (auto-approve runtime inside Brain; approval UI in toolset).
- [x] `cargo test -p octopus-cli` passes with no regressions.
- [x] `octopus-cli --print` still produces answers.

### Phase 3 — Plugin tool ABI and `qqbot-core` integration

- [x] Extend WASM plugin ABI with optional exports:
  - [x] `register_tools` / `execute` via Extism PDK.
- [x] Implement plugin discovery via `brain::tools::plugin::ExtismPluginSource`.
- [x] Dispatch plugin tool calls from the Brain back into the plugin.
- [x] Create one `Brain` per allowed group in `qqbot-core` (`GroupBrainManager`).
- [x] Implement two-phase QQ messaging (processing indicator → final reply).
- [x] Rewrite `summary` plugin as a tool provider (`summary_format_conversation`).
- [x] Provide a QQ-safe default toolset (`qqbot_recent_messages`).
- [x] Remove the simple `LlmClient` summary path.
- [x] `/status` and `/help` commands handled directly in the host.
- [ ] Verify `/summary` works end-to-end against a real group.

### Phase 4 — Configuration and auth unification

- [ ] `qqbot-core` reads the same provider/model config as `octopus-cli`.
- [ ] OAuth token refresh works in daemon context.
- [ ] Document `qqbot init` bootstrapping from `~/.kimi/config.toml`.

## Open blockers

| Blocker | Impact | Owner |
|---------|--------|-------|
| None currently | — | — |

## Notes

- This tracker is separate from the existing `octopus-cli` 1:1 rewrite tracker in `docs/tracking/`.
- The Brain crate is a cross-cutting architectural change, not a direct Python→Rust port.
