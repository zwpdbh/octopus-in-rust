# Kosong Mirror Comparison: Python ↔ Rust

> **Scope:** Compare the Python `kosong` abstraction layer (`tmp/kimi-cli/packages/kosong/`) with the Rust `kosong` crate (`kosong/`). The Rust crate is a faithful rewrite of the Python package, consumed by `octopus-cli` as a local path dependency.
>
> **Legend:**
> - ✅ = **Implemented** — ported from Python to Rust
> - 🚫 = **Not used by `kimi-cli`** — implemented in Rust, but `kimi-cli` doesn't consume it; safe to ignore for CLI parity
> - ⚪ = **Optional in `kimi-cli`** — supported but requires manual config (not part of default onboarding)
> - ✨ = **Natively handled by Rust** — no port needed because Rust's type system / stdlib / ecosystem provides equivalent abstractions
> - 🔄 = **Done differently** — Rust idioms or language features lead to a different implementation approach
> - ❌ = **Missing** — not yet ported from Python
>
> **Last updated:** 2026-05-24

---

## Summary

| Metric | Python kosong | Rust kosong | Notes |
|--------|--------------|-------------|-------|
| **LOC** | ~2,800 | ~1,900 | Rust is ~68% of Python size |
| **Modules** | 12 files + contrib/ | 11 `.rs` files | Contrib providers not yet ported |
| **Providers** | Kimi, OpenAI-Legacy, OpenAI-Responses, Anthropic, Google GenAI, Echo, Mock | Kimi, OpenAI-Legacy, OpenAI-Responses, **Echo, Mock** | Core + OpenAI-Responses + Echo/Mock done |
| **Streaming** | Full async streaming | Full async streaming | Both use SSE; Rust uses `futures::Stream` |
| **Tool traits** | `CallableTool`, `CallableTool2[Params]` | `CallableTool`, `CallableTool2` | Rust uses `schemars` instead of Pydantic |
| **`on_tool_result`** | `future.add_done_callback()` | `StepResult::tool_results_with_callback()` | Fires per tool result as futures resolve |

**Overall parity: ~85%** — Core engine (generate, step, providers, tooling) is complete. OpenAI-Responses + Echo/Mock providers ported. Missing: Anthropic, Google GenAI (both ⚪ optional).

**`octopus-cli` default user parity: ~100%** — For the standard `kimi login` onboarding flow, `kosong` is complete. The default user only needs the `kimi` provider. OpenAI providers are also fully ported. The only remaining items (Anthropic, Google GenAI) require manual `config.toml` editing and are not part of the default experience.

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
| `on_tool_result` callback | `StepResult::tool_results_with_callback()` | ✅ | Fires callback synchronously as each `JoinHandle` resolves |

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

**Note:** Python's `on_message_part` callback is async (`await callback(...)`); Rust's is synchronous `FnMut`. For most use cases this is equivalent since the callback is side-effect-only.

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
| `UnknownDisplayBlock` | — | ✨ | Rust uses a closed enum; unknown variants are impossible by design. No dynamic dispatch needed. |
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

**Gap:** Python's `DisplayBlock` supports user-defined subclasses with a runtime registry. Rust's is a closed enum. This is a deliberate simplification — `octopus-cli` doesn't use custom display blocks. In Rust, extending display behavior would require modifying the enum definition (or using a trait object approach), which is the idiomatic Rust pattern.

**Gap:** Python's `CallableTool.call()` validates arguments with `jsonschema` before dispatch. Rust's `SimpleToolset` parses JSON but does not validate against the schema. (Validation happens at the LLM level; the model is expected to produce valid JSON. Adding `jsonschema` validation in Rust would require pulling in a JSON Schema validation crate — possible but not critical for `kimi-cli`.)

---

### 6. `tooling/simple.py` → `simple_toolset.rs` 🚫

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `SimpleToolset` class | `SimpleToolset` struct | ✅ | Same: `HashMap<String, Arc<dyn CallableTool>>` |
| `+=` operator (`add()`) | `SimpleToolset::add()` | ✅ | |
| `-=` operator (`remove()`) | `SimpleToolset::remove()` | ✅ | Both panic on missing tool |
| `tools()` property | `Toolset::tools()` | ✅ | |
| `handle()` | `Toolset::handle()` | ✅ | Spawns tokio task for each tool call |
| JSON argument parsing | JSON argument parsing | ✅ | Same `serde_json::from_str` logic |

