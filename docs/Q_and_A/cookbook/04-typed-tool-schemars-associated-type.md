# 04 — Eliminate Schema/Parser Drift with `schemars` + Associated Types

## Problem

A tool registry stores heterogeneous tools behind `dyn CallableTool`. Each tool currently exposes:

- `fn parameters(&self) -> Value` — hand-written JSON Schema for the LLM
- `async fn call_raw(&self, arguments: Value) -> ToolReturnValue` — manual `serde_json::from_value`

This creates **two parallel descriptions** of the same parameter shape. If you add a field to the Rust struct but forget to update the JSON, the LLM never sends it and deserialization fails at runtime. Rename a field in one place but not the other and you get a silent mismatch.

## Constraints

### 1. The registry must store heterogeneous tools

A `KimiToolset` holds `ShellTool`, `ReadFileTool`, `AgentTool`, and potentially third-party `WasmPluginTool`s side by side. In Rust, the only way to store values of different types in a single collection is through **type erasure** — typically `Box<dyn Trait>`. This means the trait used for storage must be **object-safe**.

An object-safe trait has two key restrictions:
- Its methods cannot be generic (no `<T>` parameters).
- It cannot have associated types.

Why? Because the compiler must generate a single vtable for `dyn CallableTool`. If a method were generic, the compiler would need to monomorphize it for every possible type `T` — impossible when the concrete type is erased behind `dyn`. If the trait had an associated type, the compiler wouldn't know what `type Params = ???` to use for the vtable entry.

This is why we cannot simply write:

```rust
// DOES NOT COMPILE — CallableTool2 is not object-safe
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn CallableTool2>>, // ❌ error
}
```

The associated `type Params` makes `CallableTool2` non-object-safe. We need a separate, object-safe `CallableTool` trait for storage.

### 2. Associated types are the only way to link schema and parser

We want the **same type** to generate the JSON Schema *and* deserialize the incoming arguments. The natural Rust mechanism for an associated type:

```rust
trait CallableTool2 {
    type Params: DeserializeOwned + JsonSchema;
}
```

If we tried to avoid associated types (e.g., by making `parameters()` generic over `T: JsonSchema`), we would push the type parameter onto the trait itself, which again breaks object safety. If we tried to use `serde_json::Value` everywhere, we sacrifice the compile-time link we are trying to achieve.

So we are forced into a two-trait design: one trait with the associated type (for type safety), and one without (for object safety).

### 3. Why not a blanket impl?

You might wonder: can we write `impl<T: CallableTool2> CallableTool for T` and skip the adapter struct entirely?

```rust
// Possible, but problematic
impl<T: CallableTool2> CallableTool for T { ... }
```

This creates an **orphan rule hazard**: any type that already implements `CallableTool` directly (e.g., `WasmPluginTool`) would conflict if it later gained a `CallableTool2` impl, or vice versa. It also makes the type erasure implicit rather than explicit — a new developer cannot tell by looking at `ShellTool` whether it is stored as `dyn CallableTool` or `dyn CallableTool2`. The explicit `CallableTool2Adapter` makes the boundary visible.

More importantly, a blanket impl forces **every** `CallableTool2` to also be a `CallableTool`, which means you cannot have a type that is *only* `CallableTool2` and is intended to be wrapped differently. The adapter keeps the two traits decoupled.

### 4. Dynamic tools cannot use compile-time schema generation

Not every tool has a Rust struct at compile time:

- **WASM plugins** load a `.wasm` file at runtime. The schema comes from the plugin's `tool_metadata` export or a sidecar JSON manifest. There is no Rust struct to derive `JsonSchema` on.
- **MCP tools** discover their schemas by talking to an MCP server over stdio at startup. The schema is runtime data.
- **Wire-external tools** are registered dynamically by an external client over the wire protocol. Their schemas arrive as arbitrary JSON.

These tools **must** continue to implement `CallableTool` directly and return a runtime `Value` from `parameters()`. We cannot force the entire ecosystem into `CallableTool2`.

This means our design must support **both** paths simultaneously: typed tools for first-party code, and raw `CallableTool` impls for dynamic/external tools.

### 5. Schema semantics

`llm_provider::CallableTool::parameters()` returns **only** the inner parameters JSON Schema:

```json
{
  "type": "object",
  "properties": { ... }
}
```

`llm_provider::Tool` already has separate `name` and `description` fields, so there is no double-wrapping. This is the correct semantic shape.

## Solution

Use llm-provider's built-in two-layer design:

1. **`CallableTool`** — the object-safe trait the registry stores. `parameters()` returns **only** the parameters JSON Schema.
2. **`CallableTool2`** — a developer-facing trait with an associated `Params` type bounded by `DeserializeOwned + JsonSchema + Send`. It is never stored directly.
3. **`CallableTool2Adapter<T>`** — a concrete generic struct that bridges `CallableTool2` → `CallableTool`, mechanically deriving the schema and handling deserialization.

