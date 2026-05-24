# Kosong Mirror Comparison: Python ↔ Rust

> **Scope:** Compare the Python `kosong` abstraction layer (`tmp/kimi-cli/packages/kosong/`) with the Rust `kosong` crate (`kosong/`). The Rust crate is a faithful rewrite of the Python package, consumed by `octopus-cli` as a local path dependency.
>
> **Last updated:** 2026-05-24

---

## Summary

| Metric | Python kosong | Rust kosong | Notes |
|--------|--------------|-------------|-------|
| **LOC** | ~2,800 | ~1,600 | Rust is ~57% of Python size |
| **Modules** | 12 files + contrib/ | 10 `.rs` files | Contrib providers not yet ported |
| **Providers** | Kimi, OpenAI-Legacy, OpenAI-Responses, Anthropic, Google GenAI, Echo, Mock | Kimi, OpenAI-Legacy | Only core providers implemented |
| **Streaming** | Full async streaming | Full async streaming | Both use SSE; Rust uses `futures::Stream` |
| **Tool traits** | `CallableTool`, `CallableTool2[Params]` | `CallableTool`, `CallableTool2` | Rust uses `schemars` instead of Pydantic |

**Overall parity: ~75%** — Core engine (generate, step, providers, tooling) is complete. Missing: contrib providers (Anthropic, Google, OpenAI-Responses), Echo/Mock providers, advanced tooling (MCP tool wrapper, empty tool), and the `contrib.context` module.

---

## Module-by-Module Mapping

### 1. `__init__.py` → `lib.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `kosong.step()` | `kosong::step()` | ✅ | Same signature; Rust uses `tokio::task::JoinHandle` instead of `asyncio.Future` |
| `kosong.generate()` | `kosong::generate()` | ✅ | Same streaming semantics |
| `StepResult` | `StepResult` | ✅ | `tool_results()` awaits futures identically |
| `GenerateResult` | `GenerateResult` | ✅ | Same fields: `id`, `message`, `usage` |
| `__all__` exports | Re-exports in `lib.rs` | ✅ | All major types re-exported at crate root |

**Gap:** Python `step()` has an `on_tool_result` callback that fires when each tool future completes. Rust `step()` currently only collects futures; the callback is not wired. (The `octopus-cli` `KimiSoul` implements its own `on_tool_result` at a higher layer instead.)

---

### 2. `_generate.py` → `generate.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `generate()` async function | `generate()` async function | ✅ | Identical logic: stream → merge parts → validate |
| `GenerateResult` dataclass | `GenerateResult` struct | ✅ | Same 3 fields |
| `on_message_part` callback | `on_message_part` callback | ✅ | `FnMut(StreamedMessagePart)` in Rust vs async callback in Python |
| `on_tool_call` callback | `on_tool_call` callback | ✅ | Fires when a complete `ToolCall` is assembled |
| Part merging logic | Part merging logic | ✅ | `ContentPart::merge_in_place()`, `ToolCall::merge_in_place()` |
| Empty response validation | Empty response validation | ✅ | Identical "think-only" guard |
| `_message_append()` | `_message_append()` | ✅ | Same match logic |

**Gap:** Python's `on_message_part` callback is async (`await callback(...)`); Rust's is synchronous `FnMut`. For most use cases this is equivalent since the callback is side-effect-only.

---