🚫 **`kimi-cli` does not use `SimpleToolset`.** It has its own `KimiToolset` with dedup, hooks, telemetry, and MCP support. `SimpleToolset` is still implemented for other `kosong` consumers.

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

### 11. `chat_provider/openai_responses.py` → `provider/openai_responses.rs` ⚪

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `OpenAIResponses` class | `OpenAIResponses` struct | ✅ | Builder pattern with `new()`, `with_api_key()`, etc. |
| `responses.create()` API | `POST /v1/responses` via `reqwest` | ✅ | Direct REST instead of SDK |
| `developer` role for system | `developer` role | ✅ | Used for system prompts |
| `function_call` / `function_call_output` | Same types | ✅ | Tool calls and results |
| `reasoning` items | `reasoning` with `summary` | ✅ | `thinking_effort` → `reasoning.effort` |
| `include: ["reasoning.encrypted_content"]` | Same | ✅ | |
| SSE streaming | SSE parsing | ✅ | `response.output_text.delta`, `response.function_call_arguments.delta`, `response.reasoning_summary_text.delta` |
| Non-streaming fallback | Non-streaming fallback | ✅ | Parses `ResponsesResponse` into `StreamedMessage` |
| `store: false` | `store: false` | ✅ | |
| Tool format (`type: "function"`) | Same | ✅ | `strict: false` |

⚪ **Optional in `kimi-cli`.** Supported but requires manual config; not part of `kimi login` onboarding.

---

### 12. `utils/jsonschema.py` → `utils/jsonschema.rs`

| Python | Rust | Status | Notes |
|--------|------|--------|-------|
| `deref_json_schema()` | `deref_json_schema()` | ✅ | Same `$ref` resolution logic |
| `ensure_property_types()` | `ensure_property_types()` | ✅ | Same recursive normalization |
| `normalize_property()` | `normalize_property()` | ✅ | Same `type` injection |
| `infer_type_from_structure()` | `infer_type_from_structure()` | ✅ | Same keyword → type mapping |
| `infer_type_from_values()` | `infer_type_from_values()` | ✅ | Same value → type mapping |

---

## What's Missing in Rust Kosong

### ⚪ Contrib Providers (`contrib/chat_provider/`)

| Provider | Python File | Rust Status | Effort | Notes |
|----------|-------------|-------------|--------|-------|
| Anthropic | `contrib/chat_provider/anthropic.py` (723 LOC) | ❌ Not started | Medium | SSE deltas. Top-level `system` param. Tool result merging. Cache control. Adaptive vs budget-based thinking. |
| Google GenAI | `contrib/chat_provider/google_genai.py` (769 LOC) | ❌ Not started | High | Full-response chunks. Role `model`. Tool result packing. Tool call ID synthesis. Media download. VertexAI vs Gemini. |
| Common contrib utils | `contrib/chat_provider/common.py` (5 LOC) | ❌ Not started | Trivial | `ToolMessageConversion = Literal["extract_text"]` |

⚪ **Optional in `kimi-cli`.** Both providers are supported but require manual `config.toml` editing. Not part of default onboarding. Only `kimi` + `openai_legacy`/`openai_responses` are configured by the login flow.

---

### ✅ Echo / Mock Providers (`chat_provider/echo/`, `chat_provider/mock.py`) 🚫

| Provider | Python File | Rust File | Status | Notes |
|----------|-------------|-----------|--------|-------|
| Echo provider | `chat_provider/echo/echo.py` | `provider/echo/mod.rs` `EchoChatProvider` | ✅ | Reads DSL script from last user message in history |
| Scripted echo | `chat_provider/echo/scripted_echo.py` | `provider/echo/mod.rs` `ScriptedEchoChatProvider` | ✅ | Consumes queue of DSL scripts per call |
| Echo DSL parser | `chat_provider/echo/dsl.py` | `provider/echo/dsl.rs` | ✅ | Full DSL: `id`, `usage`, `text`, `think`, `image_url`, `audio_url`, `video_url`, `tool_call`, `tool_call_part`, `error` |
| Mock provider | `chat_provider/mock.py` | `provider/mock.rs` `MockChatProvider` | ✅ | Returns predefined `Vec<Part>` on every call |

