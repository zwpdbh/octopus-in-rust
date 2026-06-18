# Plan: FAF Party Scheduling Plugin

## Goal

Add a `faf-party` WASM plugin plus host-side scheduling logic to the QQ bot so it can organize FAF (Forged Alliance Forever) game sessions from group chat messages.

The bot will:

1. Detect when a user wants to join/leave a FAF party.
2. Parse the user's availability into a structured time range.
3. Maintain a per-group candidate list.
4. When enough candidates for a 3v3 match (6 players) overlap in time, notify the group.
5. Retry the notification up to 3 times (immediately, +30s, +60s) in case players are slow to respond.

## User decisions

| Question | Decision |
|---|---|
| Where does parsing live? | In a WASM plugin `plugins/faf-party` for sandboxing and hot-reload. |
| Where does state/scheduling live? | In `qqbot-core` host service, because plugins cannot run timers or send messages. |
| Storage format | JSON file per group: `data/qqbot-data/faf-party-<group_id>.json`. |
| Fixed daily end time | 22:00 (10 PM). Every availability range extends to 22:00 unless the user gives an explicit end. |
| Past-time handling | If a parsed start time is already in the past, schedule it for the same wall-clock time **tomorrow**. |
| Party size trigger | 6 candidates for a 3v3 match. |
| Notification retries | 3 times: immediately, +30s, +60s. After the 3rd retry the candidate list is cleared. |
| Leave detection | Phrases: "不玩了", "cancel", "退出", "leave", and command `/faf leave`. |

## Current state

- `qqbot-core` runs one `Brain` per allowed group and dispatches `@`-triggered messages to WASM plugins.
- Existing plugin `faf-units` is stateless; data is embedded JSON in the WASM binary.
- No host-side persistent state service exists for plugins.
- Plugins can request filesystem access via `allowed_paths` in the manifest, but they are instantiated per call and cannot run background timers.

## Target architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│                         qqbot-core                               │
│  ┌─────────────────────┐    ┌───────────────────────────────┐  │
│  │ group_brain         │───▶│ FafPartyHostService            │  │
│  │ (per-group Brain)   │    │ - read/write JSON state        │  │
│  └─────────────────────┘    │ - run retry timers             │  │
│           │                 │ - send OneBot group messages   │  │
│           │ calls           └───────────────────────────────┘  │
│           ▼                                                      │
│  ┌─────────────────────┐                                        │
│  │ faf-party plugin    │                                        │
│  │ - parse intent      │                                        │
│  │ - parse availability│                                        │
│  └─────────────────────┘                                        │
└─────────────────────────────────────────────────────────────────┘
```

### Plugin tools (`faf-party`)

1. **`faf_party_parse_availability`**
   - Input: `{ "message": "我大约半个小时后可以玩", "now": "2026-06-18T13:00:00+08:00" }`
   - Output: `{ "start": "...", "end": "...", "description": "半个小时后到晚上10点" }`
   - Pure computation, no side effects.

2. **`faf_party_process_message`**
   - Input: `{ "user_id": 123456, "message": "...", "now": "...", "group_id": 136430130 }`
   - Output: `{ "intent": "join|leave|unknown", "availability": { "start": "...", "end": "..." }, "candidates": [...], "ready": false, "notification": null }`
   - Reads/writes the group state file via `allowed_paths`.

### Host service (`qqbot-core`)

- `FafPartyHostService` is created once per `GroupBrainManager`.
- After each addressed message, `group_brain` calls the plugin and then asks the host service to:
  - Update candidate list
  - Check for overlap among candidates
  - Schedule notification retries when ready
- Background `tokio` tasks handle the +30s and +60s retries.

### State file format

`data/qqbot-data/faf-party-136430130.json`:

```json
{
  "candidates": [
    { "user_id": 123456, "start": "2026-06-18T13:30:00+08:00", "end": "2026-06-18T22:00:00+08:00", "joined_at": "2026-06-18T13:00:00+08:00" }
  ],
  "notification_count": 0,
  "last_notification_at": null
}
```

## Implementation steps

1. **Create plugin skeleton**
   - `plugins/faf-party/Cargo.toml`
   - `plugins/faf-party/faf_party_plugin.json`
   - `plugins/faf-party/src/lib.rs`

2. **Implement availability parser**
   - Support expressions:
     - "现在", "马上" → now
     - "半小时后", "30分钟后", "半个小时后" → now + 30min
     - "一小时后", "一个小时后" → now + 60min
     - "两小时后" → now + 120min
     - "今晚", "今天晚上" → today 18:00
     - "明天晚上" → tomorrow 18:00
     - "8点", "20点", "晚上8点" → today 20:00
     - "9点到10点" → today 21:00–22:00
   - End time defaults to 22:00.
   - Past times roll forward to tomorrow.

3. **Implement plugin state management**
   - Read/write `data/qqbot-data/faf-party-<group_id>.json` via `allowed_paths`.
   - Handle `join`, `leave`, `unknown` intents.
   - Detect leave phrases and `/faf leave`.

4. **Add host service in `qqbot-core`**
   - New module `apps/qqbot-core/src/faf_party.rs`.
   - Schedule retry timers.
   - Send notifications via `action_tx`.

5. **Wire into `group_brain`**
   - Call `faf_party_process_message` for every addressed message.
   - If `ready`, pass result to `FafPartyHostService` for notification scheduling.

6. **Build and deploy integration**
   - Add plugin to `scripts/build-qqbot-release.sh`.
   - Enable in group configs.
   - Update system prompt to mention FAF party scheduling.

## Open questions

1. Should the bot also respond to **non-`@` messages** that mention FAF? Currently it only processes addressed messages.
2. Should candidates be **removed automatically** if their availability window expires?
3. Should the host service support other game modes (e.g., 4v4 needs 8 players) or is 3v3 enough?
4. Should the notification include `@` mentions for each player, or just list names?

## Success criteria

- `faf_party_parse_availability` correctly converts "我大约半个小时后可以玩" into a time range.
- Six joining players trigger exactly 3 notifications at 0s, +30s, and +60s.
- `leave` phrases and `/faf leave` remove the user from the candidate list.
- State persists across `qqbot-core` restarts.
