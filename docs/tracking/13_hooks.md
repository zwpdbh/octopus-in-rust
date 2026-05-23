# 13 — Hooks

## Status: 🔄 Partial

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/hooks/__init__.py` | Hooks package | ~50 |
| `kimi_cli/hooks/engine.py` | Hook matching engine | ~200 |
| `kimi_cli/hooks/config.py` | Hook config schema | ~100 |
| `kimi_cli/hooks/events.py` | Event types | ~50 |
| `kimi_cli/hooks/runner.py` | Hook command runner | ~100 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/hooks/mod.rs` | Hooks module placeholder | ~? | ⬜ Empty / stub |
| `octopus-cli/src/config.rs` | Hook config in Config struct | ~427 | 🔄 Config parsed |

## What's Done

- [x] Hook config parsing in `Config` (TOML `[[hooks]]` tables)

## What's Missing

- [ ] Hook engine (event matching, glob patterns)
- [ ] Hook runner (subprocess execution)
- [ ] Event types (`UserPromptSubmit`, `ToolCall`, etc.)
- [ ] `/hooks` full implementation
- [ ] Wire-side hook delivery
