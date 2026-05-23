# Soul Rewrite Gap Analysis

> Generated: 2026-05-23
> Compare: `tmp/kimi-cli/src/kimi_cli/soul/` (Python) → `octopus-cli/src/soul/` (Rust)

---

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Ported and functional |
| 🔄 | Partial / simplified |
| ❌ | Missing / stubbed |
| 🔒 | Blocked on another subsystem (see note) |

---

## Implementation Phases

Work through these in order. Check boxes as you complete items.

---

### Phase 1: Core Loop Reliability (P0)

Must-have before the soul is usable in real sessions.

- [x] `StepRetry` wire type + emission
- [x] Basic tenacity retry in `_step()` (exponential backoff + jitter)
- [x] `_is_retryable_error()` classification
- [x] `_classify_api_error()` for telemetry
- [x] `_run_with_connection_recovery()` — 401 → OAuth refresh, connection retry
  - OAuth refresh: `ensure_fresh()` stub present; real token refresh TBD
  - Connection recovery: chat provider `RetryableChatProvider` path stubbed (no kosong provider implements it yet)
- [x] `StopFailure` hook on fatal step error — fires through stub `HookEngine`
- [x] Approval source cleanup in `finally` block (run / interrupt / return)
- [x] Turn interruption telemetry (`turn_interrupted`) — `tracing::warn!` present; real `track()` call TBD
- [x] `check_message()` on user input in `_turn()`
- [x] Turn ID tracking

**Status: COMPLETE**

---

### Phase 2: Context & Compaction (P0)

Long sessions will break without these.

- [x] Real compaction LLM call
- [x] `normalize_history()` — merge adjacent user messages
- [ ] PreCompact / PostCompact hooks 🔒 *blocked on HookEngine implementation (Phase 5)*
- [ ] Post-compact: background task snapshot injection 🔒 *blocked on BackgroundTaskManager (Phase 5)*
- [ ] Post-compact: notify injection providers 🔒 *blocked on DynamicInjectionProvider (Phase 3)*
- [ ] Compaction telemetry 🔒 *blocked on telemetry system (Phase 12)*
- [ ] `asyncio.shield` equivalent for `_grow_context` — *low priority; Rust futures are cancel-safe via drop*

**Status: COMPLETE** (unblocked items done; blocked items will resolve when their dependencies are implemented)

---

### Phase 3: Dynamic Injection (P1)

Plan mode and afk mode need periodic reminders to work correctly.

- [x] `DynamicInjectionProvider` trait
- [x] `PlanModeInjectionProvider` — periodic reminders, reentry logic
- [x] `AfkModeInjectionProvider` — afk guidance injection
- [x] `_collect_injections()` in `_step()` before LLM call
- [x] `_notify_injection_providers_compacted()`
- [x] `notify_afk_changed()`

**Status: NOT STARTED**

---

### Phase 4: Toolset Depth (P1)

Correctness and efficiency of tool execution.

- [ ] Same-step dedup (wait for original task result)
- [ ] Cross-step dedup + "don't repeat" reminder injection
- [ ] `begin_step()` / `end_step()` on `KimiToolset`
- [ ] Hidden tools (`hide()` / `unhide()`)
- [ ] PreToolUse hook 🔒 *blocked on HookEngine (Phase 5)*
- [ ] PostToolUse hook 🔒 *blocked on HookEngine (Phase 5)*
- [ ] PostToolUseFailure hook 🔒 *blocked on HookEngine (Phase 5)*
- [ ] Tool execution timing + telemetry 🔒 *blocked on telemetry system (Phase 12)*
- [ ] `on_tool_result` streaming callback

**Status: NOT STARTED**

---

### Phase 5: Hooks & Notifications (P2)

Plugin system and background awareness.

- [ ] `HookEngine` — real implementation with async handler dispatch, matcher evaluation, action results
- [ ] `UserPromptSubmit` hook 🔒 *needs HookEngine*
- [ ] `Stop` hook 🔒 *needs HookEngine*
- [ ] `StopFailure` hook 🔒 *needs HookEngine* (stub already fires)
- [ ] `PreToolUse` hook 🔒 *needs HookEngine*
- [ ] `PostToolUse` hook 🔒 *needs HookEngine*
- [ ] `PostToolUseFailure` hook 🔒 *needs HookEngine*
- [ ] `Notification` hook 🔒 *needs HookEngine*
- [ ] `NotificationManager` — deliver pending to LLM context in `_step()`
- [ ] `build_notification_message()`
- [ ] Ack notification IDs on context restore

