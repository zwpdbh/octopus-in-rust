# Plan: Integrate `brain` Agent Q&A into `fafcn-web`

**Goal:** Add a Q&A page to `fafcn-web` that answers FAF-related questions by running the same `brain` agent runtime + `faf-units` plugin tools used by `qqbot-core`.

**Approach:** Run the agent inside `fafcn-server`. Expose a new HTTP endpoint that the Dioxus frontend calls. Keep the first version synchronous (full answer in one response), leaving room to upgrade to SSE streaming later.

---

## 1. Architecture

```text
┌─────────────────┐      POST /api/ask        ┌────────────────────┐
│   fafcn-web     │  ───────────────────────► │    fafcn-server    │
│   /qa page      │  { question, history }    │                    │
│                 │                           │ 1. Build Brain     │
│                 │ ◄──────────────────────── │ 2. Load plugins    │
│                 │      { answer, events }   │ 3. Run turn        │
└─────────────────┘                           └────────────────────┘
```

Key decisions:

- **Agent lives in the backend.** `brain` depends on `extism`, `tokio`, and native network calls; it is not WASM-browser friendly.
- **One `Brain` per request** in the first version. This avoids shared mutable state and makes the endpoint stateless. Later you can pool brains or share an `InMemoryMessageStore` if you want conversation history across requests.
- **OpenAI-compatible provider via `DefaultProviderFactory`.** For the first iteration, set `api_key` directly in `BrainConfig`. If you need KimiCode/OAuth later, copy or reuse `QqbotProviderFactory` from `apps/qqbot-core`.
- **Load only the `faf-units` plugin.** Use `ExtismPluginSource::with_filter` so the server does not accidentally load party/HTTP plugins.

---

## 2. Files to Change

| File                                                        | Change                                                  |
| ----------------------------------------------------------- | ------------------------------------------------------- |
| `apps/fafcn-server/Cargo.toml`              | Add `brain` dependency (add `futures-util`/`tokio-stream` only if you later switch to streaming) |
| `apps/fafcn-server/src/qa.rs` (new)         | `QaConfig`, `create_brain`, `ask`                                                               |
| `apps/fafcn-server/src/main.rs`             | Import qa module, add `POST /api/ask`, wire state                                               |
| `apps/fafcn-web/src/main.rs`                | Add `/qa` route inside the `#[layout(Navbar)]` block                                            |
| `apps/fafcn-web/src/views/qa.rs` (new)      | Chat UI: message list, input, send handler                                                      |
| `apps/fafcn-web/src/views/mod.rs`           | Re-export `qa` module                                                                           |
| `apps/fafcn-web/src/views/navbar.rs`        | Add "Q&A" link                                                                                  |

---

## 3. Backend Implementation Steps

### 3.1 Add dependencies

```toml
# apps/fafcn-server/Cargo.toml
[dependencies]
brain = { workspace = true }
```

`reqwest` is not required for the LLM call because `brain` handles provider networking. Keep `reqwest` out unless you add non-agent endpoints later. `futures-util` and `tokio-stream` are only needed if you switch from `run_turn_to_completion` to streaming `run_turn`; leave them out for v1.

### 3.2 Create `apps/fafcn-server/src/qa.rs`

#### 3.2.1 Configuration

Read settings from environment variables so you do not need a new config file yet:

| Env var                  | Purpose                         | Example                     |
| ------------------------ | ------------------------------- | --------------------------- |
| `FAFCN_LLM_BASE_URL`     | OpenAI-compatible base URL      | `https://api.openai.com/v1` |
| `FAFCN_LLM_API_KEY`      | API key                         | `sk-...`                    |
| `FAFCN_LLM_MODEL`        | Model name                      | `gpt-4o`                    |
| `FAFCN_PLUGINS_DIR`      | Directory with`.wasm` plugins   | `data/qqbot-data/plugins`   |
| `FAFCN_QA_SYSTEM_PROMPT` | Optional system prompt override | —                           |

