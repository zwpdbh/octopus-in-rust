# Octopus-CLI ↔ Kimi-CLI Mirror Rewrite Comparison

> File-by-file, folder-by-folder comparison between Python `kimi-cli` and Rust `octopus-cli`.
> 
> **Legend:** ✅ Complete | 🔄 Partial / Stub | ❌ Missing | **Bold** = critical path

---

## Summary

| Metric | Python (kimi-cli) | Rust (octopus-cli) | Ratio |
|--------|-------------------|--------------------|-------|
| Total LOC | ~47,200 | ~12,700 | 27% |
| Files | ~120 `.py` | ~65 `.rs` | 54% |
| Modules | 28 folders | 22 folders | 79% |

---

## Top-Level Files

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `lib.rs` | 31 | 🔄 | Package init only |
| `__main__.py` | — | `main.rs` | 410 | 🔄 | CLI entry; OAuth subcommands stubbed |
| `app.py` | 807 | `app.rs` | 233 | 🔄 | Runtime builder; background tasks stubbed |
| `config.py` | 429 | `config.rs` | 433 | 🔄 | Models done; no live config reload |
| `constant.py` | — | `constant.rs` | — | ✅ | Constants ported |
| `exception.py` | — | `exception.rs` | 223 | 🔄 | Core errors done; `BackToTheFuture` added |
| `llm.py` | 333 | `llm.rs` | 385 | 🔄 | Streaming not implemented |
| `metadata.py` | — | `metadata.rs` | 106 | ✅ | Build metadata |
| `session.py` | 319 | `session.rs` | 290 | 🔄 | Fork/undo missing |
| `session_state.py` | — | `session_state.rs` | 181 | 🔄 | Approval state synced |
| `session_fork.py` | 325 | — | — | ❌ | Session fork/clone not implemented |
| `share.py` | — | `share.rs` | 28 | ✅ | Share dir helpers |
| `agentspec.py` | — | `agents/mod.rs` | 20 | 🔄 | Agent YAML loading stubbed |
| `mcp_oauth.py` | — | — | — | ❌ | MCP OAuth flow missing |

---

## `acp/` — ACP/MCP Server

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | — | ❌ | Stub only |
| `server.py` | 468 | — | — | ❌ | ACP server not started |
| `session.py` | 499 | — | — | ❌ | ACP session management missing |
| `tools.py` | — | — | — | ❌ | ACP tool exposure missing |
| `types.py` | — | — | — | ❌ | ACP type definitions missing |
| `convert.py` | — | — | — | ❌ | ACP↔MCP conversion missing |
| `kaos.py` | 291 | — | — | ❌ | Kaos integration missing |
| `mcp.py` | — | — | — | ❌ | MCP server-side missing |
| `version.py` | — | — | — | ❌ | Version endpoint missing |

---

## `approval_runtime/` — Approval System

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 123 | 🔄 | Stubbed `wait_for_response()` |
| `models.py` | — | — | — | ✅ | Models inlined into `mod.rs` |
| `runtime.py` | — | — | — | ✅ | Runtime inlined into `mod.rs` |

**Gaps:** Real approval flow UI (wire request/response cycle) not implemented.

---

## `auth/` — Authentication

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 23 | 🔄 | OAuth manager stubbed |
| `oauth.py` | 1092 | — | — | ❌ | Full OAuth flow missing |
| `platforms.py` | 374 | — | — | ❌ | Platform-specific auth missing |

---

## `background/` — Background Tasks

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 45 | 🔄 | Skeleton only |
| `manager.py` | 725 | — | — | ❌ | Task lifecycle (create/run/kill/wait) |
| `models.py` | 105 | — | — | ❌ | TaskSpec/TaskRuntime/TaskView |
| `store.py` | 150 | — | — | ❌ | File/SQLite persistence |
| `summary.py` | 66 | — | — | ❌ | `build_active_task_snapshot()` |
| `worker.py` | 200 | — | — | ❌ | Background worker process |
| `agent_runner.py` | 150 | — | — | ❌ | Agent background runner |
| `ids.py` | 50 | — | — | ❌ | Task ID generation |

