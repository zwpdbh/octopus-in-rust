# Octopus-CLI ↔ Kimi-CLI Mirror Rewrite Comparison

> File-by-file, folder-by-folder comparison between Python `kimi-cli` and Rust `octopus-cli`.
>
> **Legend:** ✅ Complete | 🔄 Partial / Stub | ❌ Missing | **Bold** = critical path
>
> **Last updated:** 2026-05-24
>
> **P0 = complete | P1 = complete | P2-P3 = remaining polish**
>
> **Quick status:** ~95% of core TUI features done. All daily-use features work: chat, tools, approval, OAuth, MCP, background tasks, subagents, skills, clipboard, history, editor, markdown rendering, `/btw`. Remaining gaps are polish (task browser, crash reporting, theme switching, agent YAML) and out-of-scope items (web/vis/ACP servers).

---

## Summary

| Metric | Python (kimi-cli) | Rust (octopus-cli) | Ratio |
|--------|-------------------|--------------------|-------|
| Total LOC | ~47,200 | ~15,200 | 32% |
| Files | ~120 `.py` | ~75 `.rs` | 63% |
| Modules | 28 folders | 22 folders | 79% |

**Core agent experience: ~95% complete (TUI-only scope).** All P0 and P1 features are implemented: interactive TUI with rich rendering, agent loop with streaming, full tool system, session management, OAuth, approval flow, MCP stdio client, background tasks, subagents, skills discovery, clipboard, history navigation, external editor, and `/btw` side questions.

**Scope note:** Web server, Visualizer server, and ACP server are **out of scope** for the TUI-focused rewrite. These would require WASM frontend + Axum backend stacks that are better suited to a separate project.

**Major remaining gaps (TUI):** `/task` browser, crash reporting, real-time theme switching, and agent YAML loading.

---

## Top-Level Files

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `lib.rs` | 31 | 🔄 | Package init only |
| `__main__.py` | — | `main.rs` | 430 | 🔄 | CLI entry; login/logout wired to OAuth |
| **app.py** | **807** | **app.rs** | **233** | **🔄** | **Runtime builder; background tasks wired** |
| **config.py** | **429** | **config.rs** | **433** | **🔄** | **Models done; no live config reload** |
| `constant.py` | — | `constant.rs` | — | ✅ | Constants ported |
| `exception.py` | — | `exception.rs` | 223 | 🔄 | Core errors done; `BackToTheFuture` added |
| **llm.py** | **333** | **llm.rs** | **446** | **✅** | **Streaming + non-streaming via kosong. Kimi, OpenAI Legacy, OpenAI Responses supported.** |
| `metadata.py` | — | `metadata.rs` | 106 | ✅ | Build metadata |
| **session.py** | **319** | **session.rs** | **290** | **🔄** | **Create/resume/continue/list/delete working. Fork/undo partial.** |
| `session_state.py` | — | `session_state.rs` | 181 | 🔄 | Approval state synced |
| `session_fork.py` | 325 | `soul/slash.rs` | — | ✅ | `fork_session()` + `enumerate_turns()` inlined |
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
| `__init__.py` | — | `mod.rs` | 123 | ✅ | `ApprovalRuntime` with wire events + ShellUI overlay |
| `models.py` | — | — | — | ✅ | Models inlined into `mod.rs` |
| `runtime.py` | — | — | — | ✅ | Runtime inlined into `mod.rs` |

**Status:** ✅ Complete. Interactive approval via `ApprovalRuntime` + `RootWireHub` + ShellUI overlay. YOLO/AFK/plan mode toggles work.

---

## `auth/` — Authentication

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 251 | ✅ | OAuth manager implemented |
| **oauth.py** | **1092** | `oauth.rs` | 320 | ✅ | Full OAuth 2.0 device flow implemented |
| `platforms.py` | 374 | `platforms.rs` | 75 | ✅ | Platform definitions |

---

