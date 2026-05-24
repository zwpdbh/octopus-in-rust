# 08 — Auth / OAuth

## Status: ✅ Complete

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/auth/__init__.py` | Auth package | ~50 |
| `kimi_cli/auth/oauth.py` | OAuth flow, token refresh | ~400 |
| `kimi_cli/auth/platforms.py` | Platform definitions, model refresh | ~300 |
| `kimi_cli/mcp_oauth.py` | MCP-specific OAuth | ~100 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/auth/mod.rs` | OAuthManager, token cache, platform registry | ~251 | ✅ |
| `octopus-cli/src/auth/oauth.rs` | OAuth 2.0 device flow + refresh | ~320 | ✅ |
| `octopus-cli/src/auth/platforms.rs` | Platform definitions | ~75 | ✅ |

## What's Done

- [x] OAuth 2.0 device flow (`login_kimi_code()`)
- [x] Token refresh with retry + 401 recovery
- [x] Platform provider registry (`Platform`, `PLATFORMS`)
- [x] API key resolution (`resolve_api_key()`)
- [x] Atomic file storage (`0o600`)
- [x] `/login` and `/logout` CLI commands
- [x] Tombstone cooldown for rejected refresh tokens

## What's Missing

- [ ] MCP-specific OAuth (deferred)
- [ ] Managed model refresh (deferred)