### 3. `chat_provider/__init__.py` → `chat_provider.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `ChatProvider` Protocol | `ChatProvider` trait (`#[async_trait]`) | ✅ | Same methods: `name`, `model_name`, `thinking_effort`, `generate`, `with_thinking` |
| `RetryableChatProvider` Protocol | `RetryableChatProvider` trait | ✅ | Same `on_retryable_error` method |
| `StreamedMessage` Protocol | `StreamedMessage` struct | ✅ | Rust uses `BoxStream<'static, Part>` instead of async iterator |
| `StreamedMessagePart` type alias | `Part` enum + type alias | ✅ | `ContentPart \| ToolCall \| ToolCallPart` → `Part::Content/ToolCall/ToolCallPart` |
| `TokenUsage` Pydantic model | `TokenUsage` struct | ✅ | Same fields + `input()`/`total()` methods |
| `ThinkingEffort` Literal | `ThinkingEffort` type alias (`String`) | 🔄 | Rust uses plain `String` instead of enum; validated at provider level |
| `ChatProviderError` | `ChatProviderError` | ✅ | `thiserror` vs Exception hierarchy |
| `APIConnectionError` | `APIConnectionError` | ✅ | Same semantics |
| `APITimeoutError` | `APITimeoutError` | ✅ | Same semantics |
| `APIStatusError` | `APIStatusError` | ✅ | Same fields: `status_code`, `message`, `request_id` |
| `APIEmptyResponseError` | `APIEmptyResponseError` | ✅ | Same semantics |
| `convert_httpx_error()` | `convert_httpx_error()` + `convert_reqwest_error()` | ✅ | Maps `httpx` → `reqwest` error types |

---

### 4. `message.py` → `message.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `Message` Pydantic model | `Message` struct | ✅ | Same fields: `role`, `name`, `content`, `tool_calls`, `tool_call_id`, `partial` |
| `ContentPart` union | `ContentPart` enum | ✅ | `Text`, `Think`, `ImageUrl`, `AudioUrl`, `VideoUrl` |
| `TextPart` | `ContentPart::Text` | ✅ | |
| `ThinkPart` | `ContentPart::Think` | ✅ | Same `think` + `encrypted` fields |
| `ImageUrl` / `AudioUrl` / `VideoUrl` | Same structs | ✅ | |
| `ToolCall` Pydantic model | `ToolCall` struct | ✅ | Same: `call_type`, `id`, `function`, `extras` |
| `ToolCallPart` | `ToolCallPart` struct | ✅ | Same: `arguments_part` |
| `FunctionBody` | `FunctionBody` struct | ✅ | Same: `name`, `arguments` |
| `Role` enum | `Role` enum | ✅ | `System`, `User`, `Assistant`, `Tool` |
| `Message.extract_text()` | `Message.extract_text()` | ✅ | Same implementation |
| `merge_in_place()` on parts | `merge_in_place()` methods | ✅ | Both `ContentPart` and `ToolCall` |
| Content serialization | `serialize_content()` | ✅ | Single text → string; multiple → array |

---

### 5. `tooling/__init__.py` → `tooling.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `Tool` Pydantic model | `Tool` struct | ✅ | Same: `name`, `description`, `parameters` |
| `DisplayBlock` ABC | `DisplayBlock` enum | 🔄 | Rust uses plain enum; no subclass registry. Python has dynamic subclass dispatch via Pydantic |
| `BriefDisplayBlock` | `DisplayBlock::Brief` | ✅ | |
| `UnknownDisplayBlock` | — | ❌ | Not needed in Rust (no dynamic dispatch) |
| `ToolReturnValue` Pydantic model | `ToolReturnValue` struct | ✅ | Same fields: `is_error`, `output`, `message`, `display`, `extras` |
| `ToolOk` helper class | `ToolReturnValue::ok()` / `ok_parts()` | ✅ | Constructor methods instead of subclass |
| `ToolError` helper class | `ToolReturnValue::error()` | ✅ | |
| `ToolResult` Pydantic model | `ToolResult` struct | ✅ | Same: `tool_call_id`, `return_value` |
| `ToolResultFuture` | `tokio::task::JoinHandle<ToolResult>` | ✅ | Equivalent async future type |
| `HandleResult` union | `HandleResult` enum | ✅ | `Ready(ToolResult) \| Pending(JoinHandle)` |
| `Toolset` Protocol | `Toolset` trait (`#[async_trait]`) | ✅ | Same: `tools()`, `handle()` |
| `CallableTool` ABC | `CallableTool` trait | ✅ | Same: `name`, `description`, `parameters`, `call_raw()` |
| `CallableTool2[Params]` | `CallableTool2` trait | ✅ | Same: `Params: DeserializeOwned + JsonSchema`, `call_typed()` |
| `CallableTool2Adapter` | `CallableTool2Adapter<T>` | ✅ | Bridges `CallableTool2` → `CallableTool` |
| JSON schema generation | `schemars::schema_for!` | ✅ | Replaces Pydantic's `model_json_schema()` |

