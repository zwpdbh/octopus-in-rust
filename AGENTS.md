# Agent Guidelines for Octopus

## Rust Best Practices

### Model States with Enums and Match on Them

**Rule:**
1. Represent mutually exclusive states, statuses, and conditions as `enum` variants with associated data.
2. Branch on those enums with `match` (or `if let`). Avoid `if-else` chains built from boolean flags, string comparisons, or integer codes.

Never use scattered booleans, `String` constants, or raw integers to model status.

#### Why

- **Type safety:** Invalid states become unrepresentable.
- **Exhaustiveness:** `match` forces you to handle every variant. Adding a state becomes a compile error, not a silent bug.
- **Readability:** A `match` arm pairs the state with its behavior; `if-else` chains make the reader reconstruct the state matrix.

#### Bad

```rust
// Boolean flags — impossible states allowed
struct Order {
    is_active: bool,
    is_deleted: bool,
}

// String constants — typo-prone
struct Task {
    status: String, // "pending", "running", "done"
}

// Reconstructing state from scattered booleans
let mode = if cli.print { "print" } else if cli.acp { "acp" } else { "shell" };
if mode == "acp" { ... }

// Independent if chains for related conditions
if cli.agent.is_some() && cli.agent_file.is_some() { ... }
if cli.config.is_some() && cli.config_file.is_some() { ... }
```

#### Good

```rust
enum OrderStatus {
    Active { placed_at: DateTime<Utc> },
    Deleted { reason: String, deleted_at: DateTime<Utc> },
}

struct Order {
    id: Uuid,
    status: OrderStatus,
}

match order.status {
    OrderStatus::Active { placed_at } => { /* ... */ }
    OrderStatus::Deleted { reason, deleted_at } => { /* ... */ }
}
```

Model mutually exclusive CLI modes as an enum and branch exhaustively:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Shell,
    Print,
    Acp,
    Wire,
}

match ui_mode {
    UiMode::Print => instance.run_print(...).await,
    UiMode::Acp   => instance.run_acp().await,
    UiMode::Wire  => instance.run_wire_stdio().await,
    UiMode::Shell => instance.run_shell(...).await,
}
```

For multiple related boolean conditions, precompute and match on tuples:

```rust
let agent_conflict  = cli.agent.is_some() && cli.agent_file.is_some();
let config_conflict = cli.config.is_some() && cli.config_file.is_some();

match (agent_conflict, config_conflict) {
    (false, false) => {}
    (true, _) => { /* handle agent conflict */ }
    (_, true) => { /* handle config conflict */ }
}
```

#### Example: Config lookup with fallback priority

**Bad:** imperative mutation with overlapping `if` blocks and variable shadowing.

```rust
let mut model: Option<LLMModel> = None;
let mut provider: Option<LLMProvider> = None;

if model_name.is_none() && !config.default_model.is_empty() {
    if let Some(m) = config.models.get(&config.default_model) {
        model = Some(m.clone());
        provider = config.providers.get(&m.provider).cloned();
    }
}
if let Some(ref name) = model_name {
    if let Some(m) = config.models.get(name) {
        model = Some(m.clone());
        provider = config.providers.get(&m.provider).cloned();
    }
}

let mut model = model.unwrap_or_else(|| LLMModel { /* fallback */ });
let mut provider = provider.unwrap_or_else(|| LLMProvider { /* fallback */ });
```

**Good:** pre-compute existence booleans, then `match` exhaustively on the tuple.

```rust
let explicit = model_name.as_ref().and_then(|n| config.models.get(n));
let default = if config.default_model.is_empty() {
    None
} else {
    config.models.get(&config.default_model)
};

let name_given = model_name.is_some();
let name_exists = explicit.is_some();
let default_given = !config.default_model.is_empty();
let default_exists = default.is_some();

