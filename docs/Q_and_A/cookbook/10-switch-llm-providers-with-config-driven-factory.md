# Cookbook: Switch LLM Providers with a Config-Driven Factory

## Problem

`qqbot-core` originally talked to a single LLM backend through `agent_core::DefaultProviderFactory`, which only understands `base_url`, `api_key`, and `model`. When we tried to migrate the bot from a static Moonshot API key to the user's **kimi-code quota**, we hit three limitations:

1. **kimi-code uses OAuth device-flow credentials**, not a static `sk-...` key. The token lives in `~/.kimi/credentials/kimi-code.json` and must be refreshed.
2. **kimi-code requires extra identity headers** (`User-Agent`, `X-Msh-Platform`, `X-Msh-Device-Id`, etc.) that the generic OpenAI-compatible providers did not need.
3. **We still wanted generic OpenAI-compatible endpoints** (Moonshot, DeepSeek, OpenAI, local proxies) with either an API key or OAuth.

An early refactor introduced a separate `ProviderImplementation` enum that mirrored the config enum one-to-one. That duplication made the code harder to extend: adding a new provider meant updating two enums and keeping their variants in sync.

## Solution

Model the provider choice as a **tagged config enum**, then implement `agent_core::ProviderFactory` to build the concrete `llm_provider::ChatProvider` from that enum.

### 1. Tag the config with a sum type

```rust
// apps/qqbot-core/src/config.rs ~line 106 — LlmProviderConfig
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider_type", rename_all = "snake_case")]
pub enum LlmProviderConfig {
    OpenAiCompatible {
        api_url: String,
        #[serde(flatten)]
        auth: AuthConfig,
    },
    KimiCode {
        api_url: String,
        token_file: String,
        #[serde(flatten)]
        identity: KimiCodeIdentity,
    },
}
```

Authentication and identity are independent concerns, so they live in their own types:

```rust
// apps/qqbot-core/src/config.rs ~line 125 — AuthConfig
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "auth_type", rename_all = "snake_case")]
pub enum AuthConfig {
    ApiKey { api_key: String },
    OAuth { token_file: String },
}

// apps/qqbot-core/src/config.rs ~line 150 — KimiCodeIdentity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiCodeIdentity {
    pub home_dir: String,
    pub version: String,
    pub user_agent_product: String,
}
```

### 2. Implement the factory

```rust
// apps/qqbot-core/src/llm_provider.rs ~line 17 — QqbotProviderFactory
#[derive(Debug, Clone)]
pub struct QqbotProviderFactory {
    provider: LlmProviderConfig,
}

// apps/qqbot-core/src/llm_provider.rs ~line 27 — ProviderFactory impl
#[async_trait]
impl ProviderFactory for QqbotProviderFactory {
    async fn create(
        &self,
        brain_config: &BrainConfig,
    ) -> Result<Arc<dyn llm_provider::ChatProvider>, BrainError> {
        let token = auth_token(&self.provider)
            .await
            .map_err(|e| BrainError::Other(e.to_string()))?;
        let headers = identity_headers(&self.provider)
            .map_err(|e| BrainError::Other(e.to_string()))?;
        let base_url = api_url(&self.provider);

        match &self.provider {
            LlmProviderConfig::KimiCode { .. } => {
                Ok(build_kimi_provider(brain_config, base_url, token, headers))
            }
            LlmProviderConfig::OpenAiCompatible { .. } => {
                Ok(build_openai_legacy_provider(brain_config, base_url, token))
            }
        }
    }
}
```

Independent helpers resolve the bearer token (API key or OAuth), build any extra headers, and pick the base URL. The `match` then dispatches to the correct builder:

```rust
// apps/qqbot-core/src/llm_provider.rs ~line 105 — build_kimi_provider
fn build_kimi_provider(
    brain_config: &BrainConfig,
    base_url: &str,
    token: String,
    headers: HashMap<String, String>,
) -> Arc<dyn llm_provider::ChatProvider> {
    let mut provider = Kimi::new(&brain_config.model)
        .with_base_url(base_url)
        .with_api_key(token)
        .with_stream(false);
    for (name, value) in headers {
        provider = provider.with_header(name, value);
    }
    Arc::new(provider)
}
```

