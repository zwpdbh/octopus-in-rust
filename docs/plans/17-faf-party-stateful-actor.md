# Plan: FAF Party as a Stateful WASM Actor

## Motivation

The current `faf-party` feature is split awkwardly:

- The **WASM plugin** (`plugins/faf-party`) only parses text.
- The **host service** (`apps/qqbot-core/src/faf_party.rs`) owns all state, timers, OneBot actions, and LLM fallback.

This means a group-specific feature is hard-coded inside `qqbot-core`. We want to move the *policy* (when to notify, what to say, how to reconcile nicknames) into the plugin, while the host provides generic capabilities (state, scheduling, messaging, LLM). The plugin becomes a **stateful actor**.

## Goal

Refactor `faf-party` so that:

1. The WASM plugin keeps its own per-group state.
2. The host persists that state and gives the plugin callbacks for scheduling, OneBot actions, and LLM calls.
3. `qqbot-core` shrinks to a thin capability runtime; the FAF-specific logic lives in `plugins/faf-party`.
4. The architecture becomes reusable for future stateful group features.

## High-level architecture

```text
┌─────────────────────────────────────────────────────────────────────┐
│                           qqbot-core                                 │
│  ┌─────────────────────┐    ┌─────────────────────────────────────┐ │
│  │ group_brain         │───▶│ Host capability runtime              │ │
│  │ (per-group Brain)   │    │ - load / save plugin state           │ │
│  └─────────────────────┘    │ - dispatch scheduled plugin wakeups  │ │
│                             │ - send OneBot actions                │ │
│                             │ - call LLM on plugin request         │ │
│                             └─────────────────────────────────────┘ │
│                                              │                       │
│                              call + callbacks│                       │
│                                              ▼                       │
│                             ┌─────────────────────────────────────┐ │
│                             │ faf-party WASM actor                 │ │
│                             │ - holds candidate list               │ │
│                             │ - decides join/leave/notify          │ │
│                             │ - formats messages                   │ │
│                             └─────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

## Plugin lifecycle

The host loads one plugin instance per group. The plugin exports:

```rust
// Called once when the group is initialized.
pub fn init(input: String) -> FnResult<String>;

// Called for every addressed group message.
pub fn on_message(input: String) -> FnResult<String>;