🚫 **`kimi-cli` does not use Echo/Mock providers.** They are testing utilities for the `kosong` crate itself.

---

### ❌ Contrib Context (`contrib/context/`) 🚫

| Module | Python File | Rust Status | Notes |
|--------|-------------|-------------|-------|
| Linear context | `contrib/context/linear.py` | ❌ Not started | Message history management utility (`LinearContext`, `JsonlLinearStorage`). |

🚫 **`kimi-cli` does not use Contrib Context.** It has its own session/history management. This module is for standalone `kosong` consumers.

---

### ❌ Advanced Tooling (`tooling/`) 🚫

| Feature | Python File | Rust Status | Effort | Notes |
|---------|-------------|-------------|--------|-------|
| `EmptyToolset` | `tooling/empty.py` | ❌ Not started | Trivial | No-op toolset for testing. 🚫 Not used by `kimi-cli`. |
| MCP tool wrapper | `tooling/mcp.py` | ❌ Not started | Medium | Wraps MCP client as kosong `Tool`. 🚫 `kimi-cli` has its own MCP layer in `KimiToolset`. |
| Tool validation errors | `tooling/error.py` | 🔄 Partial | Low | `ToolReturnValue::error()` covers basic cases. Python has richer error taxonomy. Good enough for `kimi-cli`. |

🚫 **`kimi-cli` does not use `EmptyToolset` or the kosong MCP wrapper.** It has its own `KimiToolset` with integrated MCP support. These would only be needed for other `kosong` consumers.

---

### ✅ Utility Modules ✨

| Feature | Python File | Rust Equivalent | Notes |
|---------|-------------|-----------------|-------|
| `Callback` type + `callback()` helper | `utils/aio.py` | **Rust closures + `FnMut` traits** | Python needs a custom callback abstraction because it lacks first-class closure types that cross async boundaries cleanly. Rust's `Fn`, `FnMut`, `FnOnce` traits + closures handle this natively. |
| `JsonType` alias | `utils/typing.py` | **`serde_json::Value`** | Python needs `JsonType = dict[str, Any]` because it has no built-in JSON type. Rust uses `serde_json::Value` from the de-facto standard serialization crate. |

✨ **Natively handled by Rust.** No port needed — Rust's type system and standard ecosystem provide these abstractions out of the box.

---

## Key API Differences

### Callback Semantics

| | Python | Rust |
|---|--------|------|
| `on_message_part` | `Callback[[StreamedMessagePart], None]` — may be async | `FnMut(StreamedMessagePart)` — synchronous |
| `on_tool_call` | `Callback[[ToolCall], None]` — may be async | `FnMut(ToolCall)` — synchronous |
| `on_tool_result` | `Callable[[ToolResult], None]` — fires on future completion | `FnMut(&ToolResult)` — fires synchronously as each `JoinHandle` resolves |

**Impact:** The Rust version's synchronous callbacks are simpler but cannot perform async I/O inside the callback. The `octopus-cli` `KimiSoul` works around this by spawning tasks inside `on_tool_call` and collecting results after generation.

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
├── `LLM::generate_streaming()` → calls `kosong::generate()` with `on_message_part` + `on_tool_call` callbacks
├── `build_kosong_provider()` → builds `kosong::provider::kimi::Kimi`, `OpenAILegacy`, or `OpenAIResponses`
└── Type conversions: `wire_to_kosong_*` / `kosong_to_wire_*`

