# 14 — ACP / MCP

## Status: ⬜ Not Started

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/acp/__init__.py` | ACP package | ~50 |
| `kimi_cli/acp/server.py` | ACP server | ~200 |
| `kimi_cli/acp/tools.py` | ACP tool exposure | ~100 |
| `kimi_cli/acp/types.py` | ACP types | ~100 |
| `kimi_cli/acp/mcp.py` | MCP client | ~200 |
| `kimi_cli/acp/session.py` | ACP session | ~100 |
| `kimi_cli/acp/convert.py` | Type conversion | ~100 |
| `kimi_cli/acp/kaos.py` | Kaos path integration | ~100 |
| `kimi_cli/acp/version.py` | Version negotiation | ~50 |
| `kimi_cli/cli/mcp.py` | `kimi mcp` CLI command | ~100 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/acp/mod.rs` | ACP module placeholder | ~? | ⬜ Empty / stub |

## What's Missing

- [ ] MCP client (stdio / SSE transport)
- [ ] MCP server discovery
- [ ] Tool exposure via ACP
- [ ] Session management for MCP
- [ ] Type conversion (MCP <-> internal)
- [ ] `/mcp` full interactive status
- [ ] `kimi mcp` CLI subcommand