let (mut model, mut provider) = match (name_given, name_exists, default_given, default_exists) {
    // Explicit model requested and found in config.
    (true, true, _, _) => {
        let m = explicit.unwrap().clone();
        let p = config.providers.get(&m.provider).cloned().unwrap_or_else(|| LLMProvider { /* fallback */ });
        (m, p)
    }
    // No explicit model; default configured and found in config.
    (false, _, true, true) => {
        let m = default.unwrap().clone();
        let p = config.providers.get(&m.provider).cloned().unwrap_or_else(|| LLMProvider { /* fallback */ });
        (m, p)
    }
    // Everything else falls back to hard-coded defaults.
    _ => (LLMModel { /* fallback */ }, LLMProvider { /* fallback */ }),
};
```

This makes the scenario matrix explicit, eliminates shadowing, and guarantees every combination is handled.

Use `matches!` for one-off predicates:

```rust
if matches!(ui_mode, UiMode::Acp | UiMode::Wire) { /* ... */ }
```

#### Example: Replace `HashMap<String, Value>` payload builders with a sum type

**Bad:** Functions that construct dynamic maps for external consumers. The compiler cannot verify field names, types, or which fields belong to which event.

```rust
pub fn pre_tool_use(
    session_id: &str,
    cwd: &str,
    tool_name: &str,
    tool_input: &HashMap<String, Value>,
    tool_call_id: &str,
) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    m.insert("hook_event_name".to_string(), Value::String("PreToolUse".to_string()));
    m.insert("session_id".to_string(), Value::String(session_id.to_string()));
    m.insert("cwd".to_string(), Value::String(cwd.to_string()));
    m.insert("tool_name".to_string(), Value::String(tool_name.to_string()));
    m.insert("tool_input".to_string(), Value::Object(tool_input.clone().into_iter().collect()));
    m.insert("tool_call_id".to_string(), Value::String(tool_call_id.to_string()));
    m
}

pub fn post_tool_use_failure(
    session_id: &str,
    cwd: &str,
    tool_name: &str,
    tool_input: &HashMap<String, Value>,
    error: &str,
    tool_call_id: &str,
) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    m.insert("hook_event_name".to_string(), Value::String("PostToolUseFailure".to_string()));
    // ... repeated boilerplate for every event type
    m
}
```

Problems:
- Typos in field names are runtime errors.
- Nothing prevents passing a `PostToolUseFailure` payload to a `PreToolUse` trigger.
- Adding a new required field does not force updates at call sites.
- The compiler cannot tell you which fields are valid for which event.

**Good:** A single enum where each variant carries exactly the data it needs.

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "hook_event_name", rename_all = "PascalCase")]
pub enum HookEvent {
    PreToolUse {
        session_id: String,
        cwd: String,
        tool_name: String,
        tool_input: HashMap<String, Value>,
        tool_call_id: String,
    },
    PostToolUseFailure {
        session_id: String,
        cwd: String,
        tool_name: String,
        tool_input: HashMap<String, Value>,
        error: String,
        tool_call_id: String,
    },
    // ... every other event type
}
```

Construction is explicit and type-checked:

```rust
let event = HookEvent::PreToolUse {
    session_id: session_id.into(),
    cwd: cwd.into(),
    tool_name: tool_name.into(),
    tool_input: inputs.clone(),
    tool_call_id: call_id.into(),
};
```

- Adding a new field to `PreToolUse` is a compile error at every construction site until it is provided.
- A `PostToolUseFailure` payload cannot be accidentally passed where a `PreToolUse` is expected.
- `serde` generates the exact same JSON the old `HashMap` builders produced.

When the enum must also serve as a `HashMap` key (e.g., indexing registered hooks by event type), implement `Hash` and `Eq` on the discriminant only so the same type acts as both payload and dictionary key, eliminating the need for a parallel "kind" enum:

```rust
impl PartialEq for HookEvent {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Eq for HookEvent {}

impl std::hash::Hash for HookEvent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
    }
}
```

#### Migration Strategy

When refactoring existing code:

1. Define the `enum` covering all valid states.
2. Move associated data into the relevant variant.
3. Replace the old field(s) with a single `status: YourEnum` field.
4. Let the compiler guide you to fix all `match` or `if` sites.
5. Delete dead helper methods (`is_active()`, `set_pending()`, etc.).

#### Exception

Raw primitives are acceptable only at serialization boundaries or external API interop — convert to an enum immediately at the boundary.

---