### 3. Let the underlying provider carry custom headers

The `llm_provider::Kimi` provider gained a builder method for arbitrary headers:

```rust
// crates/llm-provider/src/provider/kimi.rs ~line 82 — Kimi::with_header
pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
    self.headers.insert(name.into(), value.into());
    self
}
```

These headers are applied to every outgoing request:

```rust
// crates/llm-provider/src/provider/kimi.rs ~line 240 — request header loop (abbreviated)
for (name, value) in &self.headers {
    req_builder = req_builder.header(name, value);
}
```

### 4. Wire the factory into Brain creation

```rust
// apps/qqbot-core/src/group_brain.rs ~line 124 — GroupBrainManager::create_brain
let provider_factory =
    std::sync::Arc::new(QqbotProviderFactory::new(self.config.llm.provider.clone()));

let mut brain = agent_core::BrainBuilder::default()
    .from_config(config)
    .with_provider_factory(provider_factory)
    .build()
    .await?;
```

`BrainConfig.base_url` and `BrainConfig.api_key` are still populated for compatibility, but the custom factory ignores them and uses the tagged config instead.

## Why This Works

| Concern | Before | After |
|---|---|---|
| Provider selection | `String` / boolean flags + parallel `ProviderImplementation` enum | Tagged `LlmProviderConfig` enum |
| Auth | Only static API key | `AuthConfig::ApiKey` or `AuthConfig::OAuth` |
| Identity headers | Not supported | `KimiCodeIdentity` + `Kimi::with_header` |
| Construction | Hard-coded in `DefaultProviderFactory` | `QqbotProviderFactory` driven by config |
| Extensibility | Update two enums | Add one variant + one match arm |

The factory pattern keeps `agent-core` generic. `Brain` only needs a `dyn ProviderFactory`; it does not know whether the provider is kimi-code, OpenAI, or a local proxy. Because `BrainConfig.build_provider()` calls the factory, the Brain can also rebuild the provider later — for example, after an OAuth token refresh.

## When to Use

- You have **multiple LLM backends** with different authentication or header requirements.
- The core library (`agent-core`, `llm-provider`) should stay provider-agnostic.
- Provider construction requires **async I/O** (OAuth refresh, reading device IDs, etc.).
- You want configuration errors to surface at **deserialization time** via tagged enums.

## When NOT to Use

- You only ever target one provider. `agent_core::DefaultProviderFactory` is simpler.
- Provider construction is synchronous and trivial. A plain builder at the call site is less indirection.
- You want runtime plugin loading of providers from unknown third parties. A trait object registry is more flexible than a closed config enum.

## Relation to Other Patterns

- **Builder pattern**: `Kimi::with_base_url(...).with_api_key(...).with_header(...)` is a builder. The factory decides *which* builder to invoke.
- **Strategy pattern**: `ProviderFactory` is a strategy for constructing a `ChatProvider`. Different applications supply different strategies.
- **Tagged enum config**: `LlmProviderConfig` replaces parallel strings/booleans and makes invalid combinations unrepresentable.

## Real Example from the Codebase

Full implementation lives in `apps/qqbot-core/src/llm_provider.rs` and `apps/qqbot-core/src/config.rs`.

Sample `config.toml` for kimi-code quota:

```toml
# docref: example
[llm]
provider_type = "kimi_code"
model = "kimi-for-coding"
api_url = "https://api.kimi.com/coding/v1/chat/completions"
token_file = "/home/zw/.kimi/credentials/kimi-code.json"
system_prompt = "You are a helpful assistant summarizing a QQ group conversation."
```

Sample config for a generic OpenAI-compatible API key provider:

```toml
# docref: example
[llm]
provider_type = "openai_compatible"
model = "moonshot-v1-8k"
api_url = "https://api.moonshot.cn/v1/chat/completions"
auth_type = "api_key"
api_key = "sk-..."
```

Sample config for a generic OpenAI-compatible provider using OAuth:

```toml
# docref: example
[llm]
provider_type = "openai_compatible"
model = "gpt-4o"
api_url = "https://api.openai.com/v1/chat/completions"
auth_type = "oauth"
token_file = "/path/to/oauth-token.json"
```
