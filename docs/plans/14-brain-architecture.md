# Plan: Extract a Reusable Brain Crate

## Goal

Turn the agent core currently embedded in `octopus-cli` into a reusable, UI-agnostic **Brain** crate. The Brain will power both:

1. `octopus-cli` — the TUI/terminal coding agent.
2. `qqbot-core` — the QQ group-bot daemon.

Both products share the same reasoning, tool-calling, and LLM-provider logic, but each supplies its own input adapter, tool policy, and output renderer.

## User decisions

| Question | Decision |
|---|---|
| Where does the Brain live? | New crate extracted from `octopus-cli`; `octopus-cli` is updated to consume it. |
| Brain lifetime in qqbot | One long-lived Brain instance per allowed group. |
| Tool policy | All tools are available by default. Users can exclude tools via configuration later. |
| Approval model | Auto-approve all tool calls by default. |
| Streaming | Supported in TUI (`octopus-cli`). In QQ, the Brain runs to completion; `qqbot-core` sends an intermediate "processing..." message, then the final message. |

## Current state

- `octopus-cli` contains the full agent loop in `src/soul/kimisoul.rs` and surrounding modules.
- `kosong` provides LLM-provider abstraction and tool-call plumbing, but not session/agent orchestration.
- `qqbot-core` dispatches OneBot events to WASM plugins and calls a simple `LlmClient` for one-shot completions.
- The `summary` plugin builds a prompt and asks for a single summary; there is no reasoning loop or tool use.

## Target architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│                          octopus-cli                             │
│  TUI / Shell / Print / Wire / ACP  →  Brain  →  Kimi providers  │
└─────────────────────────────────────────────────────────────────┘
                              ▲
                              │ uses
┌─────────────────────────────────────────────────────────────────┐
│                          Brain crate                             │
│  Session · Agent loop · Tool registry · Approval policy · Events │
└─────────────────────────────────────────────────────────────────┘
                              ▲
                              │ uses
┌─────────────────────────────────────────────────────────────────┐
│                          qqbot-core                              │
│  OneBot events  →  Brain per group  →  OneBot group messages    │
└─────────────────────────────────────────────────────────────────┘
```

### What moves into the Brain crate

- Agent turn loop (`KimiSoul` core logic).
- Session/context management.
- Tool registry and dispatch.
- Approval-policy abstraction (with a default auto-approver).
- Streaming/non-streaming response handling.
- Event/output abstraction: `TextPart`, `ThinkingPart`, `ToolCall`, `ToolResult`, `TurnBegin`, `TurnEnd`, `Error`.
- LLM provider resolution and OAuth credential lookup.

### What stays in `octopus-cli`

- UI rendering and input handling.
- Interactive approval dialogs.
- Work-dir detection, shell environment, clipboard, theme.
- Wire/ACP server adapters.
- Skills/MCP integration as additional tool sources.

### What stays in `qqbot-core`

- OneBot WebSocket connection and event parsing.
- Per-group Brain lifecycle.
- Mapping Brain events to QQ messages.
- Two-phase QQ messaging ("processing..." then final reply).
- Plugin loading / hot-reload for non-agent features.

## Brain public API (sketch)

```rust
// docs/plans/14-brain-architecture.md — conceptual API sketch
pub struct Brain {
    session: Session,
    tool_registry: ToolRegistry,
    approval: Arc<dyn ApprovalPolicy>,
    llm: Arc<dyn ChatProvider>,
}

impl Brain {
    pub fn new(config: BrainConfig) -> Self;

    /// Run one user turn and return events as a stream.
    pub async fn run_turn(
        &mut self,
        input: TurnInput,
    ) -> Result<Pin<Box<dyn Stream<Item = BrainEvent> + Send>>>;

    /// Synchronous, non-streaming convenience for hosts that need a final result.
    pub async fn run_turn_to_completion(&mut self, input: TurnInput) -> Result<TurnResult>;
}