### Deserialize JSON into Typed Structs, Not `serde_json::Value`

**Rule:** When parsing HTTP responses or JSON files, derive `Deserialize` on a struct and use `response.json::<T>().await` (or `serde_json::from_str`). Do not deserialize into `serde_json::Value` and then manually index fields with hardcoded strings.

Use `#[serde(default)]` and `#[serde(default = "...")]` for optional fields. Post-process after deserialization for computed values (e.g., converting `expires_in` to an absolute `expires_at` timestamp).

Reserve `serde_json::Value` for genuinely dynamic data (error payloads with unknown shape, truly schemaless JSON).

#### Why

- **Type safety:** A typo in a field name becomes a compile error, not a silent `None` at runtime.
- **Exhaustiveness:** Adding a new required field to the struct forces you to update every construction and deserialization site.
- **Readability:** `body.user_code` is self-documenting; `body["user_code"].as_str().unwrap_or("")` makes the reader guess the type and nullability of every field.
- **Refactoring safety:** Rename a field in the struct, and the compiler shows every call site. Rename a string literal, and `grep` is your only safety net.

#### Bad

```rust
let body: serde_json::Value = response.json().await?;

Ok(DeviceAuthorization {
    user_code: body["user_code"].as_str().unwrap_or("").to_string(),
    device_code: body["device_code"].as_str().unwrap_or("").to_string(),
    verification_uri: body["verification_uri"].as_str().unwrap_or("").to_string(),
    verification_uri_complete: body["verification_uri_complete"]
        .as_str()
        .unwrap_or("")
        .to_string(),
    expires_in: body["expires_in"].as_u64(),
    interval: body["interval"].as_u64().unwrap_or(5),
})
```

