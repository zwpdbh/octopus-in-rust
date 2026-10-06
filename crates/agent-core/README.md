# agent-core

An embeddable LLM agent "brain" for the octopus workspace: provider-agnostic
agent loop, tool dispatch, hooks, approval, retry/recovery — designed to be
embedded in other applications (CLI, bots, services) rather than to be one.

Pairs with [`llm-provider`](../llm-provider) (streaming `ChatProvider`
abstraction; dependency direction is one-way: agent-core → llm-provider).

## Architecture at a glance

- `Brain` (`core/brain.rs`) = `BrainConfig` + `Arc<dyn ChatProvider>` +
  `ToolRegistry` + optional custom `Toolset`.
- `BrainConfig` (`core/config.rs`) is a bag of policy trait objects — hook,
  retry, recovery, step, checkpoint, compaction, injection, system-prompt,
  tool-result-transformer, event, approval, provider-factory — all with NoOp
  defaults, assembled via `BrainBuilder`.
- The loop (`run_turn_loop`): user-prompt hook → per step: checkpoint →
  retry/recovery wrapper → `execute_step` (compaction → injection → before_step
  → system prompt → LLM step with tool dispatch → after_step), controlled by
  `StepControl::{Continue, Stop, RewindToCheckpoint}`.
- Events stream out over an mpsc channel; tools are gated by
  `ApprovalToolset` inside `HookAwareToolset`.

## Current status

Works: single-provider streaming steps, tool dispatch (incl. WASM plugins via
extism), approval/hook wrappers, OAuth token management.

Stubs/placeholders: subagents (`core/subagent.rs`), skills (`tools/skill.rs`),
session persistence (in-memory only), compaction (naive).

Known defects: see Part 1 below.

---

# Enhancement Plan

Reference implementation for comparison: kimi-code's `agent-core-v2` (TS) at
`/home/zw/code/examples/kimi-code`, abbreviated `ac2` below. Each item lists
the TS file worth reading **before** implementing.

**Rule for all work below:** `cargo test` green, project AGENTS.md enforced
(enums for state, typed structs at boundaries, thin `mod.rs`), and both real
consumers (`octopus-cli` brain_bridge, `qqbot-core` group_brain) still compile
and behave — the public API must not break silently.

## Part 1 — Correctness fixes (do first, in this order)

**1.1 Bound the retry/recovery loop** — `core/brain.rs:397-475`
Worst defect: `RefreshProvider`/`Retry` reset `attempt = 0` (`:432`, `:468`)
with no global cap → persistent 401/503 loops forever.
Fix: never reset the counter; total-attempt and/or wall-clock budget spanning
retry + recovery; cap `RefreshProvider` to once per distinct error.
Read: `ac2/src/human/llm/requester/retry.ts` (max 10 attempts, 500ms→32s exp
backoff + jitter, honors `Retry-After`, retryability by error kind) and
`recovery.ts` (recovery does NOT reset the budget).

**1.2 Cancellation** — `core/brain.rs:92`
`run_turn` drops the `JoinHandle`; dropping the receiver leaves the task
running, burning tokens. No `CancellationToken` anywhere in either crate.
Fix: `tokio_util::sync::CancellationToken` in `BrainConfig`; check it in
step/retry loops and `select!` against provider calls; return
`TurnHandle { stream, cancel, join }`.
Read: `ac2/src/agent/loop/loop.ts` (signal threaded through every LLM call,
tool exec, gate decision), `ac2/src/human/llm/errors.ts` (aborts normalized so
they never enter the retry path), `ac2/src/agent/interruptionReminder/`
(after abort, tell the model it was interrupted).

**1.3 Hooks single-fire + full tool identity** — `core/brain.rs:593-597` vs `:803-827`
`execute_step` calls `on_pre_tool_use` and discards the result;
`HookAwareToolset` fires it again. Post-tool hooks receive `tool_name: ""` and
`Value::Null` input (`:607`, `:640`, `:645`).
Fix: delete the pre-loop in `execute_step` (HookAwareToolset is the single
gate); thread real tool name + arguments to `ToolResultTransformer` and
post-use hooks.