**Gap:** Python's `DisplayBlock` supports user-defined subclasses with a runtime registry. Rust's is a closed enum. This is a deliberate simplification — `octopus-cli` doesn't use custom display blocks.

**Gap:** Python's `CallableTool.call()` validates arguments with `jsonschema` before dispatch. Rust's `SimpleToolset` parses JSON but does not validate against the schema. (Validation happens at the LLM level; the model is expected to produce valid JSON.)

---

### 6. `tooling/simple.py` → `simple_toolset.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `SimpleToolset` class | `SimpleToolset` struct | ✅ | Same: `HashMap<String, Arc<dyn CallableTool>>` |
| `+=` operator (`add()`) | `SimpleToolset::add()` | ✅ | |
| `-=` operator (`remove()`) | `SimpleToolset::remove()` | ✅ | Both panic on missing tool |
| `tools()` property | `Toolset::tools()` | ✅ | |
| `handle()` | `Toolset::handle()` | ✅ | Spawns tokio task for each tool call |
| JSON argument parsing | JSON argument parsing | ✅ | Same `serde_json::from_str` logic |

---

### 7. `chat_provider/kimi.py` → `provider/kimi.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `Kimi` class | `Kimi` struct | ✅ | Builder pattern: `new()`, `with_api_key()`, etc. |
| SSE streaming | SSE streaming | ✅ | `create_sse_stream()` using `futures::stream::unfold` |
| Non-streaming fallback | Non-streaming fallback | ✅ | Parses `ChatCompletionResponse` directly |
| Message building | `build_messages()` | ✅ | Same content serialization logic |
| Tool building | `build_tools()` | ✅ | Uses `tool_to_openai()` + `ensure_property_types()` |
| Request building | `build_request()` | ✅ | Same extra_body / thinking / kwargs merge |
| Error conversion | `convert_reqwest_error()` | ✅ | Maps to kosong error types |
| `x-request-id` header | `x-request-id` header | ✅ | Extracted and passed to `APIStatusError` |
| `on_retryable_error()` | `on_retryable_error()` | ✅ | Recreates HTTP client |

---

### 8. `chat_provider/openai_common.py` → `provider/openai_common.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `tool_to_openai()` | `tool_to_openai()` | ✅ | |
| `convert_httpx_error()` | `convert_reqwest_error()` + `convert_httpx_error()` | ✅ | |
| `convert_status_error()` | `convert_status_error()` | ✅ | |
| `thinking_effort_to_reasoning_effort()` | `thinking_effort_to_reasoning_effort()` | ✅ | Same mapping |
| `reasoning_effort_to_thinking_effort()` | `reasoning_effort_to_thinking_effort()` | ✅ | Same mapping |

---

### 9. `chat_provider/openai_legacy.py` → `provider/openai_legacy.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `OpenAILegacy` class | `OpenAILegacy` struct | ✅ | Builder pattern identical to Kimi |
| `with_reasoning_key()` | `with_reasoning_key()` | ✅ | Rust-specific (not in Python base) |
| SSE streaming | Reuses `create_sse_stream()` from kimi | ✅ | DRY — same SSE parser |
| Non-streaming fallback | Non-streaming fallback | ✅ | |

---