## `background/` — Background Tasks

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 45 | ✅ | Module init |
| **manager.py** | **725** | `background/mod.rs` | **205** | **✅** | **Task lifecycle (create/run/kill/wait/output)** |
| `models.py` | 105 | `background/mod.rs` | — | ✅ | Inlined |
| `store.py` | 150 | — | — | ❌ | File/SQLite persistence (deferred) |
| `summary.py` | 66 | — | — | ❌ | Task summarization (deferred) |
| `worker.py` | 200 | `background/mod.rs` | — | ✅ | Background reader tasks |
| `agent_runner.py` | 150 | `tools/agent/mod.rs` | — | ✅ | Agent background runner |
| `ids.py` | 50 | `background/mod.rs` | — | ✅ | UUID-based task IDs |

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
| `__init__.py` | — | `mod.rs` | 115 | ✅ | Config models + `McpClient` stdio transport |

**Status:** ✅ Stdio transport implemented. `McpClient` handles JSON-RPC initialize handshake, `tools/list`, and `tools/call`. `McpTool` wraps remote tools as local `Tool` trait objects. HTTP/SSE and OAuth not yet supported.

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
| `__init__.py` | — | `mod.rs` | — | ✅ | `init.md` + `compact.md` embedded |
| `init.md` | — | `init.md` | — | ✅ | System prompt template |
| `compact.md` | — | `compact.md` | — | ✅ | Compaction prompt template |

---

## `skills/` — Skills System

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | 727 | `mod.rs` | 180 | ✅ | Skill discovery with frontmatter parsing |
| `flow/d2.py` | 482 | — | — | ❌ | D2 diagram flow rendering |
| `flow/mermaid.py` | 266 | — | — | ❌ | Mermaid flow rendering |
| `kimi-cli-help/` | 200 | — | — | ❌ | CLI help skill |
| `skill-creator/` | — | — | — | ❌ | Skill creator skill |

---

## `soul/` — Agent Core

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | 304 | — | — | ✅ | Module exports in `mod.rs` |
| **kimisoul.py** | **1710** | **mod.rs** | **1290** | **✅** | **Core loop complete; all P0/P1 wired** |
| `agent.py` | 519 | `agent.rs` | 202 | 🔄 | `load_agent()` stubbed; no Jinja2. `DenwaRenji` inlined. |
| `approval.py` | 267 | `approval.rs` | 255 | ✅ | Full approval flow with wire events and ShellUI overlay |
| `btw.py` | — | `soul/slash.rs` | — | ✅ | `/btw` one-off LLM call with `BtwBegin`/`BtwEnd` |
| `compaction.py` | — | `compaction.rs` | 169 | ✅ | Real LLM compaction with hooks + telemetry |
| `context.py` | 339 | `context.rs` | 385 | 🔄 | Checkpoint/revert, token counting, `context.jsonl`. Malformed line logging missing. |
| `denwarenji.py` | 39 | — | — | ✅ | Inlined into `agent.rs` |
| `dynamic_injection.py` | — | `dynamic_injection.rs` | 48 | ✅ | Trait + collection |
| `dynamic_injections/__init__.py` | — | `mod.rs` | — | ✅ | Module init |
| `dynamic_injections/afk_mode.py` | — | `afk_mode.rs` | 74 | ✅ | Afk mode injection |
| `dynamic_injections/plan_mode.py` | — | `plan_mode.rs` | 256 | ✅ | Plan mode injection |
| `message.py` | — | `message.rs` | 129 | 🔄 | `tool_result_to_message()` simplified |
| `slash.py` | 341 | `slash.rs` | 1240 | ✅ | 20+ commands; all core commands implemented |
| **toolset.py** | **888** | **toolset.rs** | **777** | **🔄** | **MCP stdio wired; kosong streaming fully wired** |

### `soul/` Sub-Gaps