```rust
// apps/fafcn-server/src/qa.rs
use serde::Deserialize;
use std::{collections::HashSet, path::PathBuf};

#[derive(Clone, Debug)]
pub struct QaConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub plugins_dir: PathBuf,
    pub system_prompt: String,
    pub max_steps_per_turn: usize,
}

impl QaConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let default_plugins = workspace_root.join("data/qqbot-data/plugins");

        Ok(Self {
            base_url: env_var_or("FAFCN_LLM_BASE_URL", "https://api.openai.com/v1"),
            api_key: std::env::var("FAFCN_LLM_API_KEY").unwrap_or_default(),
            model: env_var_or("FAFCN_LLM_MODEL", "gpt-4o"),
            plugins_dir: std::env::var("FAFCN_PLUGINS_DIR")
                .map(PathBuf::from)
                .unwrap_or(default_plugins),
            system_prompt: env_var_or(
                "FAFCN_QA_SYSTEM_PROMPT",
                "You are an expert assistant for the game Forged Alliance Forever...",
            ),
            max_steps_per_turn: 16,
        })
    }
}
```

#### 3.2.2 Build a `Brain`

Use `ExtismPluginSource::with_filter` to load only `faf_units_plugin`:

```rust
use brain::{Brain, BrainBuilder, BrainConfig, ExtismPluginSource, ToolAwareSystemPromptPolicy};
use std::sync::Arc;

pub async fn create_brain(config: &QaConfig) -> anyhow::Result<Brain> {
    let allowed: HashSet<String> = ["faf_units_plugin"].into_iter().map(String::from).collect();
    let tool_source = Arc::new(ExtismPluginSource::with_filter(&config.plugins_dir, allowed));

    let brain_config = BrainConfig {
        system_prompt: config.system_prompt.clone(),
        base_url: config.base_url.clone(),
        api_key: config.api_key.clone(),
        model: config.model.clone(),
        max_steps_per_turn: config.max_steps_per_turn,
        tool_sources: vec![tool_source],
        ..Default::default()
    };

    let brain = BrainBuilder::default()
        .from_config(brain_config)
        .with_system_prompt_policy(Arc::new(ToolAwareSystemPromptPolicy))
        .build()
        .await?;

    Ok(brain)
}
```

> **Note:** If you want to reuse `qqbot-core`'s KimiCode auth, replace `DefaultProviderFactory` by implementing a small `ProviderFactory` in `fafcn-server` or by depending on `qqbot-config` and copying `QqbotProviderFactory`. The `brain` crate only requires something that implements `brain::ProviderFactory`.

#### 3.2.3 Run one turn and collect the answer

```rust
pub async fn ask(config: &QaConfig, question: &str) -> anyhow::Result<QaResponse> {
    let mut brain = create_brain(config).await?;
    let result = brain.run_turn_to_completion(question.into()).await?;

    let events = result
        .events
        .into_iter()
        .filter_map(|ev| match ev {
            BrainEvent::ToolCall { name, arguments, .. } => {
                Some(QaEvent::ToolCall { name, arguments })
            }
            BrainEvent::ToolResult { output, is_error, .. } => {
                Some(QaEvent::ToolResult { output, is_error })
            }
            _ => None,
        })
        .collect();

    Ok(QaResponse {
        answer: result.final_text,
        events,
    })
}
```