### 10. `chat_provider/openai_types.py` (Python inline) → `provider/openai_types.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `ChatCompletionRequest` | `ChatCompletionRequest` | ✅ | Same fields |
| `ChatCompletionMessage` | `ChatCompletionMessage` | ✅ | Same fields |
| `ChatCompletionTool` | `ChatCompletionTool` | ✅ | Same fields |
| `FunctionDefinition` | `FunctionDefinition` | ✅ | Same fields |
| `ToolCallObject` | `ToolCallObject` | ✅ | Same fields |
| `FunctionCallObject` | `FunctionCallObject` | ✅ | Same fields |
| `ChatCompletionResponse` | `ChatCompletionResponse` | ✅ | Same fields |
| `Choice` | `Choice` | ✅ | Same fields |
| `Usage` | `Usage` | ✅ | Same fields |
| `PromptTokensDetails` | `PromptTokensDetails` | ✅ | Same fields |
| `ChatCompletionChunk` | `ChatCompletionChunk` | ✅ | Same fields |
| `ChunkChoice` | `ChunkChoice` | ✅ | Same fields |

---

### 11. `utils/jsonschema.py` → `utils/jsonschema.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `deref_json_schema()` | `deref_json_schema()` | ✅ | Same `$ref` resolution logic |
| `ensure_property_types()` | `ensure_property_types()` | ✅ | Same recursive normalization |
| `normalize_property()` | `normalize_property()` | ✅ | Same `type` injection |
| `infer_type_from_structure()` | `infer_type_from_structure()` | ✅ | Same keyword → type mapping |
| `infer_type_from_values()` | `infer_type_from_values()` | ✅ | Same value → type mapping |

---

## What's Missing in Rust Kosong

### ❌ Contrib Providers (`contrib/chat_provider/`)

| Provider | Python File | Rust Status | Priority |
|----------|-------------|-------------|----------|
| Anthropic | `contrib/chat_provider/anthropic.py` | ❌ Not started | P2 |
| Google GenAI | `contrib/chat_provider/google_genai.py` | ❌ Not started | P2 |
| OpenAI Responses | `contrib/chat_provider/openai_responses.py` | ❌ Not started | P2 |
| Common contrib utils | `contrib/chat_provider/common.py` | ❌ Not started | P2 |

These are non-core providers. `octopus-cli` currently only needs Kimi and OpenAI-Legacy.

### ❌ Echo / Mock Providers (`chat_provider/echo/`, `chat_provider/mock.py`)

| Provider | Python File | Rust Status | Priority |
|----------|-------------|-------------|----------|
| Echo provider | `chat_provider/echo/echo.py` | ❌ Not started | P3 |
| Scripted echo | `chat_provider/echo/scripted_echo.py` | ❌ Not started | P3 |
| Mock provider | `chat_provider/mock.py` | ❌ Not started | P3 |

Useful for testing; not needed for production CLI.

### ❌ Contrib Context (`contrib/context/`)

| Module | Python File | Rust Status | Notes |
|--------|-------------|-------------|-------|
| Linear context | `contrib/context/linear.py` | ❌ Not started | Message history management utility |

### ❌ Advanced Tooling (`tooling/`)

| Feature | Python File | Rust Status | Notes |
|---------|-------------|-------------|-------|
| `EmptyToolset` | `tooling/empty.py` | ❌ Not started | No-op toolset for testing |
| MCP tool wrapper | `tooling/mcp.py` | ❌ Not started | Wraps MCP client as kosong Tool |
| Tool validation errors | `tooling/error.py` | 🔄 Partial | `ToolReturnValue::error()` covers basic cases |

### ❌ Utility Modules

| Feature | Python File | Rust Status | Notes |
|---------|-------------|-------------|-------|
| `Callback` type + `callback()` helper | `utils/aio.py` | ❌ Not needed | Rust callbacks are plain closures |
| `JsonType` alias | `utils/typing.py` | ❌ Not needed | Rust uses `serde_json::Value` |

---

## Key API Differences

### Callback Semantics

| | Python | Rust |
|---|--------|------|
| `on_message_part` | `Callback[[StreamedMessagePart], None]` — may be async | `FnMut(StreamedMessagePart)` — synchronous |
| `on_tool_call` | `Callback[[ToolCall], None]` — may be async | `FnMut(ToolCall)` — synchronous |
| `on_tool_result` | `Callable[[ToolResult], None]` — fires on future completion | Not present in `step()`; handled at `KimiSoul` layer |

