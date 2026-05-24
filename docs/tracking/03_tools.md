# 03 — Tools

## Status: 🔄 Partial

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/tools/__init__.py` | Tool base classes | ~200 |
| `kimi_cli/tools/shell/__init__.py` | Shell command tool | ~150 |
| `kimi_cli/tools/file/__init__.py` | Read/Write/Replace/Glob/Grep | ~400 |
| `kimi_cli/tools/file/read.py` | Read file logic | ~150 |
| `kimi_cli/tools/file/write.py` | Write file logic | ~100 |
| `kimi_cli/tools/file/replace.py` | StrReplaceFile logic | ~200 |
| `kimi_cli/tools/file/glob.py` | Glob tool | ~100 |
| `kimi_cli/tools/file/grep_local.py` | Grep tool | ~150 |
| `kimi_cli/tools/file/utils.py` | File tool utilities | ~100 |
| `kimi_cli/tools/file/plan_mode.py` | Plan mode file helpers | ~50 |
| `kimi_cli/tools/web/__init__.py` | Web search / fetch | ~150 |
| `kimi_cli/tools/web/search.py` | SearchWeb implementation | ~100 |
| `kimi_cli/tools/web/fetch.py` | FetchURL implementation | ~100 |
| `kimi_cli/tools/ask_user/__init__.py` | AskUser tool | ~100 |
| `kimi_cli/tools/todo/__init__.py` | Todo list tool | ~100 |
| `kimi_cli/tools/think/__init__.py` | Think tool | ~50 |
| `kimi_cli/tools/plan/__init__.py` | Plan mode tools | ~100 |
| `kimi_cli/tools/plan/enter.py` | EnterPlanMode | ~50 |
| `kimi_cli/tools/plan/heroes.py` | Plan heroes | ~50 |
| `kimi_cli/tools/agent/__init__.py` | Agent tool | ~150 |
| `kimi_cli/tools/background/__init__.py` | TaskOutput/TaskStop tools | ~100 |
| `kimi_cli/tools/display.py` | Display tool | ~50 |
| `kimi_cli/tools/dmail/__init__.py` | Dmail tool | ~100 |
| `kimi_cli/tools/test.py` | Test tool | ~50 |
| `kimi_cli/tools/utils.py` | Tool utilities | ~50 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/tools/mod.rs` | Tool trait definitions | ~79 | 🔄 Basic trait |
| `octopus-cli/src/tools/shell/mod.rs` | ShellTool | ~169 | ✅ |
| `octopus-cli/src/tools/file/mod.rs` | File tools (read/write/replace/glob/grep) | ~559 | ✅ |
| `octopus-cli/src/tools/web/mod.rs` | SearchWeb / FetchURL | ~109 | 🔄 Skeleton |
| `octopus-cli/src/tools/ask_user/mod.rs` | AskUserTool | ~? | 🔄 |
| `octopus-cli/src/tools/todo/mod.rs` | SetTodoListTool | ~? | 🔄 |
| `octopus-cli/src/tools/think/mod.rs` | ThinkTool | ~? | 🔄 |
| `octopus-cli/src/tools/plan/mod.rs` | EnterPlanMode / ExitPlanMode | ~80 | 🔄 |
| `octopus-cli/src/tools/agent/mod.rs` | AgentTool | ~? | 🔄 |
| `octopus-cli/src/tools/background/mod.rs` | TaskOutput / TaskStop | ~106 | 🔄 Skeleton |

## What's Done

- [x] `Tool` trait with JSON schema generation
- [x] `ShellTool` — shell command execution
- [x] `ReadFileTool` — file reading with line offsets
- [x] `WriteFileTool` — file writing
- [x] `StrReplaceFileTool` — string replacement
- [x] `GlobTool` — file globbing
- [x] `GrepTool` — ripgrep-based search
- [x] `AskUserTool` — user question prompt
- [x] `SetTodoListTool` — todo list management
- [x] `ThinkTool` — reasoning tool
- [x] `EnterPlanModeTool` / `ExitPlanModeTool`
- [x] `AgentTool` — subagent dispatch
- [x] `TaskOutputTool` / `TaskStopTool` stubs

## What's Missing

- [ ] `SearchWebTool` full implementation (stub — needs service config wiring)
- [x] `FetchURLTool` full implementation (HTTP GET with content extraction)
- [ ] `display` tool
- [ ] `dmail` tool
- [ ] `test` tool
- [ ] KaosPath integration (file paths use raw `PathBuf`)
- [ ] File tool validation (sensitive file checks, path escaping)
- [ ] Plan mode heroes