---

## `cli/` — CLI Subcommands

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | 1081 | `mod.rs` | 235 | 🔄 | Main CLI dispatcher; many subcommands stubbed |
| `__main__.py` | — | — | — | ✅ | Entry point in `main.rs` |
| `_lazy_group.py` | — | — | — | ❌ | Lazy CLI group loading |
| `export.py` | 322 | `export.rs` | 123 | 🔄 | Export format limited |
| `info.py` | — | `info.rs` | 29 | ✅ | Basic info command |
| `mcp.py` | 353 | — | — | ❌ | MCP CLI commands missing |
| `plugin.py` | 347 | — | — | ❌ | Plugin CLI commands missing |
| `toad.py` | — | — | — | ❌ | TUI mode missing |
| `vis.py` | — | — | — | ❌ | Visualizer CLI missing |
| `web.py` | — | — | — | ❌ | Web server CLI missing |

---

## `hooks/` — Hook System

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 287 | ✅ | HookEngine fully implemented |
| `engine.py` | 371 | — | — | ✅ | Inlined into `mod.rs` |
| `events.py` | — | `events.rs` | 186 | ✅ | All event builders |
| `runner.py` | — | `runner.rs` | 168 | ✅ | Shell runner + `HookAction` enum |
| `config.py` | — | — | — | ✅ | Inlined into top-level `config.rs` |

---

## `mcp/` — MCP Integration

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 115 | 🔄 | Config models only; no real client |

**Gaps:** MCP client transport, OAuth, real tool loading, `mcp_status_snapshot()`.

---

## `notifications/` — Notification System

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 115 | ✅ | Models + enums |
| `models.py` | — | — | — | ✅ | Inlined into `mod.rs` |
| `manager.py` | 200 | `manager.rs` | 176 | ✅ | Claim/ack/deliver/recover |
| `store.py` | 150 | `store.rs` | 138 | ✅ | File-based JSON store |
| `llm.py` | 50 | `llm.rs` | 64 | ✅ | `build_notification_message()` |
| `wire.py` | 50 | `wire.rs` | — | ✅ | Wire conversion helpers |
| `notifier.py` | 100 | — | — | ❌ | Push notifier missing |

---

## `plugin/` — Plugin System

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | — | ❌ | Empty stub |
| `manager.py` | 200 | — | — | ❌ | Plugin discovery/loading |
| `tool.py` | 100 | — | — | ❌ | Plugin tool exposure |

---

## `prompts/` — System Prompts

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | — | 🔄 | Stub only |

---

## `skills/` — Skills System

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | 727 | `mod.rs` | 30 | ❌ | Skill discovery/loading not started |
| `flow/d2.py` | 482 | — | — | ❌ | D2 diagram flow rendering |
| `flow/mermaid.py` | 266 | — | — | ❌ | Mermaid flow rendering |
| `kimi-cli-help/` | 200 | — | — | ❌ | CLI help skill |
| `skill-creator/` | — | — | — | ❌ | Skill creator skill |

---

## `soul/` — Agent Core

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | 304 | — | — | ✅ | Module exports in `mod.rs` |
| **kimisoul.py** | **1710** | **mod.rs** | **1165** | **🔄** | **Core loop; streaming, btw, subagent gaps** |
| `agent.py` | 519 | `agent.rs` | 202 | 🔄 | `load_agent()` stubbed; no Jinja2 |
| `approval.py` | 267 | `approval.rs` | 243 | 🔄 | Persistence wired; runtime approval flow stubbed |
| `btw.py` | — | — | — | ❌ | Side question execution missing |
| `compaction.py` | — | `compaction.rs` | 169 | ✅ | Real LLM compaction |
| `context.py` | 339 | `context.rs` | 385 | 🔄 | Malformed line logging missing |
| `denwarenji.py` | 39 | — | — | ✅ | Inlined into `agent.rs` |
| `dynamic_injection.py` | — | `dynamic_injection.rs` | 48 | ✅ | Trait + collection |
| `dynamic_injections/__init__.py` | — | `mod.rs` | — | ✅ | Module init |
| `dynamic_injections/afk_mode.py` | — | `afk_mode.rs` | 74 | ✅ | Afk mode injection |
| `dynamic_injections/plan_mode.py` | — | `plan_mode.rs` | 256 | ✅ | Plan mode injection |
| `message.py` | — | `message.rs` | 129 | 🔄 | `tool_result_to_message()` simplified |
| `slash.py` | 341 | `slash.rs` | 1202 | 🔄 | Many commands stubbed |
| `toolset.py` | 888 | `toolset.rs` | 625 | 🔄 | MCP stubbed; streaming wired |