**1.4 Stream integrity** — `llm-provider/src/provider/kimi.rs:364,370`, `openai_responses.rs:498`
SSE parse/chunk errors are swallowed; a mid-stream failure yields a truncated
but "successful" message that gets persisted.
Fix: `Part::Error` (or stream-terminating error) so truncation is visible;
type the Responses stream with `#[serde(tag = "type")]` instead of
`data.get("delta")` walls (`openai_responses.rs:454-573`) — the AGENTS.md
"typed structs, not Value indexing" rule.

**1.5 OAuth refresh race** — `core/oauth.rs:84-110`
Check-then-refresh without the lock → concurrent double-refresh can
double-spend a refresh token (IdPs may treat this as theft and revoke).
Fix: single-flight/mutex across check+refresh.

**1.6 Smaller items**
- HTTP timeouts on all providers (`reqwest::Client::new()` has none).
- `run_turn_to_completion` clears accumulated text on any ToolCall event
  (`brain.rs:110`) — lossy; fix or document.
- `InjectionPolicy` doc says "not persisted" but `execute_step` persists
  (`brain.rs:511-516`) — align doc or behavior.
- Registry/`SimpleToolset` panics (`RwLock::write().unwrap()`, poisoned locks)
  → return errors instead.

## Part 2 — API hygiene (the "reusable brain" contract)

**2.1 Kill the dead surface** (wire it or delete it):
`EventPolicy` never consulted (`config.rs:88`, `brain.rs:168`);
`max_step_attempts` never read (`config.rs:38`);
`StepContext.turn_id` always `None` (`brain.rs:227`, `:331`);
`RetryableChatProvider` never implemented (`chat_provider.rs:49-51`, providers
have shadowing inherent methods); `ApprovalRequest.display` always empty
(`brain.rs:741`).

**2.2 Structured errors at the event boundary.**
`BrainEvent::Error(String)` forces consumers to re-parse strings
(`classify_kosong_error` in the CLI). Carry `BrainErrorCategory` + message or
the full `BrainError`. Remove stale "kosong" names
(`BrainError::from_kosong_error`, `KosongToolResult`).

**2.3 AGENTS.md state-modeling violations.**
`ThinkingEffort = String` (`chat_provider.rs:19`) → enum with serde impls;
`BrainEvent::ApprovalResolved { approved: bool, reason: Option<String> }`
(`events.rs:78`) → reuse the existing `ApprovalResponse` enum
(`approval.rs:24`) — `true + Some(reason)` must be unrepresentable.
Audit the rest: `stream: bool` fields, `Message.partial: Option<bool>`,
`GroupRuntimeStatus.brain_ready: bool`, `ToolReturnValue.is_error: bool`.

**2.4 Real streaming + backpressure.**
Pass `on_message_part` through `execute_step` (`brain.rs:539-545`) so
`TextPart` is token-level, not once-per-step; replace unbounded mpsc channels
(`brain.rs:85,141,444`) with bounded ones + documented capacity.
Read: `packages/transcript/src/granularity/` (subscription grades
off/turn/block/delta — consumers pick coarseness).

**2.5 Turn serialization guard.**
`run_turn(&mut self)` mutates nothing; two concurrent calls on clones
interleave histories in the shared `message_store` mutex. Take a real turn
lock or make the API shape honest.

## Part 3 — Decoupling for reuse

