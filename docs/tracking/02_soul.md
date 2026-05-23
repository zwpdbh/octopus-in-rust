# 02 — Soul (Agent Core)

## Status: 🔄 Partial

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/soul/__init__.py` | Soul protocol, `run_soul()`, wire pump | ~304 |
| `kimi_cli/soul/kimisoul.py` | `KimiSoul` class, agent loop | ~400 |
| `kimi_cli/soul/slash.py` | Soul-level slash commands | ~341 |
| `kimi_cli/soul/agent.py` | Agent / runtime models | ~300 |
| `kimi_cli/soul/approval.py` | Approval state machine | ~200 |
| `kimi_cli/soul/compaction.py` | Context compaction | ~250 |
| `kimi_cli/soul/context.py` | Context management | ~400 |
| `kimi_cli/soul/message.py` | Message formatting | ~150 |
| `kimi_cli/soul/toolset.py` | Tool registration | ~200 |
| `kimi_cli/soul/btw.py` | Side-question modal | ~150 |
| `kimi_cli/soul/denwarenji.py` | Denwarenji protocol | ~100 |
| `kimi_cli/soul/dynamic_injection.py` | Dynamic system prompt injection | ~100 |
| `kimi_cli/soul/dynamic_injections/afk_mode.py` | AFK reminder text | ~20 |
| `kimi_cli/soul/dynamic_injections/plan_mode.py` | Plan mode reminder text | ~20 |
| `kimi_cli/utils/slashcmd.py` | Slash command registry utility | ~145 |
| `kimi_cli/session_fork.py` | Session forking for undo/fork | ~200 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/soul/mod.rs` | `KimiSoul`, agent loop, status | ~486 | 🔄 Core loop done; missing notifications, hooks, steers |
| `octopus-cli/src/soul/slash.rs` | Soul-level slash commands | ~1,118 | ✅ All major commands ported with mirror comments |
| `octopus-cli/src/soul/agent.rs` | Agent struct, system prompt | ~162 | 🔄 Basic agent exists |
| `octopus-cli/src/soul/approval.rs` | Approval state | ~234 | ✅ `ApprovalState` with yolo/afk |
| `octopus-cli/src/soul/compaction.rs` | Context compaction | ~151 | 🔄 Simple compaction only |
| `octopus-cli/src/soul/context.rs` | Context file backend | ~385 | 🔄 File-based context with checkpoints |
| `octopus-cli/src/soul/message.rs` | Message helpers | ~98 | 🔄 Basic message formatting |
| `octopus-cli/src/soul/toolset.rs` | Tool registration | ~109 | 🔄 Tool dispatch works |

## Detailed Mapping

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `Soul` protocol | `KimiSoul` struct | 🔄 | Direct struct instead of trait |
| `run_soul()` | `KimiSoul::run()` | 🔄 | Missing notification pump, cancel event |
| `Soul.run()` | `KimiSoul::run()` | 🔄 | Missing `skip_user_prompt_hook` |
| `_agent_loop()` | `KimiSoul::_agent_loop()` | 🔄 | Missing steer consumption |
| `_step()` | `KimiSoul::_step()` | 🔄 | Missing tool rejection handling |
| `SoulSlashCmdFunc` | `SoulSlashCmdFunc` type alias | ✅ | Fully commented mirror |
| `@registry.command` | `registry.register()` | ✅ | Imperative instead of decorator |
| `/clear` | `build_default_slash_commands()` | ✅ | Including alias `/reset` |
| `/yolo` | `build_default_slash_commands()` | ✅ | |
| `/afk` | `build_default_slash_commands()` | ✅ | |
| `/plan` | `build_default_slash_commands()` | ✅ | on/off/view/clear subcommands |
| `/compact` | `build_default_slash_commands()` | ✅ | |
| `/add-dir` | `build_default_slash_commands()` | 🔄 | Missing KaosPath, directory validation |
| `/undo` | `build_default_slash_commands()` | ✅ | With `fork_session()` helper |
| `/fork` | `build_default_slash_commands()` | ✅ | |
| `/debug` | `build_default_slash_commands()` | ✅ | |
| `/changelog` | `build_default_slash_commands()` | ✅ | |
| `/help` | `build_default_slash_commands()` | ✅ | Soul-level version |
| `/version` | `build_default_slash_commands()` | ✅ | |
| `/model` | `build_default_slash_commands()` | 🔄 | Show only; no switching |
| `/feedback` | `build_default_slash_commands()` | 🔄 | GitHub fallback only |
| `/new` | `build_default_slash_commands()` | ✅ | |
| `/title` | `build_default_slash_commands()` | ✅ | |
| `/sessions` | `build_default_slash_commands()` | ✅ | |
| `/web` | `build_default_slash_commands()` | ⬜ | Placeholder |
| `/vis` | `build_default_slash_commands()` | ⬜ | Placeholder |
| `/mcp` | `build_default_slash_commands()` | 🔄 | Basic status dump |
| `/hooks` | `build_default_slash_commands()` | 🔄 | Basic list |
| `/btw` | `build_default_slash_commands()` | ⬜ | Placeholder |
| `/editor` | `build_default_slash_commands()` | 🔄 | Basic validation |
| `/task` | `build_default_slash_commands()` | ⬜ | Placeholder |
| `/theme` | `build_default_slash_commands()` | 🔄 | In-memory only |
| `fork_session()` | `fork_session()` | ✅ | Mirrored with comments |
| `enumerate_turns()` | `enumerate_turns()` | ✅ | Mirrored with comments |

## What's Missing

- [ ] Notification pump (`_pump_notifications_to_wire`)
- [ ] Cancel event handling (`asyncio.Event` equivalent)
- [ ] Dynamic system prompt injection (`dynamic_injection.py`)
- [ ] AFK disabled reminder injection
- [ ] Plan mode dynamic injection
- [ ] `/btw` side-question modal
- [ ] `/model` interactive switching
- [ ] Shell-level slash commands (`ui/shell/slash.py`) — some exist as soul-level stubs