```rust
use llm_provider::async_trait;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Object-safe boundary (what the registry stores)
// ---------------------------------------------------------------------------
#[async_trait]
pub trait CallableTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// MUST return only the JSON Schema object (type/object/properties).
    fn parameters(&self) -> Value;
    async fn call_raw(&self, arguments: Value) -> ToolReturnValue;
}

// ---------------------------------------------------------------------------
// Developer-facing trait (not object-safe because of associated type)
// ---------------------------------------------------------------------------
#[async_trait]
pub trait CallableTool2: Send + Sync {
    type Params: DeserializeOwned + JsonSchema + Send;

    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn call_typed(&self, params: Self::Params) -> ToolReturnValue;
}

// ---------------------------------------------------------------------------
// Type-erasing bridge
// ---------------------------------------------------------------------------
pub struct CallableTool2Adapter<T: CallableTool2> {
    pub inner: T,
}

impl<T: CallableTool2> CallableTool2Adapter<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<T: CallableTool2> CallableTool for CallableTool2Adapter<T> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        // THE GUARANTEE: schema is mechanically derived from Self::Params.
        let schema = schemars::schema_for!(T::Params);
        serde_json::to_value(schema).unwrap_or(Value::Null)
    }

    async fn call_raw(&self, arguments: Value) -> ToolReturnValue {
        // THE GUARANTEE: parse target is the same type used for schema generation.
        let params: T::Params = serde_json::from_value(arguments)
            .map_err(|e| format!("Invalid parameters: {e}"))?;
        self.inner.call_typed(params).await
    }
}
```

## What a tool looks like now

```rust
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ShellParams {
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

pub struct ShellTool { /* ... */ }

#[async_trait]
impl CallableTool2 for ShellTool {
    type Params = ShellParams;

    fn name(&self) -> &str { "Shell" }
    fn description(&self) -> &str { "Execute a shell command." }

    async fn call_typed(&self, params: ShellParams) -> ToolReturnValue {
        // `params` is already typed — no manual Value parsing.
        if params.command.is_empty() {
            return ToolReturnValue::error("Command cannot be empty.".to_string());
        }
        // ...
        ToolReturnValue::ok(output)
    }
}
```

Registration:

```rust
toolset.register_typed(ShellTool::new(bg_manager));
```

`KimiToolset::register_typed` wraps the tool in `CallableTool2Adapter` automatically:

```rust
pub fn register_typed<T: llm_provider::tooling::CallableTool2 + 'static>(&mut self, tool: T) {
    self.register(Box::new(llm_provider::tooling::CallableTool2Adapter::new(tool)));
}
```

## Why this works

### Compile-time guarantees

| Failure mode | Before | After |
|-------------|--------|-------|
| Add field to struct, forget schema | Runtime: LLM never sends it | Impossible — schema is derived |
| Rename field in struct only | Runtime: parse failure | Compile error in adapter (type mismatch) |
| Wrong type in schema | Runtime: parse failure | `JsonSchema` derive generates matching type |
| Forgot `required` update | Runtime: missing field | `schemars` respects `#[serde(default)]` automatically |

### Dynamic tools still work

WASM plugins, MCP tools, and wire-external tools implement `CallableTool` directly because their schemas are runtime data. They are unaffected:

```rust
#[async_trait]
impl llm_provider::tooling::CallableTool for WasmPluginTool {
    fn parameters(&self) -> Value {
        // From plugin manifest — already parameters-only JSON Schema.
        self.schema.clone()
    }
    // ...
}
```

### Registry stays unchanged

Callers using `Box<dyn CallableTool>` or iterating over `&dyn CallableTool` require zero changes. The adapter is invisible at the registry boundary.

## Migration checklist

1. Add `schemars::JsonSchema` to every params struct derive.
2. Replace `impl Tool for X` with `impl CallableTool2 for X` (from `llm_provider::tooling`).
3. Add `type Params = YourParams;`.
4. Rename `call(&self, arguments: Value)` → `call_typed(&self, params: Self::Params)`.
5. Change return type from `Result<String, String>` to `ToolReturnValue` (use `ToolReturnValue::ok(...)` / `::error(...)`).
6. Delete the hand-written `parameters()` method and the `serde_json::from_value` line.
7. At registration sites, use `register_typed(tool)` or wrap with `CallableTool2Adapter::new(tool)`.
8. Ensure dynamic tools return **only** the parameters JSON Schema from `parameters()`.

## Trade-offs

| Aspect | Cost |
|--------|------|
| Derive macro | Every params struct needs `#[derive(JsonSchema)]` |
| Adapter wrapper | One `CallableTool2Adapter::new(...)` per registration (hidden by `register_typed`) |
| Extra dependency | `schemars` (already present in this project) |
| `'static` bound | `register_typed<T: CallableTool2 + 'static>` because `dyn CallableTool` requires it |
| Schema shape | `schemars::schema_for!()` emits `$schema`, `title`, etc. — the LLM provider ignores extras |

## Related

- [03-newtype-adapter-enforce-construction-invariant.md](./03-newtype-adapter-enforce-construction-invariant.md) — the newtype wrapper technique used for `KimiToolsetHandle`.
- `llm-provider/src/tooling.rs` — the llm-provider crate owns `CallableTool`, `CallableTool2`, and `CallableTool2Adapter`.
