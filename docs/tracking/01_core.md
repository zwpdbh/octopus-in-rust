# 01 — Core / CLI Entry

## Status: 🔄 Partial

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/__init__.py` | Package init | ~30 |
| `kimi_cli/__main__.py` | Entrypoint shim | ~10 |
| `kimi_cli/cli/__init__.py` | CLI command definitions (click/lazy loading) | ~400 |
| `kimi_cli/cli/__main__.py` | CLI entry | ~20 |
| `kimi_cli/app.py` | Main application / runtime wiring | ~350 |
| `kimi_cli/metadata.py` | Package metadata | ~100 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/main.rs` | Binary entrypoint, arg parsing | ~410 | 🔄 Basic CLI args exist; missing lazy loading |
| `octopus-cli/src/lib.rs` | Library root | ~60 | 🔄 Module declarations only |
| `octopus-cli/src/app.rs` | Main App struct / runtime wiring | ~232 | 🔄 Skeleton exists; missing reload/switch logic |
| `octopus-cli/src/metadata.rs` | Build-time metadata | ~106 | ✅ Version info ported |

## What's Done

- [x] CLI argument parsing (basic `--model`, `--session`, `--work-dir`)
- [x] `App` struct with `KimiSoul` lifecycle
- [x] Version metadata

## What's Missing

- [ ] Lazy command groups (`_lazy_group.py`)
- [ ] Export / info / mcp / plugin / toad / vis / web subcommands
- [ ] Shell mode vs agent mode toggling (`Ctrl-X`)
- [ ] `Reload` exception-based flow control
- [ ] `SwitchToWeb` / `SwitchToVis` exceptions