pub enum BrainEvent {
    TextPart(String),
    ThinkingPart(String),
    ToolCall { id: String, name: String, arguments: Value },
    ToolResult { id: String, output: ToolOutput },
    TurnBegin,
    TurnEnd,
    Error(String),
}
```

The exact shape will be finalized during extraction, but the principle is: **the Brain emits events, not terminal output.**

## qqbot integration details

### One Brain per group

- When `qqbot-core` starts, it creates one `Brain` for each configured allowed group.
- Group messages are appended to that group's session history.
- Commands such as `/summary` become user input to the Brain; the Brain decides whether to call a tool or answer directly.

### Two-phase messaging

Because OneBot/QQ does not support streaming text, `qqbot-core` will:

1. On command receipt, send a short processing indicator to the group, e.g.:
   > 🤔 Summarizing recent messages...
2. Run the Brain turn to completion.
3. Send the final reply as a single group message.

The processing message can include the request id / operation label so the final reply can refer back to it.

### Tool policy

- Default: all tools available, auto-approved.
- Later: configuration section `[bot.tools]` with `allow`/`deny` lists and per-tool approval settings.
- Dangerous tools (Shell, WriteFile, StrReplaceFile) should be considered for a stricter default, but per the current decision they remain enabled.

### Message boundaries

- A group chat is a single session.
- Only messages from allowed groups are processed.
- Self-messages are ignored unless the OneBot bridge reports them and the command is meant for testing.

## Configuration

The Brain reads the same model/provider config that `octopus-cli` uses:

```toml
[llm]
api_url = "https://api.kimi.com/coding/v1/chat/completions"
model = "kimi-for-coding"
system_prompt = "You are a helpful QQ group assistant."

[llm.oauth]
provider = "kimi-code"
token_file = "~/.kimi/credentials/kimi-code.json"
```

`qqbot-core` can share `~/.kimi/config.toml` or keep its own file; the plan is to reuse the credential store so Kimi Code quota works the same way as in `octopus-cli`.

## Implementation phases

### Phase 1 — Extract the Brain crate

1. Create `crates/brain/` (or reuse `kosong` if appropriate).
2. Move agent-loop logic out of `octopus-cli/src/soul` into `brain/src/agent.rs`.
3. Move session/context types into `brain/src/session.rs`.
4. Define `BrainEvent` and `Brain` API.
5. Keep `octopus-cli` compiling by re-exporting and wrapping the new crate.

### Phase 2 — Make `octopus-cli` a consumer

1. Replace inline `KimiSoul` agent loop with `Brain::run_turn`.
2. Map Brain events to existing UI render paths.
3. Preserve interactive approval in TUI by implementing `ApprovalPolicy`.
4. Verify `cargo test -p octopus-cli` still passes.

### Phase 3 — Integrate Brain into `qqbot-core`

1. Add `brain` dependency to `qqbot-core`.
2. Create a `GroupBrain` manager keyed by `group_id`.
3. On `/summary` or any command, send the processing indicator, run the Brain turn, and post the final reply.
4. Provide a QQ-safe toolset by default.
5. Remove or demote the simple `LlmClient` summary plugin path.

### Phase 4 — Configuration and auth unification

1. Let `qqbot-core` read the same provider/model config as `octopus-cli`.
2. Ensure OAuth token refresh works in daemon context.
3. Document how `qqbot init` can bootstrap from an existing `~/.kimi/config.toml`.

## Success criteria

- `cargo check --workspace` passes.
- `cargo test -p octopus-cli` passes with no regressions.
- `octopus-cli --print` still produces answers.
- `qqbot-core` can answer `/summary` using the Brain and Kimi Code quota.
- A `/status` command in the group returns a meaningful status without crashing.

## Risks and open questions

1. **Coupling to `octopus-cli` internals.** `KimiSoul` references UI, notifications, background tasks, and subagents. Extraction must break those dependencies cleanly.
2. **Approval in daemon mode.** Auto-approval is acceptable now, but long-term we need a way to audit/restrict tool calls in a group setting.
3. **Tool schemas for Kimi Code API.** The API rejected some tool schemas (`$ref` not supported). The Brain must normalize tool definitions before sending them.
4. **Streaming vs non-streaming.** The Brain should support both; the QQ adapter consumes the non-streaming path.
5. **Group session memory.** Keeping one Brain per group means memory usage grows with the number of groups. We may need compaction later.

## Related documents

- [`STATUS.md`](../../STATUS.md) — project status and active task tracking.
- [`docs/plans/00-index.md`](./00-index.md) — plans index.
- [`docs/plans/13-feature-checklist.md`](./13-feature-checklist.md) — feature tracker.
- [`AGENTS.md`](../../AGENTS.md) — project coding conventions.