#### Good

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceAuthorization {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: Option<u64>,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

// One line. The compiler checks field names and types.
let body: DeviceAuthorization = response.json().await?;
Ok(body)
```

#### Computed fields: deserialize first, then transform

When the response contains raw data that must be enriched (e.g., relative `expires_in` → absolute `expires_at`), deserialize into the struct, then compute:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_at: f64,
    #[serde(default)]
    pub scope: String,
    #[serde(default = "default_bearer")]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: f64,
}

fn default_bearer() -> String {
    "Bearer".to_string()
}

fn parse_token_response(body: &serde_json::Value) -> Result<OAuthToken> {
    let mut token: OAuthToken = serde_json::from_value(body.clone())
        .map_err(|e| OctopusError::Other(format!("Invalid token response: {}", e)))?;

    if token.expires_in > 0.0 && token.expires_at == 0.0 {
        token.expires_at = now_secs() + token.expires_in;
    }

    Ok(token)
}
```

`#[serde(default)]` handles absent fields; the computation step handles derived data. No manual `Value` indexing anywhere.

#### Exception

`serde_json::Value` is acceptable only when:
1. The schema is truly unknown (e.g., arbitrary user-provided JSON).
2. You must inspect a small subset of fields before deciding which typed struct to parse into (e.g., reading `body["error"]` to check for an OAuth error before parsing a success response).

Even in case 2, convert to a typed struct as soon as the shape is known.

---

### Use Strong Enums for Channel and IPC Messages

**Rule:** When messages flow through channels (`tokio::sync::broadcast`, `mpsc`, etc.), use a single `enum` that lists every possible message type. Do not use `serde_json::Value`, `String`, or raw bytes as the common carrier type.

If JSON backward compatibility is required (e.g., log files), use `#[serde(untagged)]` on the enum so serialization stays identical while deserialization gains type safety.

#### Why

- **Exhaustiveness:** `match event` forces every consumer to acknowledge every message type. Adding a new variant becomes a compile error across all receivers, not a silent deserialization failure.
- **Discoverability:** A new developer can open the enum definition and immediately see the complete protocol. With `Value`, they must grep every `wire_send` call site to reconstruct the protocol.
- **Refactoring safety:** Rename a variant and the compiler shows every producer and consumer. Rename a JSON field string and you rely on runtime tests to catch missed sites.

#### Bad

```rust
pub struct Wire {
    raw_tx: broadcast::Sender<serde_json::Value>,
    merged_tx: broadcast::Sender<serde_json::Value>,
}

pub fn wire_send<T: Serialize>(event: T) {
    let value = serde_json::to_value(&event).unwrap();
    soul_side.send(value);
}

// Producer: any struct can be sent
wire_send(TextPart { text: "hello".to_string() });
wire_send(TurnBegin { user_input: Some("hi".to_string()) });

// Consumer: trial-and-error deserialization
if let Ok(req) = serde_json::from_value::<ApprovalRequestEvent>(value.clone()) {
    self.pending_approval = Some(req);
} else if let Ok(text) = serde_json::from_value::<TextPart>(value) {
    self.append_text(text.text);
}
```

#### Good

```rust
// File: octopus-cli/src/wire/types.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireEvent {
    TextPart(TextPart),
    TurnBegin(TurnBegin),
    TurnEnd(TurnEnd),
    StepBegin(StepBegin),
    StatusUpdate(StatusUpdate),
    ApprovalRequest(ApprovalRequestEvent),
    ApprovalResponse(ApprovalResponseEvent),
    // ... every message type in one place
}

pub struct Wire {
    raw_tx: broadcast::Sender<WireEvent>,
    merged_tx: broadcast::Sender<WireEvent>,
}

pub fn wire_send(event: WireEvent) {
    soul_side.send(event);
}

// Producer: variant is explicit
wire_send(WireEvent::TextPart(TextPart { text: "hello".to_string() }));
wire_send(WireEvent::TurnBegin(TurnBegin { user_input: Some("hi".to_string()) }));

// Consumer: exhaustive match
match event {
    WireEvent::ApprovalRequest(req) => self.pending_approval = Some(req),
    WireEvent::TextPart(text) => self.append_text(text.text),
    WireEvent::TurnEnd(_) => self.end_turn(),
    // Compiler forces you to handle every variant
}
```

#### Exception

Raw bytes or `Value` are acceptable only when:
1. The channel crosses a true process/network boundary where the peer is not Rust (e.g., WebSocket, gRPC).
2. The protocol is intentionally schemaless (e.g., plugin extensibility where third parties define message shapes).

Even then, deserialize to a strong enum at the earliest possible point inside your process.

---

### Keep `mod.rs` Thin — Index Modules Only

**Rule:** `mod.rs` contains **only** `pub mod` declarations and `pub use` re-exports. Logic lives in sibling files.

**Why:** The index says *what* is exposed; sibling files say *how* it works.

```rust
// Bad: 1,300-line mod.rs with mixed declarations + logic
pub mod toolset;
pub struct KimiSoul { /* ... */ }
impl KimiSoul { /* 800 lines */ }

// Good: mod.rs is an index
pub mod agent;
pub mod toolset;
mod kimisoul;
pub use kimisoul::KimiSoul;
```

**Migration:**
1. Move the dominant struct/impl to a sibling file (`kimisoul.rs`, `manager.rs`, `engine.rs`).
2. In `mod.rs`: `mod sibling_file; pub use sibling_file::MainItem;`
3. Let the compiler guide visibility fixes — promote private items to `pub(crate)` if other modules need them.

Callers stay unchanged because `mod.rs` re-exports.


---

### Avoid Ambiguous Duplicate Module File Names

**Rule:** Do not create two module files with the same base name in the same module hierarchy if they serve different purposes. When a parent module and its submodule both need a logical "episode"/"task"/"item" file, split concerns into descriptively named files such as `types.rs`/`data.rs` for data definitions and `run_*.rs`/`engine.rs` for behavior logic.

**Why:** Duplicate base names make imports, `grep`, stack traces, and doc links confusing. It is hard to tell which `episode` module a symbol comes from, and refactoring one file can silently affect the wrong module.

#### Bad

```text
planner/mcts/train/episode.rs         # EpisodeStep / Episode data
planner/mcts/train/trainer/episode.rs # Trainer::run_episode logic
```

Both files appear as `episode` in import paths (`train::episode` vs `train::trainer::episode`). The name does not reveal which holds data and which holds behavior.

#### Good

```text
planner/mcts/train/episode.rs         # EpisodeStep / Episode data
planner/mcts/train/trainer/run_episode.rs # episode generation logic
```

The behavior file is named after what it does, so the two modules are immediately distinguishable.

#### Migration

1. Identify which file is primarily data/definitions and which is primarily behavior.
2. Rename the behavior file to a verb-led or role-led name (`run_episode.rs`, `engine.rs`, `runner.rs`).
3. Update the parent `mod.rs` declaration.
4. Run `cargo test` to confirm the module tree still resolves.

#### Exception

Thin `mod.rs` indexes that re-export a sibling file may share the file's base name intentionally (e.g., `mod episode; pub use episode::*;`).


---

## Documentation Code Snippets

### Always Annotate Code Snippets with Source Location

**Rule:** Every code snippet in documentation (markdown files, READMEs, inline comments) that shows project source code must begin with an explicit source-location comment.

The comment must include:
1. **File path** — relative to project root (e.g., `octopus-cli/src/hooks/engine.rs`)
2. **Approximate line number** — prefixed with `~line` (e.g., `~line 196`)
3. **Function or item name** — what the snippet is showing (e.g., `HookEngine::trigger`)

```rust
// octopus-cli/src/hooks/engine.rs ~line 206 — HookEngine::trigger
pub async fn trigger(&self, event: HookEvent, matcher_value: &str) -> Vec<HookResult> {
    // ...
}
```

#### Why

- **Traceability:** A reader can grep the codebase and jump to the exact definition.
- **Drift detection:** When source changes, the line number becomes visibly outdated, signaling the doc needs updating.
- **Reviewability:** PR reviewers can verify doc snippets against source without hunting.

#### Bad

```rust
pub async fn trigger(&self, event: HookEvent, matcher_value: &str) -> Vec<HookResult> {
```

```rust
// In HookEngine
trigger(&self, event, matcher_value)
```

```rust
// engine.rs
trigger(...)
```

#### Good

```rust
// octopus-cli/src/hooks/engine.rs ~line 206 — HookEngine::trigger
pub async fn trigger(&self, event: HookEvent, matcher_value: &str) -> Vec<HookResult> {
```

```rust
// octopus-cli/src/hooks/runner.rs ~line 61 — run_hook
pub async fn run_hook(command: &str, event: &HookEvent, timeout_secs: u64, cwd: Option<&Path>) -> HookResult {
```

```rust
// octopus-cli/src/soul/toolset.rs ~line 126 — KimiToolset::requires_approval (private associated fn)
fn requires_approval(name: &str) -> bool {
    matches!(name, "Shell" | "WriteFile" | "StrReplaceFile" | "Agent")
}
```

#### Abbreviated Snippets

If a snippet is intentionally truncated (omitting fields, match arms, or helper logic), add `(abbreviated)` to the comment:

```rust
// octopus-cli/src/hooks/engine.rs ~line 59 — HookEngine (abbreviated)
pub struct HookEngine {
    hooks: Vec<HookDef>,
    // ... callbacks omitted
}
```

#### Pseudo-Code Snippets

If a snippet is conceptual / pseudo-code (not actual source), mark it explicitly:

```rust
// Conceptual pseudo-code: cloning once per matched hook
let event = event.clone();
```

#### Demo / Example Markers

If a snippet is a pure teaching example with no corresponding source file (illustrative code invented for documentation purposes), mark it with `docref: demo` or `docref: example` as the first line of the code block. This tells `docref` not to attempt matching it against the codebase during migration or drift checks.

```rust
// docref: demo
fn how_to_clone<T: Clone>(item: T) -> (T, T) {
    (item.clone(), item.clone())
}
```

```python
# docref: example
def hello(name: str) -> str:
    return f"Hello, {name}!"
```

Use any of the supported comment styles (`//`, `#`, `<!--`, `--`, `;`, `(*`, etc.) for the language in question. The marker keywords `demo` or `example` are interchangeable.

#### Exception

- External examples (e.g., a Python hook script in `config.toml` documentation) do not need source-location comments because they are user-written, not project source.
- Shell commands and JSON examples do not need source-location comments unless they are machine-generated from the codebase.