// Called when a scheduled timer fires.
pub fn on_timer(input: String) -> FnResult<String>;
```

Each input/output is JSON. The plugin can return a list of **host requests**:

```json
{
  "state": "<opaque plugin state blob>",
  "requests": [
    {"kind": "send_group_msg", "text": "..."},
    {"kind": "schedule_timer", "at": "2026-06-18T19:30:00+08:00", "payload": "retry_1"},
    {"kind": "llm_parse", "expression": "...", "now": "..."}
  ]
}
```

The host:

1. Saves `state` to `data/qqbot-data/faf-party-<group_id>.json`.
2. Executes each request.
3. Sends any responses back via the next `on_message` / `on_timer` call.

## Host capability API exposed to the plugin

The host registers Extism `Function`s that the WASM module imports:

| Import | Purpose |
|---|---|
| `host_state_load()` | Returns the last persisted state blob for this group. |
| `host_state_save(state: &str)` | Persists the state blob atomically. |
| `host_send_group_msg(text: &str)` | Sends a plain group message. |
| `host_send_group_msg_with_mentions(user_ids_json: &str, text: &str)` | Sends a group message with `@` segments. |
| `host_schedule_timer(when_rfc3339: &str, payload: &str)` | Schedules a future `on_timer(payload)` call. |
| `host_llm_parse(expression: &str, now_rfc3339: &str)` | Calls the configured LLM and returns parsed JSON. |
| `host_get_member_nickname(user_id: i64)` | Returns the cached group nickname, if known. |

All imports are synchronous from the plugin’s point of view. The host buffers async work (timers, LLM calls) and feeds results back later.

## State ownership

- The plugin **owns the schema and contents** of its state.
- The host **owns persistence and concurrency**: it wraps `state_save` with the per-group mutex already used by `PartyStateStore`.
- The state file remains `data/qqbot-data/faf-party-<group_id>.json`, but it now stores the plugin’s opaque blob plus a small envelope:

```json
{
  "version": 2,
  "blob": "<base64 or JSON-stringified plugin state>"
}
```

This lets the plugin evolve its internal schema without host changes.

## Message flow example

User: `@FatBot parton 晚上7点半之后可以玩`

1. Host calls `on_message` with:
   ```json
   {
     "user_id": 123,
     "sender_nickname": "zwpdbh",
     "text": "parton 晚上7点半之后可以玩",
     "now": "2026-06-18T16:04:00+08:00"
   }
   ```
2. Plugin parses intent (`join`), extracts nickname `parton`, parses time (`19:30`–`22:00`).
3. Plugin updates its candidate list.
4. Plugin returns:
   ```json
   {
     "state": "{...}",
     "requests": [
       {"kind": "send_group_msg", "text": "已登记 parton。\n当前 party 名单（共 1 人）：\n1. parton — 19:30 - 22:00"}
     ]
   }
   ```
5. Host saves state and sends the message. No LLM turn is started.

## Timer / notification flow

When the plugin detects 6 overlapping candidates:

1. Plugin returns `schedule_timer` requests for `+0s`, `+30s`, `+60s` (or one recurring timer).
2. Host fires `on_timer` at each time with a payload like `{"phase": "notify", "attempt": 1}`.
3. Plugin decides whether to send a notification, a reminder, or final call, and whether to clear the list.

## LLM fallback

When rule-based parsing fails, the plugin calls `host_llm_parse`. The host:

1. Runs the same provider/model configured for the group Brain.
2. Returns the LLM JSON directly to the plugin.
3. The plugin decides whether to trust the result or ask the user for clarification.

## GroupBrain integration

`group_brain` no longer contains FAF-specific code. Instead:

- Each group config lists `host_extensions = ["faf_party"]`.
- `GroupBrainManager` loads the configured extension modules at startup.
- For each addressed message, it calls `extension.on_message(...)` in order.
- If any extension returns `handled: true`, the normal LLM turn is skipped.
- Host tools exposed by extensions are registered with the group Brain (e.g. `faf_party_status`).

## Migration steps

1. **Define the host capability trait in `qqbot-core`**
   - `GroupHostExtension` trait with `init`, `on_message`, `on_timer`, `host_tools`.
   - `ExtensionContext` giving access to `action_tx`, state directory, LLM config, nickname cache.

2. **Build the capability bridge for WASM**
   - Extism `Function` imports for state, messaging, timers, LLM.
   - A small runtime that maps plugin requests to host actions and feeds async results back.

3. **Rewrite `plugins/faf-party`**
   - Export `init`, `on_message`, `on_timer`.
   - Move candidate list, overlap logic, notification formatting into the plugin.
   - Call host imports instead of using host service logic.

4. **Replace `FafPartyHostService`**
   - Delete `apps/qqbot-core/src/faf_party.rs`.
   - Add `apps/qqbot-core/src/extensions/mod.rs` and `apps/qqbot-core/src/extensions/faf_party.rs` that only implements the bridge.

5. **Update `GroupBrainManager`**
   - Load extensions from group config.
   - Call them before the LLM turn and register their host tools.

6. **Update build / deploy**
   - Keep `faf_party_plugin.wasm` in the release package.
   - Update docs and group config schema.

7. **Tests**
   - Unit-test the plugin actor with a mock host.
   - Integration-test the bridge end-to-end.

## Risks and open questions

1. **Async in Extism host functions** — timers and LLM are async; we need a clean way to return results to the plugin without blocking the guest.
2. **Plugin crash / bad state** — if a plugin returns invalid state, the host must log and fall back to an empty state, not crash `qqbot-core`.
3. **Versioning** — the opaque state blob needs a version field so plugin upgrades can migrate old state.
4. **One plugin per group vs. shared instance** — per-group instances are simpler; shared instances need group_id in every call.
5. **Should extensions be in group config or global config?** Per-group is more flexible but requires each group to opt in.

## Success criteria

- `faf-party` logic is entirely inside `plugins/faf-party`.
- `qqbot-core` has no FAF-specific code.
- A second stateful feature could be added by writing a new plugin and adding it to `host_extensions`.
- Registration, listing, and 6-player notifications still work after the refactor.