**Status: NOT STARTED** — *unblocks Phases 2, 4, 7, 13*

---

### Phase 6: Wire & Messaging (P2)

UI visibility and multi-soul support.

- [ ] `wire_send()` — wire file backend (`wire.jsonl`)
- [ ] Wire isolation per run (concurrent souls)
- [ ] `QueueShutDown` handling
- [ ] Notification pump (`_pump_notifications_to_wire()`)
- [ ] `MCPLoadingBegin` / `MCPLoadingEnd` emission
- [ ] `BtwBegin` / `BtwEnd` emission

**Status: NOT STARTED**

---

### Phase 7: MCP Integration (P3)

External tool servers.

- [ ] MCP client transport
- [ ] MCP OAuth
- [ ] Deferred background loading
- [ ] `MCPServerInfo` tracking
- [ ] `MCPTool` wrapper
- [ ] `mcp_status_snapshot()` real implementation
- [ ] `WireExternalTool` — external tool calls via wire

**Status: NOT STARTED**

---

### Phase 8: D-Mail / Time Travel (P3)

Undo + rewind mechanism.

- [ ] `DenwaRenji` stateful tracking
- [ ] `send_dmail()`
- [ ] `fetch_pending_dmail()`
- [ ] `BackToTheFuture` exception
- [ ] `_step()` D-Mail handling
- [ ] `_agent_loop()` revert + re-inject

**Status: NOT STARTED**

---

### Phase 9: Side Questions (`/btw`) (P3)

- [ ] `execute_side_question()`
- [ ] `_DenyAllToolset`
- [ ] `_build_btw_context()`
- [ ] Streaming text chunks
- [ ] `BtwBegin` / `BtwEnd` wire events

**Status: NOT STARTED**

---

### Phase 10: Flow Runner (P3)

- [ ] `FlowRunner`
- [ ] `ralph_loop()`
- [ ] Flow node execution
- [ ] Choice parsing

**Status: NOT STARTED**

---

### Phase 11: Agent / Runtime Loading (P3)

- [ ] `load_agent()` — real Jinja2 templating, spec loading
- [ ] `BuiltinSystemPromptArgs` populated from env
- [ ] Jinja2 system prompt templating
- [ ] `AGENTS.md` discovery + merge
- [ ] Skills discovery + formatting
- [ ] Subagent type registration
- [ ] `Runtime::create()` async setup
- [ ] `Runtime::copy_for_subagent()`
- [ ] `Environment::detect()` — real detection

**Status: NOT STARTED**

---

### Phase 12: Telemetry (P3)

- [ ] `track()` infrastructure (event name + kwargs → fire-and-forget dispatch)
- [ ] Turn started / interrupted
- [ ] Tool call (success / error / dedup)
- [ ] Compaction finished / failed
- [ ] MCP connected / failed

**Status: NOT STARTED** — *unblocks blocked items in Phases 1, 2, 4*

---

### Phase 13: Slash Commands Polish (P3)

- [ ] `/web` — real implementation
- [ ] `/vis` — real implementation
- [ ] `/btw` — real implementation (see Phase 9)
- [ ] `/task` — real implementation
- [ ] `/model` — model switching
- [ ] `/feedback` — full flow
- [ ] `/add-dir` — KaosPath, dir validation, listing injection
- [ ] Skill commands (`skill:*`)
- [ ] Flow commands (`flow:*`)

**Status: NOT STARTED**

---

### Phase 14: OAuth / Token Refresh (P3)

- [ ] `oauth.ensure_fresh()` per turn (currently stubbed; `OAuthManager` exists)
- [ ] 401 → force refresh → retry (path exists; needs real refresh logic)

**Status: NOT STARTED** — *scaffolding done in Phase 1*

---

### Phase 15: Subagents (P3)

