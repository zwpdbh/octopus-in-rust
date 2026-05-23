# 06 — Config / Constants / Exceptions

## Status: 🔄 Partial

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/config.py` | TOML config loading, validation | ~600 |
| `kimi_cli/constant.py` | Version, paths, constants | ~100 |
| `kimi_cli/exception.py` | Custom exceptions | ~200 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/config.rs` | Config struct, TOML serde | ~427 | 🔄 Basic config |
| `octopus-cli/src/constant.rs` | Version, build info | ~? | ✅ |
| `octopus-cli/src/exception.rs` | Error types with `thiserror` | ~173 | 🔄 |

## What's Done

- [x] `Config` struct with serde deserialization
- [x] `default_model`, `providers`, `hooks`, `theme`
- [x] `workspace_dirs`, `default_editor`
- [x] `OctopusError` enum with `thiserror`
- [x] `LLMNotSet`, `LLMNotSupported`, `MaxStepsReached`
- [x] Version constant

## What's Missing

- [ ] Config validation (provider credentials, model names)
- [ ] Config migration / backward compatibility
- [ ] `is_from_default_location` tracking
- [ ] `loop_control` full validation
- [ ] Rich error messages with suggestions