### `soul/` Sub-Gaps

| Feature | Status | Notes |
|---------|--------|-------|
| `on_tool_result` streaming | ✅ | Fires `wire_send` per completed tool |
| D-Mail / time travel | ✅ | `BackToTheFuture` revert + re-inject |
| Auto-compaction | ✅ | Real LLM call with hooks + telemetry |
| Dynamic injection | ✅ | Plan mode + afk mode |
| Notification delivery | ✅ | Claim/ack/deliver to LLM context |
| Tool dedup | ✅ | Same-step + cross-step |
| Pre/PostToolUse hooks | ✅ | Blocks + fire-and-forget |
| MCP deferred loading | 🔄 | Scaffolding only; real client missing |
| Background task snapshot | ❌ | Blocked on BackgroundTaskManager |
| Subagent runner | ❌ | Not started |
| Side questions (`/btw`) | ❌ | Not started |
| Plan mode hero system | ❌ | `heroes.py` not ported |

---

## `subagents/` — Subagent System

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 43 | 🔄 | Stubs only |
| `core.py` | 300 | — | — | ❌ | Subagent core logic |
| `builder.py` | 200 | — | — | ❌ | Subagent builder |
| `runner.py` | 200 | — | — | ❌ | Subagent runner |
| `registry.py` | 100 | — | — | ❌ | Subagent registry |
| `models.py` | 100 | — | — | ❌ | Subagent models |
| `output.py` | 100 | — | — | ❌ | Output formatting |
| `git_context.py` | 100 | — | — | ❌ | Git context helper |
| `store.py` | 100 | — | — | ❌ | Subagent store |

---

## `telemetry/` — Telemetry

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | 100 | `mod.rs` | 258 | ✅ | `track!` macro + queue |
| `transport.py` | 100 | `transport.rs` | 322 | ✅ | HTTP + disk fallback |
| `sink.py` | 100 | `sink.rs` | 164 | ✅ | Event sink + batching |
| `crash.py` | 100 | — | — | ❌ | Panic hook / crash reporting |

---

## `tools/` — Tool Implementations

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 80 | ✅ | Tool trait + registry |
| **agent/__init__.py** | **277** | **agent/mod.rs** | **56** | **🔄** | **Stub only** |
| `ask_user/__init__.py` | — | `ask_user/mod.rs` | 76 | ✅ | AskUserQuestion tool |
| **background/__init__.py** | **318** | **background/mod.rs** | **106** | **🔄** | **TaskOutput/TaskStop only** |
| `dmail/__init__.py` | 38 | `dmail/mod.rs` | 75 | ✅ | SendDMail tool |
| `file/grep_local.py` | 590 | — | — | ❌ | Grep local implementation |
| `file/read.py` | 300 | — | — | ✅ | Inlined into `file/mod.rs` |
| `file/utils.py` | 257 | — | — | ✅ | Inlined into `file/mod.rs` |
| `plan/__init__.py` | 339 | `plan/mod.rs` | 80 | 🔄 | Enter/Exit plan mode; hero system missing |
| `plan/heroes.py` | 277 | — | — | ❌ | Hero name slug system |
| `shell/__init__.py` | 260 | `shell/mod.rs` | 169 | 🔄 | Basic shell; no `subprocess_env` |
| `think/__init__.py` | — | `think/mod.rs` | 50 | ✅ | Think tool |
| `todo/__init__.py` | — | `todo/mod.rs` | 72 | ✅ | SetTodoList tool |
| `web/__init__.py` | — | `web/mod.rs` | 109 | ✅ | SearchWeb + FetchURL |
| `file/__init__.py` | — | `file/mod.rs` | 559 | 🔄 | Read/Write/StrReplace/Glob/Grep |