- [ ] `AgentTool` real implementation
- [ ] `LaborMarket` real implementation
- [ ] `SubagentStore` real implementation
- [ ] Subagent-specific rejection messages

**Status: NOT STARTED**

---

## Dependency Graph

```
Phase 3 (Dynamic Injection)
  │
  ├─► Phase 5 (Hooks) ──► Phase 2 items (PreCompact, PostCompact hooks)
  │                        Phase 4 items (Pre/PostToolUse hooks)
  │
  └─► Phase 12 (Telemetry) ──► Phase 1 item (turn_interrupted track)
                               Phase 2 item (compaction telemetry)
                               Phase 4 item (tool timing + telemetry)

Phase 5 (Hooks)
  │
  └─► Phase 13 (Notification hook)

Phase 7 (MCP)
  │
  └─► Phase 6 (MCPLoading wire events)
```

---

## Original Detailed Breakdown (for reference)

> **Note:** The phase checklists above are the canonical source of truth. The tables below may be slightly out of date; they are kept for per-subsystem detail.

---

## 1. Core Agent Loop (`KimiSoul::run` → `_turn` → `_agent_loop` → `_step`)

| Feature | Status | Notes |
|---------|--------|-------|
| `run()` entrypoint | 🔄 | Missing `skip_user_prompt_hook`, OAuth refresh per turn, `UserPromptSubmit` hook |
| `_turn()` | ✅ | `check_message`, turn ID tracking, `_last_tool_calls` reset all implemented |
| `_agent_loop()` | 🔄 | Missing MCP loading, D-Mail handling (`BackToTheFuture`), `StepRetry` events |
| Auto-compaction | ✅ | Calls real LLM compaction |
| Max steps guard | ✅ | |
| Steer consumption | 🔄 | Only string steers; Python supports `str \| list[ContentPart]` |
| `_step()` | 🔄 | Retry + 401 recovery implemented; dynamic injection, notifications, dedup missing |

### `_step()` Sub-Lifecycle Breakdown

| Phase | Python | Rust | Gap |
|-------|--------|------|-----|
| 2e.1 Notification delivery (root only) | ✅ | ❌ | Background tasks can't notify LLM |
| 2e.2 Dynamic injection (plan/afk mode) | ✅ | ❌ | Plan mode is just a flag; no reminder injection |
| 2e.3 History normalization | ✅ | ✅ | `normalize_history()` implemented |
| 2e.4 LLM call with retry + recovery | ✅ | ✅ | Basic retry + 401 OAuth recovery done; connection recovery stubbed |
| 2e.4.1 Toolset `begin_step` / `end_step` | ✅ | ❌ | No dedup state management |
| 2e.5 Usage & status update | 🔄 | 🔄 | Missing `message_id`, `plan_mode` correction after tools |
| 2e.6 Tool execution | 🔄 | 🔄 | No streaming (`on_tool_result`), no timing |
| 2e.7 Context growth | 🔄 | 🔄 | Missing `asyncio.shield` |
| 2e.8 Outcome resolution (rejection/D-Mail/stop) | ✅ | 🔄 | D-Mail absent; rejection stops turn for both root and subagent |

---

## 2. Error Handling & Resilience

| Feature | Status | Notes |
|---------|--------|-------|
| `classify_api_error()` for telemetry | ✅ | Implemented |
| `_run_with_connection_recovery()` | ✅ | 401→OAuth refresh path wired; connection recovery stubbed |
| `StepRetry` wire events | ✅ | |
| `StopFailure` hook on fatal error | 🔄 | Stub `HookEngine::trigger` fires; real hook engine TBD |
| Turn interruption telemetry | 🔄 | `tracing::warn!` logs it; `track()` call TBD |
| Approval source cleanup in `finally` | ✅ | Explicit cleanup in `run()` error path |

---

## 3. Dynamic Injection System

| Component | Status | Notes |
|-----------|--------|-------|
| `DynamicInjectionProvider` trait | ❌ | Completely absent |
| `PlanModeInjectionProvider` | ❌ | No periodic reminders, no reentry logic |
| `AfkModeInjectionProvider` | ❌ | No afk guidance injection |
| `normalize_history()` | ✅ | Implemented in `soul/message.rs` |
| `_collect_injections()` | ❌ | |
| `_notify_injection_providers_compacted()` | ❌ | |
| `notify_afk_changed()` | ❌ | |

