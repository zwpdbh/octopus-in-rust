# 09 — UI / Shell

## Status: 🔄 Partial

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/ui/__init__.py` | UI package | ~50 |
| `kimi_cli/ui/shell/__init__.py` | Shell app, main loop | ~400 |
| `kimi_cli/ui/shell/prompt.py` | Prompt session setup | ~200 |
| `kimi_cli/ui/shell/console.py` | Rich console config | ~50 |
| `kimi_cli/ui/shell/slash.py` | Shell-level slash commands | ~893 |
| `kimi_cli/ui/shell/visualize/*.py` | Live view, blocks, panels | ~1,500 |
| `kimi_cli/ui/shell/keyboard.py` | Key bindings | ~100 |
| `kimi_cli/ui/shell/mcp_status.py` | MCP status rendering | ~150 |
| `kimi_cli/ui/shell/session_picker.py` | Session picker UI | ~200 |
| `kimi_cli/ui/shell/task_browser.py` | Background task browser | ~300 |
| `kimi_cli/ui/shell/usage.py` | Usage stats display | ~100 |
| `kimi_cli/ui/shell/debug.py` | Debug panel | ~100 |
| `kimi_cli/ui/shell/replay.py` | Session replay | ~150 |
| `kimi_cli/ui/theme.py` | Theme definitions | ~100 |
| `kimi_cli/ui/print/__init__.py` | Print mode UI | ~100 |
| `kimi_cli/ui/acp/__init__.py` | ACP UI | ~50 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/ui/mod.rs` | UI module | ~? | 🔄 |
| `octopus-cli/src/ui/shell/mod.rs` | Shell UI, input loop | ~779 | 🔄 Basic shell |
| `octopus-cli/src/ui/picker.rs` | Generic picker | ~196 | 🔄 |
| `octopus-cli/src/ui/theme.rs` | Theme definitions | ~? | 🔄 |
| `octopus-cli/src/ui/print/mod.rs` | Print mode | ~? | 🔄 |
| `octopus-cli/src/ui/acp/mod.rs` | ACP UI placeholder | ~? | ⬜ |

## What's Done

- [x] Basic input loop with history
- [x] `TextPart` display
- [x] `StatusUpdate` display
- [x] Slash command parsing and dispatch
- [x] Basic theme support (dark/light)

## What's Missing

- [ ] Rich console integration (colors, markdown, syntax highlighting)
- [ ] Live view with streaming output
- [ ] Approval panel (interactive yes/no)
- [ ] AskUserQuestion panel
- [ ] BTW modal
- [ ] Tool call visualization
- [ ] Keyboard shortcuts (Ctrl-O, Ctrl-X, Ctrl-V, etc.)
- [ ] Session picker interactive UI
- [ ] Task browser
- [ ] MCP status live rendering
- [ ] Replay mode
- [ ] Image paste support