| Feature | Status | Notes |
|---------|--------|-------|
| Streaming LLM + early tool dispatch | ✅ | `generate_streaming()` with `on_message_part` + `on_tool_call` |
| Concurrent tool execution | ✅ | `futures::future::join_all()` |
| `on_tool_result` streaming | ✅ | Fires `wire_send` per completed tool |
| D-Mail / time travel | ✅ | `BackToTheFuture` revert + re-inject |
| Auto-compaction | ✅ | Real LLM call with hooks + telemetry |
| Dynamic injection | ✅ | Plan mode + afk mode |
| Notification delivery | ✅ | Claim/ack/deliver to LLM context |
| Tool dedup | ✅ | Same-step + cross-step |
| Pre/PostToolUse hooks | ✅ | Blocks + fire-and-forget |
| MCP deferred loading | ✅ | Real stdio client; synchronous load |
| Background task snapshot | ✅ | `BackgroundTaskManager` with real processes |
| Subagent runner | ✅ | Foreground + background subagents via `AgentTool` |
| Side questions (`/btw`) | ✅ | One-off LLM call with `BtwBegin`/`BtwEnd` wire events |
| Plan mode hero system | ❌ | `heroes.py` not ported |
| Connection recovery | ✅ | 401 → OAuth refresh with retry. Connection/timeout errors retry once. |

---

## `subagents/` — Subagent System

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `__init__.py` | — | `mod.rs` | 43 | 🔄 | `LaborMarket`, `SubagentStore` stubs |
| `core.py` | 300 | `tools/agent/mod.rs` | 160 | ✅ | Subagent execution via new `KimiSoul` |
| `builder.py` | 200 | — | — | ❌ | Subagent builder (deferred) |
| `runner.py` | 428 | `tools/agent/mod.rs` | — | ✅ | Foreground + background runner |
| `registry.py` | 100 | — | — | ❌ | Subagent registry (deferred) |
| `models.py` | 100 | — | — | 🔄 | `AgentParams` in `tools/agent/mod.rs` |
| `output.py` | 100 | — | — | ❌ | Output formatting (deferred) |
| `git_context.py` | 100 | — | — | ❌ | Git context helper (deferred) |
| `store.py` | 100 | — | — | 🔄 | `SubagentStore` stub exists |

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
| `agent/__init__.py` | 277 | `agent/mod.rs` | 160 | ✅ | `AgentTool` with foreground + background subagents |
| `ask_user/__init__.py` | — | `ask_user/mod.rs` | 76 | ✅ | AskUserQuestion tool |
| `background/__init__.py` | 318 | `background/mod.rs` | 160 | ✅ | Full process management |
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
| `shell/__init__.py` | 1540 | `shell/mod.rs` | 1148 | ✅ | TUI shell functional; all core features implemented |
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

### `ui/shell/` Sub-Gaps

| Feature | Status | Notes |
|---------|--------|-------|
| Raw mode / alternate screen | ✅ | `crossterm` |
| Input editing | ✅ | Basic editing |
| Multiline support | ✅ | `Alt+Enter` |
| Slash command completion | ✅ | Menu popup |
| Ctrl-C abort | ✅ | Graceful cancel |
| Welcome panel | ✅ | Rendered |
| Async soul execution | ✅ | Background tokio task |
| Message history display | ✅ | Working |
| History navigation (Up/Down) | ✅ | Up/Down arrows cycle through persisted history; draft preserved |
| External editor (Ctrl-O) | ✅ | Opens `$VISUAL`/`$EDITOR` in temp file; restores TUI on close |
| Rich markdown rendering | ✅ | `pulldown-cmark` + `syntect` for code blocks, diffs, inline formatting |
| Hero name slugs | ❌ | `// TODO` |

---

## `utils/` — Utilities

| Python | LOC | Rust | LOC | Status | Gap |
|--------|-----|------|-----|--------|-----|
| `clipboard.py` | 246 | `utils/clipboard.rs` | 30 | ✅ | `copy_text()` + `paste_text()` via `arboard` |
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
| `rich/diff_render.py` | 481 | `ui/shell/render.rs` | — | ✅ | Diff block coloring (+ green, − red) |
| `rich/markdown.py` | 898 | `ui/shell/render.rs` | — | ✅ | `pulldown-cmark` → ratatui `Line`s |
| `rich/syntax.py` | — | `ui/shell/render.rs` | — | ✅ | `syntect` syntax highlighting for code blocks |
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
| **server.py** | **1060** | — | — | **❌** | **Wire server (JSON-RPC over stdio) missing** |
| `types.py` | 717 | — | — | ✅ | Inlined into `mod.rs` |
| `channel.rs` | — | `channel.rs` | 89 | ✅ | `tokio::sync::broadcast` channel |