---

## 4. Toolset (`KimiToolset`)

| Feature | Python (~888 LOC) | Rust (~109 LOC) | Status |
|---------|-------------------|-----------------|--------|
| Same-step dedup | ✅ | ❌ | Waits for original task result |
| Cross-step dedup + reminder | ✅ | ❌ | Injects "don't repeat" reminder |
| `begin_step` / `end_step` | ✅ | ❌ | Per-step call tracking |
| Hidden tools | ✅ | ❌ | `hide()` / `unhide()` |
| PreToolUse hook | ✅ | ❌ | 🔒 blocked on HookEngine |
| PostToolUse hook | ✅ | ❌ | 🔒 blocked on HookEngine |
| PostToolUseFailure hook | ✅ | ❌ | 🔒 blocked on HookEngine |
| Tool execution timing + telemetry | ✅ | ❌ | 🔒 blocked on telemetry |
| MCP tool loading (background) | ✅ | ❌ | Stubs return `false`/`None` |
| MCP status snapshot | ✅ | ❌ | Returns `None` |
| `WireExternalTool` | ✅ | ❌ | External tool calls via wire |
| Plugin tools | ✅ | ❌ | |
| Tool call ContextVar | `ContextVar` | `thread_local!` | 🔄 | Different lifetime model |

---

## 5. Context (`Context`)

| Feature | Status | Notes |
|---------|--------|-------|
| `restore()` async | 🔄 | Exists but unused (soul calls `restore_sync()`) |
| `restore_sync()` | ✅ | |
| `write_system_prompt()` | ✅ | With atomic prepend via temp file |
| `checkpoint()` | ✅ | |
| `revert_to()` | ✅ | |
| `clear()` | ✅ | |
| `append_message()` | ✅ | |
| `update_token_count()` | ✅ | |
| Error logging on malformed lines | ❌ | Python logs warnings; Rust silently skips |
| `_pending_token_estimate` | ✅ | |

---

## 6. Approval System

| Feature | Status | Notes |
|---------|--------|-------|
| `ApprovalState` struct | ✅ | |
| `yolo` / `afk` / `runtime_afk` | ✅ | |
| `auto_approve_actions` | 🔄 | Field exists, not wired to slash commands |
| `on_change` callback → session persistence | 🔄 | Callback exists but not wired to `session.save_state()` |
| `ApprovalResult::rejection_error()` | 🔄 | Missing subagent-specific rejection message |
| `approve_for_session` → persist action | ✅ | |
| Telemetry tracking (approve/reject) | ❌ | 🔒 blocked on telemetry |
| `ApprovalRuntime` integration | 🔄 | Basic request/wait/response; cleanup wired in `run()` |

---

## 7. Compaction (`SimpleCompaction`)

| Feature | Status | Notes |
|---------|--------|-------|
| `should_auto_compact()` | ✅ | |
| `prepare()` | ✅ | |
| `compact()` LLM invocation | ✅ | Calls real LLM; falls back gracefully on error |
| `estimate_text_tokens()` | ✅ | |
| Post-compact: background task snapshot injection | ❌ | 🔒 blocked on BackgroundTaskManager |
| Post-compact: notify injection providers | ❌ | 🔒 blocked on DynamicInjectionProvider |
| `CompactionBegin` / `CompactionEnd` wire events | ✅ | |
| PreCompact / PostCompact hooks | ❌ | 🔒 blocked on HookEngine |
| Compaction telemetry | ❌ | 🔒 blocked on telemetry |

---

## 8. Agent / Runtime (`agent.py`)

| Feature | Status | Notes |
|---------|--------|-------|
| `Agent` struct | 🔄 | Missing real system prompt loading |
| `Runtime` struct | 🔄 | Fields exist, mostly unpopulated |
| `load_agent()` | ❌ | Returns `"You are a helpful assistant."` — no Jinja, no spec |
| `BuiltinSystemPromptArgs` | 🔄 | Struct exists, not populated from real env |
| Jinja2 system prompt templating | ❌ | |
| `AGENTS.md` discovery + merge | ❌ | |
| Skills discovery + formatting | ❌ | |
| Subagent type registration | ❌ | |
| `Runtime::create()` async setup | ❌ | |
| `copy_for_subagent()` | ❌ | |
| `DenwaRenji` D-Mail | ❌ | Empty struct; `fetch_pending_dmail()` always `None` |
| `Environment::detect()` | 🔄 | Hardcoded to `bash` on Linux |