Add request/response structs:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct AskRequest {
    pub question: String,
    /// Reserved for future conversation-memory support. Ignored in v1.
    #[serde(default)]
    pub history: Vec<ChatMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct QaResponse {
    pub answer: String,
    pub events: Vec<QaEvent>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
pub enum QaEvent {
    ToolCall { name: String, arguments: serde_json::Value },
    ToolResult { output: String, is_error: bool },
}
```

> **Conversation history:** `run_turn_to_completion` takes a `TurnInput`. If you want to pass `history` into the turn, inspect `TurnInput` (likely `String` or a struct) and convert the history into the format `brain` expects. If `TurnInput` is just `From<String>`, implement history support later by using `run_turn` with a custom message store.

### 3.3 Wire the endpoint in `main.rs`

```rust
// apps/fafcn-server/src/main.rs
mod qa;

#[derive(Clone)]
struct AppState {
    blueprints: Arc<FafBlueprints>,
    portraits_dir: Arc<PathBuf>,
    assets_dir: Arc<PathBuf>,
    qa_config: Arc<qa::QaConfig>,
}

// in main()
let qa_config = Arc::new(qa::QaConfig::from_env()?);

let app = Router::new()
    .route("/api/units", get(list_units))
    .route("/api/units/:id", get(get_unit))
    .route("/api/portraits/:id", get(get_portrait))
    .route("/ws/simulate", get(simulate_ws_handler))
    .route("/api/ask", post(ask_handler))   // <-- new
    ...

async fn ask_handler(
    State(state): State<AppState>,
    Json(req): Json<qa::AskRequest>,
) -> Result<impl IntoResponse, AppError> {
    let resp = qa::ask(&state.qa_config, &req.question).await?;
    Ok(axum::Json(resp))
}
```

Make sure CORS allows `POST` in addition to `GET`:

```rust
let cors = CorsLayer::new()
    .allow_origin(Any)
    .allow_methods([Method::GET, Method::POST])
    .allow_headers([header::CONTENT_TYPE]);
```

### 3.4 Error mapping

Add a `Brain(brain::BrainError)` variant to `AppError` and implement `From<brain::BrainError>`. Map it to a `500` response with the error message.

---

## 4. Frontend Implementation Steps

### 4.1 Add route

`fafcn-web` already uses a `Navbar` layout. Add `Qa` inside it so the nav bar is preserved:

```rust
// apps/fafcn-web/src/main.rs
use views::{Home, Navbar, Qa, Simulate};

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Home {},
        #[route("/simulate")]
        Simulate {},
        #[route("/qa")]
        Qa {},
}
```

### 4.2 Create `apps/fafcn-web/src/views/qa.rs`

Minimal chat UI:

```rust
use dioxus::prelude::*;

#[component]
pub fn Qa() -> Element {
    let mut input = use_signal(String::new);
    let mut messages = use_signal(Vec::<ChatMessage>::new);
    let mut loading = use_signal(|| false);

    let send = move |_| {
        let question = input.cloned();
        if question.trim().is_empty() {
            return;
        }
        input.set(String::new());
        messages.write().push(ChatMessage {
            role: "user".into(),
            content: question.clone(),
        });
        loading.set(true);

        spawn(async move {
            match ask_api(&question).await {
                Ok(resp) => {
                    messages.write().push(ChatMessage {
                        role: "assistant".into(),
                        content: resp.answer,
                    });
                }
                Err(e) => {
                    messages.write().push(ChatMessage {
                        role: "assistant".into(),
                        content: format!("Error: {e}"),
                    });
                }
            }
            loading.set(false);
        });
    };

    rsx! {
        div { class: "flex flex-col h-screen bg-neutral-950 text-gray-200",
            // header
            div { class: "px-4 py-3 border-b border-neutral-800",
                h1 { class: "text-lg font-semibold", "FAF Q&A Agent" }
            }
            // messages
            div { class: "flex-1 overflow-y-auto p-4 space-y-4",
                for msg in messages.read().iter() {
                    MessageBubble { role: msg.role.clone(), content: msg.content.clone() }
                }
                if *loading.read() {
                    div { class: "text-sm text-neutral-500", "Thinking..." }
                }
            }
            // input
            div { class: "p-4 border-t border-neutral-800",
                div { class: "flex gap-2",
                    input {
                        class: "flex-1 bg-neutral-900 border border-neutral-700 rounded px-3 py-2 text-sm",
                        placeholder: "Ask about FAF units...",
                        value: "{input}",
                        oninput: move |e| input.set(e.value()),
                        onkeydown: move |e| if e.key() == "Enter" { send(()) }
                    }
                    button {
                        class: "px-4 py-2 bg-blue-600 rounded text-sm hover:bg-blue-500",
                        onclick: send,
                        "Send"
                    }
                }
            }
        }
    }
}

#[component]
fn MessageBubble(role: String, content: String) -> Element {
    let is_user = role == "user";
    let align = if is_user { "justify-end" } else { "justify-start" };
    let bubble = if is_user { "bg-blue-600" } else { "bg-neutral-800" };
    rsx! {
        div { class: "flex {align}",
            div { class: "max-w-3xl px-4 py-2 rounded {bubble} text-sm whitespace-pre-wrap",
                "{content}"
            }
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, serde::Deserialize)]
struct AskResponse {
    answer: String,
}

async fn ask_api(question: &str) -> anyhow::Result<AskResponse> {
    let body = serde_json::json!({ "question": question });
    let resp = gloo_net::http::Request::post("/api/ask")
        .json(&body)?
        .send()
        .await?;
    Ok(resp.json::<AskResponse>().await?)
}
```

> **Note:** If you configured Dioxus to proxy to `localhost:3000` during development, `/api/ask` will reach `fafcn-server`. Otherwise use the full origin.

### 4.3 Update `apps/fafcn-web/src/views/mod.rs`

```rust
pub mod qa;
pub mod simulate;
```

### 4.4 Add navigation link

Edit `apps/fafcn-web/src/views/navbar.rs` and add the link next to the existing ones:

```rust
NavLink { to: Route::Home {}, label: "Home" }
NavLink { to: Route::Simulate {}, label: "Simulate" }
NavLink { to: Route::Qa {}, label: "Q&A" }
```

---

## 5. Build & Run

### 5.1 Build the plugin (if not already present)

```bash
cargo build --release -p faf-units-plugin --target wasm32-unknown-unknown
mkdir -p data/qqbot-data/plugins
cp target/wasm32-unknown-unknown/release/faf_units_plugin.wasm data/qqbot-data/plugins/
```

Verify the file exists:

```bash
ls data/qqbot-data/plugins/faf_units_plugin.wasm
```

### 5.2 Run the backend

```bash
export FAFCN_LLM_BASE_URL="https://api.openai.com/v1"
export FAFCN_LLM_API_KEY="sk-..."
export FAFCN_LLM_MODEL="gpt-4o"
cargo run -p fafcn-server
```

### 5.3 Test the endpoint with curl

```bash
curl -X POST http://localhost:3000/api/ask \
  -H 'Content-Type: application/json' \
  -d '{"question": "Which UEF T1 tank has the highest DPS?"}'
```

Expected response shape:

```json
{
  "answer": "...",
  "events": [
    { "kind": "ToolCall", "name": "faf_units_search", "arguments": { ... } },
    { "kind": "ToolResult", "output": "...", "is_error": false }
  ]
}
```

### 5.4 Run the frontend

```bash
cargo xtask fafcn frontend
# or directly:
cd apps/fafcn-web && dx serve --release
```

Open the `/qa` route and verify the chat works.

---

## 6. Common Pitfalls

1. **`Brain` is not `Clone` and `run_turn` needs `&mut self`.**
   - First version: create a new `Brain` per request. This is slower but simplest.
   - Later: keep one `Brain` behind `Arc<tokio::sync::Mutex<Brain>>` and lock it per turn. Note that this serializes requests.

2. **`ExtismPluginSource` filters by file stem.**
   - Make sure the file is named `faf_units_plugin.wasm` (the crate is `faf-units-plugin`, but the output file uses underscores: `faf_units_plugin.wasm`).

3. **Plugin manifest (`faf_units_plugin.json`).**
   - The plugin may need a manifest to allow network/file access. `faf-units` bakes data into the WASM binary and does not need network, but double-check by copying an existing manifest from `data/qqbot-data/plugins/` if loading fails.

4. **`ToolAwareSystemPromptPolicy` requires tool schemas.**
   - Make sure the plugin's `register_tools` export returns valid `ToolDef`s. If the system prompt becomes malformed, switch to `DefaultSystemPromptPolicy` temporarily.

5. **CORS.**
   - Dioxus dev server and `fafcn-server` run on different ports during development. Ensure `CorsLayer` allows `POST` and `Content-Type`.

6. **Environment variables vs. config file.**
   - The first version uses env vars. If you add more settings (multiple providers, OAuth, groups), consider introducing a `fafcn-server.toml` and loading it with `toml`.

---

## 7. Future Improvements

- **Streaming:** Upgrade `POST /api/ask` to return `text/event-stream` and emit `BrainEvent::TextPart` chunks as they arrive.
- **Conversation memory:** Share an `InMemoryMessageStore` across requests so the agent remembers earlier turns.
- **Multi-provider support:** Port `QqbotProviderFactory` from `qqbot-core` to support KimiCode and OAuth.
- **Additional tools:** Register host tools like `RecentMessagesTool` or a construction-simulation summary tool once those exist for the web context.