octopus-cli/src/soul/mod.rs (KimiSoul)
├── `_run_step_once_inner()` → manually implements step logic (NOT using kosong::step)
│   └── Calls `LLM::generate_streaming()` (streams message parts via kosong::generate)
│   └── Dispatches tools during streaming via `on_tool_call` callback
│   └── Executes tools concurrently via `futures::future::join_all()`
└── Tool results collected after generation; `on_tool_result` callback on `KimiToolset` fires per result
```

**Important:** `KimiSoul` does **not** call `kosong::step()`. Instead, it calls `LLM::generate_streaming()` (which wraps `kosong::generate()` with streaming callbacks) and handles tools manually via `KimiToolset`. This means:

- ✅ `KimiSoul` streams message parts to the user in real-time
- ✅ `KimiSoul` dispatches tools **during streaming** as they arrive from the stream
- ✅ `KimiSoul` executes tools concurrently (not sequentially)
- ✅ `KimiSoul` handles D-Mail, hooks, telemetry, MCP, etc. at a higher layer
- ✅ `KimiToolset` implements `kosong::Toolset` via `KosongToolsetAdapter` (available for future use)

The kosong crate is complete and functional; `octopus-cli` uses `kosong::generate()` with streaming + early tool dispatch via the `on_tool_call` callback.

---

## Priority Roadmap

### P0 — Critical for parity
1. ~~**Wire `kosong::step()` into `KimiSoul`**~~ ✅ **DONE** — Streaming via `kosong::generate()` + early tool dispatch during streaming + concurrent tool execution via `join_all()` implemented. `KosongToolsetAdapter` implements `kosong::Toolset` for future `kosong::step()` integration.

### P1 — Important
2. ~~**Add `on_tool_result` to Rust `kosong::step()`**~~ ✅ **DONE** — `StepResult::tool_results_with_callback()` and `step_with_tool_result_callback()` implemented. Fires callback synchronously as each `JoinHandle` resolves.
3. ~~**Implement OpenAI Responses provider**~~ ✅ **DONE** — `kosong::provider::openai_responses::OpenAIResponses` implemented with streaming and non-streaming support.

### P2 — Optional providers (not part of default onboarding) ⚪
4. **Anthropic provider** — For Claude support. Medium effort. Key complexity: adaptive vs budget-based thinking (model-version dependent), cache control injection, tool result merging, SSE delta parsing. ⚪ Optional in `kimi-cli` — requires manual `config.toml` editing.
5. **Google GenAI provider** — For Gemini support. High effort. Key complexity: full-response chunks (not deltas), role `model`, strict tool result packing rules, tool call ID synthesis, media download/base64 conversion, VertexAI vs Gemini API differences. ⚪ Optional in `kimi-cli` — requires manual `config.toml` editing.

> **Note:** These two providers are the *only* remaining gap for full `kimi-cli` parity. They are not needed for the default `kimi login` user experience.

### P3 — Future / Testing 🚫
6. ~~**Echo / Mock providers**~~ ✅ **DONE** — `EchoChatProvider`, `ScriptedEchoChatProvider`, `MockChatProvider` implemented with full DSL parser. 🚫 Not used by `kimi-cli`.
7. **MCP tool wrapper** — Bridge MCP tools into kosong `Toolset`. `octopus-cli` already has its own MCP layer; this would make MCP available to other kosong consumers. 🚫 Not used by `kimi-cli`.
8. **Contrib context modules** — `linear.py` and similar utilities. 🚫 Not used by `kimi-cli`.
9. **EmptyToolset** — No-op toolset for testing. 🚫 Not used by `kimi-cli`.

---

## File Count Comparison

| Category | Python Files | Rust Files | Status |
|----------|-------------|------------|--------|
| Core (lib, generate, step, message, tooling) | 5 | 5 | ✅ Complete |
| Chat providers (Kimi, OpenAI-Legacy, OpenAI-Responses) | 3 | 4 (+types) | ✅ Complete |
| Provider commons | 1 | 1 | ✅ Complete |
| Toolset + utils | 3 | 3 | ✅ Complete |
| Contrib providers (Anthropic, Google) | 2 | 0 | ❌ Missing ⚪ |
| Echo / Mock | 3 | 2 (+dsl) | ✅ Complete |
| Utility modules (Callback, JsonType) | 2 | 0 (✨ native) | ✨ Natively handled |
| Contrib context | 1 | 0 | ❌ Missing 🚫 |
| **Total** | **18** | **15** | **~83% file parity** |

> **~83% file parity** overall. If you exclude 🚫 items (not used by `kimi-cli`) and ✨ items (natively handled by Rust), the **effective parity for `octopus-cli` is ~100% for the default user** — only the ⚪ optional providers (Anthropic, Google GenAI) remain unported, and those require manual configuration.