**Impact:** The Rust version's synchronous callbacks are simpler but cannot perform async I/O inside the callback. The `octopus-cli` `KimiSoul` works around this by registering its own `on_tool_result` on the toolset after the `step()` call.

### Error Handling

| | Python | Rust |
|---|--------|------|
| Base error | `ChatProviderError(Exception)` | `ChatProviderError(thiserror::Error)` |
| Transport | `httpx.HTTPError` → kosong errors | `reqwest::Error` → kosong errors |
| Empty response | `APIEmptyResponseError("msg")` | `ChatProviderError::new("msg")` with matching string |

The Rust `llm.rs` in `octopus-cli` classifies kosong error strings back into specific `OctopusError` variants.

### Schema Generation

| | Python | Rust |
|---|--------|------|
| Typed tool params | Pydantic `BaseModel` → `model_json_schema()` | `schemars::JsonSchema` → `schema_for!()` |
| Schema cleanup | `_GenerateJsonSchemaNoTitles` | Not needed — `schemars` omits titles by default |
| `$ref` dereferencing | `deref_json_schema()` | `deref_json_schema()` — identical algorithm |

---

## How `octopus-cli` Uses Kosong

```
octopus-cli/src/llm.rs
├── `LLM::complete()` → calls `kosong::generate()` via `kosong` crate
├── `build_kosong_provider()` → builds `kosong::provider::kimi::Kimi` or `OpenAILegacy`
└── Type conversions: `wire_to_kosong_*` / `kosong_to_wire_*`

octopus-cli/src/soul/mod.rs (KimiSoul)
├── `_run_step_once_inner()` → manually implements step logic (NOT using kosong::step)
│   └── Calls `LLM::complete()` (blocking, no streaming)
│   └── Manually handles tool calls via `KimiToolset`
└── Streaming is NOT using kosong's streaming; it's post-hoc via `on_tool_result`
```

**Important:** `KimiSoul` does **not** call `kosong::step()`. Instead, it calls `LLM::complete()` (which wraps `kosong::generate()`) in a blocking fashion and handles tools manually. This means:

- ❌ `KimiSoul` does **not** stream message parts to the user in real-time
- ❌ `KimiSoul` does **not** use kosong's built-in concurrent tool dispatch
- ✅ `KimiSoul` handles D-Mail, hooks, telemetry, MCP, etc. at a higher layer

The kosong crate is complete and functional; `octopus-cli` simply chooses not to use `kosong::step()` because it needs more control over the step loop.

---

## Priority Roadmap

### P0 — Critical for parity
1. **Wire `kosong::step()` into `KimiSoul`** — Replace manual step logic with kosong's `step()` to get native streaming and concurrent tool dispatch. This is blocked on `on_message_part` wiring to the wire layer.

### P1 — Important
2. **Add `on_tool_result` to Rust `kosong::step()`** — Match Python's behavior of firing a callback when each tool future completes.
3. **Implement OpenAI Responses provider** — Needed for full OpenAI compatibility.

### P2 — Nice to have
4. **Anthropic provider** — For Claude support.
5. **Google GenAI provider** — For Gemini support.
6. **Echo / Mock providers** — For testing.

### P3 — Future
7. **MCP tool wrapper** — Bridge MCP tools into kosong `Toolset`.
8. **Contrib context modules** — `linear.py` and similar utilities.

---

## File Count Comparison

| Category | Python Files | Rust Files | Status |
|----------|-------------|------------|--------|
| Core (lib, generate, step, message, tooling) | 5 | 5 | ✅ Complete |
| Chat providers (Kimi, OpenAI-Legacy) | 2 | 3 (+types) | ✅ Complete |
| Provider commons | 1 | 1 | ✅ Complete |
| Toolset + utils | 3 | 3 | ✅ Complete |
| Contrib providers | 4 | 0 | ❌ Missing |
| Echo / Mock | 3 | 0 | ❌ Missing |
| **Total** | **18** | **12** | **~67% file parity** |
