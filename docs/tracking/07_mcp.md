# Phase 7: MCP Integration

## Status: COMPLETE (scaffolding + architecture)

## Implementation

### `mcp/mod.rs` — MCP config + types

- **`McpConfig`** — top-level config with `mcpServers` map
- **`McpServerConfig`** — per-server config: transport (stdio/http/sse), command, args, env, url, headers, auth
- **`load_mcp_config()` / `save_mcp_config()`** — read/write `~/.kimi/mcp.json`
- **`McpServerInfo`** — runtime state: status (`Pending`/`Connecting`/`Connected`/`Failed`/`Unauthorized`) + tools list
- **`McpToolInfo`** — name, description, schema for each exposed tool

### `soul/toolset.rs` — MCP integration in `KimiToolset`

- **MCP state fields**: `mcp_servers: HashMap<String, McpServerInfo>`, `deferred_mcp_load`, `mcp_loading_task`
- **`defer_mcp_tool_loading()`** — store configs for later background startup
- **`start_deferred_mcp_tool_loading()`** — spawn background task to connect servers
- **`wait_for_mcp_tools()`** — await background task completion
- **`has_pending_mcp_tools()`** — check if background task is still running
- **`mcp_status_snapshot()`** — real implementation returning `MCPStatusSnapshot` with loading/connected/total/tools/servers
- **`load_mcp_tools()`** — background task that sets up server entries (connection is stubbed)
- **`cleanup_mcp()`** — abort task, clear state
- **`register_external_tool()`** — register `WireExternalTool` by name/description/parameters
- **`WireExternalTool`** — `Tool` implementation that delegates to wire protocol
- **`MCPTool`** — `Tool` stub with schema; `call()` returns "not yet implemented"

### `soul/mod.rs` — MCP loading in turn lifecycle

- In `_agent_loop()`, before step loop:
  1. `start_deferred_mcp_tool_loading()`
  2. If loading: emit `StatusUpdate(mcp_status=...)` + `MCPLoadingBegin`
  3. `wait_for_mcp_tools()`
  4. On completion: track `mcp_connected` / `mcp_failed`, emit `StatusUpdate` + `MCPLoadingEnd`

## Deferred Work

- **Real MCP client connection** — needs a stable Rust MCP client library
  - `rust-mcp-sdk` (v0.9.0) is available but adds significant complexity
  - `mcp-client` (v0.1.0) from modelcontextprotocol is simpler but very new
  - When integrated, replace the stub loop in `load_mcp_tools()` with actual `initialize()` + `list_tools()` + `call_tool()`
- **MCP OAuth** — token storage, authorization flow, refresh
- **MCP tool result conversion** — convert MCP `CallToolResult` to `ToolReturnValue` with budget enforcement

## Architecture

```
┌─────────────┐     defer     ┌─────────────────┐
│  load_agent │ ─────────────►│ KimiToolset     │
│  (Phase 11) │               │ deferred_mcp_load│
└─────────────┘               └────────┬────────┘
                                       │ start_deferred_mcp_tool_loading()
                                       ▼
                               ┌───────────────┐
                               │ tokio::spawn  │
                               │ _connect()    │
                               └───────┬───────┘
                                       │ (stubbed)
                                       ▼
                               ┌───────────────┐
                               │ mcp_servers   │
                               │ (status +     │
                               │  tool list)   │
                               └───────────────┘
```