---

## Feature Matrix (Cross-Cutting)

| Feature Area | Python Files | Rust Files | Status | Key Gaps |
|-------------|-------------|-----------|--------|----------|
| **Core Agent Loop** | 6 | 6 | ✅ 98% | Hero name slugs (cosmetic) |
| **Context/Compaction** | 2 | 2 | ✅ 95% | — |
| **Tool System** | 11 | 11 | ✅ 90% | Grep local mode, agent YAML loading |
| **Hooks** | 4 | 4 | ✅ 95% | Wire-side hooks deferred |
| **Notifications** | 6 | 5 | ✅ 85% | Push notifier (desktop popup) |
| **Approval** | 3 | 1 | ✅ 100% | Wire request/response + ShellUI overlay |
| **Telemetry** | 4 | 3 | ✅ 90% | Crash reporting (panic hook) |
| **Background Tasks** | 7 | 3 | ✅ 85% | No task browser UI; no persistence |
| **Subagents** | 8 | 2 | ✅ 80% | No registry/builder/output formatting |
| **Skills/Flows** | 5 | 1 | ✅ 80% | No flowcharts (Mermaid/D2) |
| **MCP/ACP** | 9 | 2 | 🔄 70% | Stdio client ✅; ACP server, MCP OAuth deferred |
| **Web/Visualizer** | 14 | — | ❌ N/A | Out of scope for TUI rewrite |
| **UI Shell** | 9 | 5 | ✅ 85% | Task browser, token usage display deferred |
| **Auth/OAuth** | 3 | 3 | ✅ 100% | Full OAuth 2.0 device flow |
| **Wire Protocol** | 8 | 5 | ✅ 80% | JSON-RPC wire server deferred |
| **Utils** | 16 | 4 | ✅ 60% | Export limited; file filter, signals deferred |

---

## TODO List (Prioritized by Impact)

### P0 — Must have for a usable CLI

| # | Feature | Status | Blocked On | Notes |
|---|---------|--------|-----------|-------|
| 1 | ~~**Real OAuth + token refresh**~~ | ✅ | — | `auth/oauth.rs` + `auth/platforms.rs` + `auth/mod.rs`. Device flow, token refresh, file storage with atomic writes. |
| 2 | **Real approval flow** | ✅ | Wire server | `ApprovalRuntime` publishes `ApprovalRequestEvent` to `RootWireHub`; `ShellUI` subscribes and renders overlay; Y/N/A keys resolve via `ApprovalRuntime::resolve()`. |
| 3 | **MCP client** | ✅ | `mcp/client.rs` | Hand-rolled JSON-RPC stdio client. `McpClient::connect_stdio()` spawns process, performs initialize handshake, supports `tools/list` and `tools/call`. `McpTool` implements `Tool` trait for seamless integration. HTTP/SSE and OAuth are future work.

### P1 — Important for feature parity