### `tools/file/` Gaps

| Feature | Status | Notes |
|---------|--------|-------|
| ReadFile | ✅ | Implemented |
| WriteFile | ✅ | Implemented |
| StrReplaceFile | ✅ | Implemented |
| Glob | ✅ | Implemented |
| Grep | ✅ | Basic implementation |
| Grep local mode | ❌ | `grep_local.py` not ported |
| File utilities | ✅ | Inlined |

---

## `ui/` — User Interface

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `shell/__init__.py` | 1540 | `shell/mod.rs` | 779 | 🔄 | TUI shell partial; many features stubbed |
| `shell/prompt.py` | 2259 | — | — | ❌ | Rich prompt system |
| `shell/placeholders.py` | 531 | — | — | ❌ | Prompt placeholders |
| `shell/keyboard.py` | 300 | — | — | ❌ | Keyboard shortcuts |
| `shell/slash.py` | 893 | — | — | ✅ | Inlined into `soul/slash.rs` |
| `shell/task_browser.py` | 486 | — | — | ❌ | Background task browser UI |
| `shell/update.py` | 349 | — | — | ❌ | Update checker UI |
| `shell/usage.py` | 295 | — | — | ❌ | Token usage display |
| `shell/visualize/` | — | — | — | ❌ | Visualization subdir |
| `print/__init__.py` | 474 | `print/mod.rs` | 86 | 🔄 | Print mode partial |
| `acp/` | — | `acp/mod.rs` | — | ❌ | ACP UI missing |
| `picker.rs` | — | `picker.rs` | 196 | 🔄 | Interactive picker partial |
| `theme.rs` | — | `theme.rs` | — | 🔄 | In-memory theme only |

---

## `utils/` — Utilities

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `clipboard.py` | 246 | — | — | ❌ | Clipboard integration |
| `export.py` | 696 | — | — | ❌ | Export formatting |
| `file_filter.py` | 367 | — | — | ❌ | File filter logic |
| `path.py` | 245 | — | — | ❌ | Path utilities |
| `server.py` | — | — | — | ❌ | Server utilities |
| `sensitive.py` | — | — | — | ❌ | Sensitive data masking |
| `shell_quoting.py` | — | — | — | ❌ | Shell quoting |
| `signals.py` | — | — | — | ❌ | Signal handling |
| `slashcmd.py` | — | — | — | ✅ | Inlined into `soul/slash.rs` |
| `string.py` | — | — | — | ❌ | String utilities |
| `subprocess_env.py` | — | — | — | ❌ | Subprocess env setup |
| `term.py` | — | — | — | ❌ | Terminal detection |
| `typing.py` | — | — | — | ❌ | Type helpers |
| `windows_paths.py` | — | — | — | ❌ | Windows path handling |
| `rich/diff_render.py` | 481 | — | — | ❌ | Rich diff rendering |
| `rich/markdown.py` | 898 | — | — | ❌ | Rich markdown rendering |
| `rich/syntax.py` | — | — | — | ❌ | Rich syntax highlighting |
| `mod.rs` | — | `mod.rs` | 48 | 🔄 | Minimal utilities |

---

## `vis/` — Visualizer

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | — | ❌ | Stub only |
| `app.py` | 451 | — | — | ❌ | Visualizer web app |
| `api/sessions.py` | 687 | — | — | ❌ | Sessions API |
| `api/statistics.py` | — | — | — | ❌ | Statistics API |
| `api/system.py` | — | — | — | ❌ | System API |

---