- Move `control.rs` (qqbot's control-socket protocol, `group_id: i64`) to the
  qqbot crates.
- Feature-gate extism (`Cargo.toml:27`) — plain-chat consumers shouldn't pay
  the WASM runtime.
- Extract Kimi-specific defaults from the generic crate: OAuth client_id
  (`oauth.rs:27-32`), `https://api.kimi.com/coding/v1` (`provider.rs:261`),
  `~/.kimi` paths (`provider.rs:146-150`, `tools/plugin/discovery.rs:376-378`),
  `$HOSTNAME`/device_id reads (`provider.rs:284-289`). Offer a `kimi-presets`
  feature or leave them to app crates.

## Part 4 — Capability roadmap (steal from agent-core-v2)

Ordered by value-per-effort. Read the listed TS files first.

**4.1 Compaction & context management** (currently a naive stub)
Read: `ac2/src/agent/fullCompaction/strategy.ts` (trigger at 0.85 fill,
reserve 50k tokens, recency caps, max 3 overflow attempts),
`fullCompactionService.ts` + `compaction-instruction.md` (compaction is itself
an LLM call), `ac2/src/agent/tokenCounting/`, `ac2/src/agent/toolResultTruncation/`.

**4.2 Session persistence** (currently in-memory only)
Read: `ac2/src/wire/record.ts`, `ac2/src/human/store/backend/node.ts`
(append-only JSONL, atomic rename), `ac2/src/wire/migration/` (versioned
migrations on read + torn-write repair), `ac2/src/human/eventStore/`
(event-sourced store; replay = re-fold the journal).
Also: one snapshot test pinning the event/wire format
(cf. `ac2/docs/wire-manifest.d.ts`).

**4.3 Repeat-loop breaker**
Read: `ac2/src/agent/toolDedupe/toolDedupeService.ts` (identical repeated
tool calls → stop the turn). Cheap; compounds with 1.1.

**4.4 Mid-turn steering**
Read: `ac2/src/agent/loop/promptChannel.ts` + `human/agent/origin.ts`
(`mergeSteerMessages`): queue messages arriving mid-turn and merge; a queue +
one merge function, big UX difference.

**4.5 Skills** (replaces `tools/skill.rs` stub)
Read: `ac2/src/features/skill/catalog/` (frontmatter-only in system prompt) +
`tools/skillTool.ts` (body loaded on demand). A file scanner + one tool + a
prompt snippet.

**4.6 Subagents** (replaces `core/subagent.rs` stub)
Read: `ac2/src/agent/tools/agent/agentTool.ts` (child scope, own permission
mode, filtered tool set — no recursive delegation unless allowed) +
`subagent-task.ts` (foreground vs background). Child's final output = tool
result.

**4.7 Background tasks**
Read: `ac2/src/agent/task/taskService.ts` + `taskOps.ts` (registry, lifecycle,
wall-clock, persistence, XML completion notice into context) +
`ac2/src/agent/tools/task/` (4 model-facing tools; `task-wait` is bounded).
Subagents and background shells share one registry.

**4.8 Hooks engine** (realize the `HookPolicy` trait)
Read: `ac2/src/features/externalHooks/` — lifecycle points spawning user shell
commands with JSON on stdin; output can mutate the turn.

**4.9 Permissions layers** (extends approval-only gate)
Read: `ac2/src/agent/permissionRules/` (allow/ask/deny globs) →
`permissionPolicy/policies/` (composable; e.g. force-ask on dangerous shell
constructs) → `permissionGate/permissionGateService.ts` (order: rules →
policies → mode → user) → `features/plan/` (plan mode is just a mode).

**4.10 MCP client**
Read: `ac2/src/mcpCore/` (per-transport clients + `connection-manager.ts`) +
`ac2/src/agent/mcp/mcpService.ts` (MCP tools merged into the same registry so
permissions/truncation apply uniformly). Or adopt the `rmcp` crate.

## Explicitly NOT porting

- DI/scope machinery (`ac2/src/_base/di/`) — overkill at this size; it serves
  5+ frontends. Steal only the per-agent child-context idea (it's what gives
  subagents isolation in 4.6).
- Anything TUI/CLI-specific.

## Validation per item

1. Failing test first where behavior changes — extend the Echo/ScriptedEcho
   DSL providers (`llm-provider/src/provider/echo/`) rather than adding mocks.
2. `cargo test` + clippy + the AGENTS.md checklist.
3. Re-run both real consumers.