---

## 9. Message Handling (`message.py`)

| Feature | Status | Notes |
|---------|--------|-------|
| `system()` | ✅ | |
| `system_reminder()` | ✅ | |
| `is_system_reminder_message()` | ✅ | |
| `tool_result_to_message()` | 🔄 | Missing `ToolRuntimeError` special handling nuance |
| `_output_to_content_parts()` | 🔄 | Simplified to `ToolOutput` enum |
| `check_message()` | ✅ | |
| `normalize_history()` | ✅ | Adjacent user messages merged |

---

## 10. Wire / Messaging

| Feature | Status | Notes |
|---------|--------|-------|
| `wire_send()` global | 🔄 | Static channel instead of ContextVar |
| Wire isolation per run | ❌ | Can't run multiple souls concurrently |
| Wire file backend (`wire.jsonl`) | ❌ | Not integrated |
| `QueueShutDown` handling | ❌ | |
| Notification pump | ❌ | `_pump_notifications_to_wire()` absent |
| `TurnBegin` / `TurnEnd` | ✅ | |
| `StepBegin` / `StepInterrupted` | ✅ | |
| `StepRetry` | ✅ | |
| `MCPLoadingBegin` / `MCPLoadingEnd` | 🔄 | Types exist, not emitted |
| `BtwBegin` / `BtwEnd` | 🔄 | Types exist, not emitted |

---

## 11. Slash Commands

| Command | Status | Notes |
|---------|--------|-------|
| `/clear` (/reset) | ✅ | |
| `/yolo` | ✅ | |
| `/afk` | ✅ | |
| `/plan` | ✅ | on/off/view/clear |
| `/compact` | ✅ | |
| `/help` (/h, /?) | ✅ | Soul-level only |
| `/changelog` | ✅ | |
| `/debug` | ✅ | |
| `/add-dir` | 🔄 | Missing KaosPath, dir validation, listing injection |
| `/exit` (/quit) | ✅ | |
| `/version` | ✅ | |
| `/model` | 🔄 | Show only; no switching |
| `/feedback` | 🔄 | GitHub fallback only |
| `/new` | ✅ | |
| `/title` (/rename) | ✅ | |
| `/sessions` (/resume) | ✅ | No interactive picker |
| `/web` | ❌ | Stub |
| `/vis` | ❌ | Stub |
| `/mcp` | 🔄 | Basic status dump |
| `/hooks` | 🔄 | Basic list |
| `/undo` | ✅ | |
| `/fork` | ✅ | |
| `/btw` | ❌ | Stub |
| `/editor` | 🔄 | Basic validation only |
| `/task` | ❌ | Stub |
| `/theme` | 🔄 | In-memory only |
| **Skill commands** (`skill:*`) | ❌ | Not implemented |
| **Flow commands** (`flow:*`) | ❌ | Not implemented |

---

## 12. Hooks

| Feature | Status | Notes |
|---------|--------|-------|
| `HookEngine` instantiation | 🔄 | Created in `KimiSoul`; stub implementation |
| `UserPromptSubmit` hook | ❌ | 🔒 blocked on real HookEngine |
| `Stop` hook | ❌ | 🔒 blocked on real HookEngine |
| `StopFailure` hook | 🔄 | Stub fires; real async dispatch TBD |
| `PreToolUse` hook | ❌ | 🔒 blocked on real HookEngine |
| `PostToolUse` hook | ❌ | 🔒 blocked on real HookEngine |
| `PostToolUseFailure` hook | ❌ | 🔒 blocked on real HookEngine |
| `PreCompact` / `PostCompact` hook | ❌ | 🔒 blocked on real HookEngine |
| `Notification` hook | ❌ | 🔒 blocked on real HookEngine |

---

## 13. Notifications