| # | Feature | Status | Blocked On | Notes |
|---|---------|--------|-----------|-------|
| 4 | **Background Tasks** | ✅ | `background/mod.rs` + `tools/shell/mod.rs` + `tools/background/mod.rs` | `BackgroundTaskManager` spawns real `tokio::process::Child` processes, captures stdout/stderr via reader tasks. `ShellTool` with `run_in_background=true` creates tasks. `TaskOutputTool` and `TaskStopTool` query and kill tasks. |
| 5 | **Subagents** | ✅ | `tools/agent/mod.rs` | `AgentTool` creates a new `KimiSoul` with a fresh session and runs the prompt. Supports foreground (waits for result) and background (`tokio::spawn`) modes. Full runner/registry/store are future work. |
| 6 | **Skills discovery** | ✅ | `skills/mod.rs` | `SkillRegistry::discover()` scans `~/.kimi/skills/` and `<work_dir>/.kimi/skills/` for `SKILL.md` (subdirectory) and `*.md` (flat) layouts. Parses simple YAML frontmatter for `name` and `description`. Flowcharts (Mermaid/D2) are future work. |
| 7 | **Side Questions (`/btw`)** | ✅ | — | One-off `llm.complete()` call with `BtwBegin`/`BtwEnd` wire events. Answer rendered with `btw` role (💡 magenta). |
| 8 | **Rich rendering** (markdown, syntax highlighting, diff) | ✅ | — | `ui/shell/render.rs` uses `pulldown-cmark` + `syntect`. Code blocks highlighted, diff blocks colored (+ green, − red), inline formatting supported. |

### P2 — Medium impact

| # | Feature | Status | Blocked On | Notes |
|---|---------|--------|-----------|-------|
| 9 | **History navigation** (up/down arrows, `/history`) | ✅ | — | Persisted to `~/.kimi/shell_history.txt`. Up/Down cycle through history; draft preserved. |
| 10 | **Task browser UI** (`/task` slash command) | ❌ | — | Background tasks work but there's no TUI to list active/completed tasks. |
| 11 | **Wire server** (JSON-RPC over stdio) | ❌ | — | Enables external editor integration (`kimi open-in`, VS Code extension). Useful but not critical for standalone TUI. |
| 12 | **Session fork/clone** | ✅ | — | `fork_session()` + `enumerate_turns()` in `slash.rs`. `/fork` copies session. `/undo <n>` forks at turn N and switches. |
| 13 | **Crash reporting** (panic hook) | ❌ | — | `telemetry/crash.py` not ported. |
| 14 | **Push notifier** | ❌ | — | `notifications/notifier.py` not ported. Desktop notifications when background tasks complete. |
| 15 | **Agent/Runtime loading** (Jinja2, AGENTS.md) | 🔄 | — | `agents/mod.rs` returns hardcoded default. YAML files exist but are ignored. Low daily impact. |
| 16 | ~~**Web UI server**~~ | ❌ | — | **Out of scope** — would require Dioxus/Leptos WASM frontend + Axum backend. |
| 17 | ~~**Visualizer server**~~ | ❌ | — | **Out of scope** — same stack as web server. |
| 18 | ~~**ACP server**~~ | ❌ | — | **Out of scope** — ACP is an IDE/editor protocol; TUI doesn't need it. |

### P3 — Polish / Utilities

| # | Feature | Status | Blocked On | Notes |
|---|---------|--------|-----------|-------|
| 19 | **Export formatting** | 🔄 | — | `cli/export.rs` limited. `utils/export.py` not ported. |
| 20 | **Clipboard integration** | ✅ | — | `Ctrl+V` paste, `Ctrl+Y` copy last assistant, `/copy` and `/copy all` slash commands. |
| 21 | **File filter logic** | ❌ | — | `utils/file_filter.py` not ported. |
| 22 | **Grep local mode** | ❌ | — | `tools/file/grep_local.py` not ported. |
| 23 | **Hero name slug system** | ❌ | — | `tools/plan/heroes.py` not ported. |
| 24 | **Signal handling** | ❌ | — | `utils/signals.py` not ported. |
| 25 | **Windows path handling** | ❌ | — | `utils/windows_paths.py` not ported. |
| 26 | **Plugin system** | ❌ | — | `plugin/mod.rs` stub. |
| 27 | **Toad TUI** | ❌ | — | `term`/`toad` subcommands stubbed. |

---

## Kosong Crate Status

See [`kosong_mirror_comparison.md`](./kosong_mirror_comparison.md) for the detailed `kosong` parity analysis.

**Summary:** `kosong` is **~100% complete for the default `kimi-cli` user**. All core providers (Kimi, OpenAI Legacy, OpenAI Responses) + testing providers (Echo, Mock) are implemented. Only Anthropic and Google GenAI remain as optional extras requiring manual configuration.