## `web/` — Web UI

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | — | ❌ | Stub only |
| `app.py` | 451 | — | — | ❌ | Web app backend |
| `auth.py` | — | — | — | ❌ | Web auth |
| `models.py` | — | — | — | ❌ | Web models |
| `api/config.py` | — | — | — | ❌ | Config API |
| `api/open_in.py` | — | — | — | ❌ | Open-in API |
| `api/sessions.py` | 1223 | — | — | ❌ | Sessions REST API |
| `runner/process.py` | 754 | — | — | ❌ | Web runner process |
| `runner/worker.py` | — | — | — | ❌ | Web runner worker |
| `runner/messages.py` | — | — | — | ❌ | Web runner messages |
| `store/sessions.py` | 432 | — | — | ❌ | Session store |

---

## `wire/` — Wire Protocol

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 351 | 🔄 | Core types + `wire_send()` |
| `file.py` | — | `file.rs` | 126 | ✅ | JSONL wire file backend |
| `jsonrpc.py` | 263 | — | — | ❌ | JSON-RPC protocol |
| `protocol.py` | — | — | — | ❌ | Wire protocol definitions |
| `root_hub.py` | — | — | — | ✅ | Inlined into `mod.rs` |
| `serde.py` | — | — | — | ❌ | Serde helpers |
| `server.py` | 1060 | — | — | ❌ | Wire server (WebSocket) |
| `types.py` | 717 | — | — | ✅ | Inlined into `mod.rs` |
| `channel.rs` | — | `channel.rs` | 89 | ✅ | `tokio::sync::broadcast` channel |

---

## Feature Matrix (Cross-Cutting)

| Feature Area | Python Files | Rust Files | Status | Key Gaps |
|-------------|-------------|-----------|--------|----------|
| **Core Agent Loop** | 6 | 6 | 🔄 75% | Streaming LLM, subagents, btw |
| **Context/Compaction** | 2 | 2 | ✅ 85% | Background task snapshot |
| **Tool System** | 11 | 11 | 🔄 70% | MCP, agent tool, grep local |
| **Hooks** | 4 | 4 | ✅ 90% | — |
| **Notifications** | 6 | 5 | ✅ 90% | Push notifier |
| **Approval** | 3 | 1 | 🔄 60% | Real wire approval flow |
| **Telemetry** | 4 | 3 | ✅ 85% | Crash reporting |
| **Background Tasks** | 7 | 1 | ❌ 10% | Full subsystem missing |
| **Subagents** | 8 | 1 | ❌ 10% | Full subsystem missing |
| **Skills/Flows** | 5 | 1 | ❌ 10% | Full subsystem missing |
| **MCP/ACP** | 9 | 2 | 🔄 30% | Real client, server, OAuth |
| **Web/Visualizer** | 14 | 2 | ❌ 5% | Full subsystem missing |
| **UI Shell** | 9 | 4 | 🔄 40% | Prompt, keyboard, task browser |
| **Auth/OAuth** | 3 | 1 | 🔄 30% | Real OAuth flow |
| **Wire Protocol** | 8 | 3 | 🔄 60% | JSON-RPC, WebSocket server |
| **Utils** | 16 | 1 | 🔄 20% | Clipboard, export, rich render |

---

## What's Next? (Prioritized by Impact)

| Priority | Feature | Blocked On | Est. Effort |
|----------|---------|-----------|-------------|
| P0 | **Real OAuth + token refresh** | — | Medium |
| P0 | **Streaming LLM completion** | — | Medium |
| P1 | **Agent/Runtime loading** (Jinja2, AGENTS.md) | — | Large |
| P1 | **Background Tasks** (manager, store, snapshot) | — | Large |
| P1 | **Subagents** (runner, registry, store) | Background Tasks | Large |
| P2 | **Side Questions (`/btw`)** | — | Medium |
| P2 | **Web/Visualizer servers** | — | Large |
| P2 | **Skills/Flows** | Agent loading | Large |
| P3 | **Rich UI** (prompt, keyboard, task browser) | — | Large |
| P3 | **Utils** (clipboard, export, rich render) | — | Medium |
