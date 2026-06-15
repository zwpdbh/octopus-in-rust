# Octopus-CLI 1:1 Rewrite Tracker

This directory tracks the progress of rewriting `kimi-cli` (Python) into `octopus-cli` (Rust).

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Complete — all major features ported and commented |
| 🔄 | Partial — core structure exists, some features missing |
| ⬜ | Not Started — module does not exist or is empty stub |

## Quick Overview

| # | Area | Status | Python LOC | Rust LOC |
|---|------|--------|-----------|----------|
| 01 | Core / CLI Entry | 🔄 | ~800 | ~650 |
| 02 | Soul (Agent Core) | 🔄 | ~1,800 | ~2,400 |
| 03 | Tools | 🔄 | ~1,200 | ~1,100 |
| 04 | Wire / Messaging | 🔄 | ~1,000 | ~240 |
| 05 | Session Management | 🔄 | ~600 | ~470 |
| 06 | Config / Constants / Exceptions | 🔄 | ~1,000 | ~640 |
| 07 | LLM Integration | 🔄 | ~500 | ~350 |
| 08 | Auth / OAuth | ✅ | ~800 | ~570 |
| 09 | UI / Shell | 🔄 | ~3,500 | ~920 |
| 10 | Web / Visualizer | ❌ | ~1,500 | ~0 |
| 11 | Background Tasks | 🔄 | ~800 | ~270 |
| 12 | Subagents | ✅ | ~1,200 | ~970 |
| 13 | Hooks | ✅ | ~400 | ~1,100 |
| 14 | MCP Client | ✅ | ~800 | ~440 |
| 14 | ACP Server | ❌ | ~800 | ~0 |
| 15 | Utils | 🔄 | ~2,500 | ~80 |
| 16 | Telemetry | ✅ | ~500 | ~950 |
| 16 | Notifications | 🔄 | ~500 | ~500 |
| 16 | Plugins | ✅ | ~400 | ~180 |
| 16 | Skills | 🔄 | ~300 | ~180 |

## File Mapping Index

| Python File | Rust File | Tracker |
|-------------|-----------|---------|
| `kimi_cli/soul/slash.py` | `octopus-cli/src/soul/slash.rs` | [02_soul.md](02_soul.md) |
| `kimi_cli/soul/mod.py` (`__init__.py`) | `octopus-cli/src/soul/mod.rs` | [02_soul.md](02_soul.md) |
| `kimi_cli/soul/kimisoul.py` | `octopus-cli/src/soul/mod.rs` | [02_soul.md](02_soul.md) |
| `kimi_cli/utils/slashcmd.py` | `octopus-cli/src/soul/slash.rs` | [02_soul.md](02_soul.md) |
| `kimi_cli/session_fork.py` | `octopus-cli/src/soul/slash.rs` | [02_soul.md](02_soul.md) |
| `kimi_cli/wire/types.py` | `octopus-cli/src/wire/mod.rs` | [04_wire.md](04_wire.md) |
| `kimi_cli/session.py` | `octopus-cli/src/session.rs` | [05_session.md](05_session.md) |
| `kimi_cli/session_state.py` | `octopus-cli/src/session_state.rs` | [05_session.md](05_session.md) |
| `kimi_cli/config.py` | `octopus-cli/src/config.rs` | [06_config.md](06_config.md) |
| `kimi_cli/exception.py` | `octopus-cli/src/exception.rs` | [06_config.md](06_config.md) |
| `kimi_cli/constant.py` | `octopus-cli/src/constant.rs` | [06_config.md](06_config.md) |
| `kimi_cli/llm.py` | `octopus-cli/src/llm.rs` | [07_llm.md](07_llm.md) |
| `kimi_cli/tools/file/*.py` | `octopus-cli/src/tools/file/mod.rs` | [03_tools.md](03_tools.md) |
| `kimi_cli/tools/shell/*.py` | `octopus-cli/src/tools/shell/mod.rs` | [03_tools.md](03_tools.md) |
| `kimi_cli/tools/web/*.py` | `octopus-cli/src/tools/web/mod.rs` | [03_tools.md](03_tools.md) |
| `kimi_cli/tools/ask_user/*.py` | `octopus-cli/src/tools/ask_user/mod.rs` | [03_tools.md](03_tools.md) |
| `kimi_cli/tools/todo/*.py` | `octopus-cli/src/tools/todo/mod.rs` | [03_tools.md](03_tools.md) |
| `kimi_cli/tools/think/*.py` | `octopus-cli/src/tools/think/mod.rs` | [03_tools.md](03_tools.md) |
| `kimi_cli/tools/plan/*.py` | `octopus-cli/src/tools/plan/mod.rs` | [03_tools.md](03_tools.md) |
| `kimi_cli/tools/agent/*.py` | `octopus-cli/src/tools/agent/mod.rs` | [03_tools.md](03_tools.md) |
| `kimi_cli/tools/background/*.py` | `octopus-cli/src/tools/background/mod.rs` | [03_tools.md](03_tools.md) |
| `kimi_cli/ui/shell/slash.py` | `octopus-cli/src/soul/slash.rs` (soul-level) | [02_soul.md](02_soul.md) |
| `kimi_cli/ui/shell/*.py` | `octopus-cli/src/ui/shell/mod.rs` | [09_ui_shell.md](09_ui_shell.md) |

## Cross-Cutting Initiatives

| Initiative | Tracker | Status |
|------------|---------|--------|
| Brain crate extraction & QQ integration | [`brain/index.md`](brain/index.md) | ✅ Phase 2 (octopus-cli consumer) and Phase 3 (qqbot-core integration) complete; Phase 4 (config/auth unification) next |

## How to Update This Tracker

1. After porting a module, update the corresponding `XX_*.md` file.
2. Change status symbols (⬜ → 🔄 → ✅).
3. Add notes about divergence from the Python original.
4. Update `index.md` summary table if the status changes.