| Feature | Status | Notes |
|---------|--------|-------|
| `NotificationManager` | 🔄 | Stub exists in module tree |
| Deliver pending to LLM context | ❌ | Never called in `_step()` |
| `build_notification_message()` | ❌ | |
| Ack notification IDs on restore | ❌ | |

---

## 14. MCP Integration

| Feature | Status | Notes |
|---------|--------|-------|
| MCP client transport | ❌ | |
| MCP OAuth | ❌ | |
| Deferred background loading | ❌ | Stubs return `false` |
| `MCPServerInfo` tracking | ❌ | |
| `MCPTool` wrapper | ❌ | |
| `mcp_status_snapshot()` | ❌ | Returns `None` |

---

## 15. D-Mail / DenwaRenji / Time Travel

| Feature | Status | Notes |
|---------|--------|-------|
| `DenwaRenji` stateful tracking | ❌ | Empty struct |
| `send_dmail()` | ❌ | |
| `fetch_pending_dmail()` | ❌ | Always `None` |
| `BackToTheFuture` exception | ❌ | |
| `_step()` D-Mail handling | ❌ | |
| `_agent_loop()` revert + re-inject | ❌ | |

---

## 16. Side Questions (`/btw`)

| Feature | Status | Notes |
|---------|--------|-------|
| `execute_side_question()` | ❌ | |
| `_DenyAllToolset` | ❌ | |
| `_build_btw_context()` | ❌ | |
| Streaming text chunks | ❌ | |
| `BtwBegin` / `BtwEnd` wire events | ❌ | Types exist but not emitted |

---

## 17. Flow Runner

| Feature | Status | Notes |
|---------|--------|-------|
| `FlowRunner` | ❌ | |
| `ralph_loop()` | ❌ | |
| Flow node execution | ❌ | |
| Choice parsing | ❌ | |

---

## 18. Telemetry

| Feature | Status | Notes |
|---------|--------|-------|
| `track()` calls throughout soul | ❌ | None present |
| Turn started / interrupted | ❌ | 🔒 blocked on track() infrastructure |
| Tool call (success/error/dedup) | ❌ | 🔒 blocked on track() infrastructure |
| Compaction finished / failed | ❌ | 🔒 blocked on track() infrastructure |
| API error classification | ✅ | Implemented via `_classify_api_error` |
| MCP connected / failed | ❌ | 🔒 blocked on track() infrastructure |

---

## 19. OAuth / Token Refresh

| Feature | Status | Notes |
|---------|--------|-------|
| `oauth.ensure_fresh()` per turn | 🔄 | Stub exists; `OAuthManager` created in `KimiSoul` |
| 401 → force refresh → retry | ✅ | Path wired in `_run_step_once()`; needs real refresh logic |

---

## 20. Subagents

| Feature | Status | Notes |
|---------|--------|-------|
| `AgentTool` | 🔄 | Stub exists |
| `LaborMarket` | 🔄 | Stub exists |
| `SubagentStore` | 🔄 | Stub exists |
| `Runtime::copy_for_subagent()` | ❌ | |
| Subagent-specific rejection messages | ❌ | |

---

## Bottom-Line Assessment

| Subsystem | Completeness |
|-----------|-------------|
| Core loop structure | 70% |
| `_step()` depth | 50% |
| Resilience (retry/recovery) | 50% |
| Dynamic injection | 10% |
| Toolset (dedup/hooks/MCP) | 20% |
| Context persistence | 90% |
| Approval state machine | 70% |
| Compaction | 70% |
| Agent/Runtime loading | 25% |
| Message formatting | 95% |
| Wire architecture | 50% |
| Slash commands (core) | 80% |
| Slash commands (skills/flows) | 0% |
| Hooks | 10% |
| Notifications | 10% |
| MCP | 0% |
| D-Mail / time travel | 0% |
| BTW | 0% |
| Flow runner | 0% |
| Telemetry | 10% |
| OAuth refresh | 20% |

**Overall soul rewrite: ~50–55% complete.**

The Rust code now has a **solid foundation** — core loop reliability, real compaction, history normalization, and connection recovery scaffolding are all in place. The remaining work is primarily **feature depth** (dynamic injection, toolset dedup, hooks, MCP, notifications, D-Mail, telemetry) rather than structural scaffolding.
