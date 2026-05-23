# 08 — Auth / OAuth

## Status: ⬜ Not Started

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
| `octopus-cli/src/auth/mod.rs` | Auth module placeholder | ~? | ⬜ Empty / stub |

## What's Missing

- [ ] OAuth 2.0 device flow
- [ ] Token refresh logic
- [ ] Platform provider registry
- [ ] API key resolution
- [ ] Managed model refresh
- [ ] `/login` command
