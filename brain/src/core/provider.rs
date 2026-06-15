use std::sync::Arc;

use async_trait::async_trait;
use kosong::provider::openai_legacy::OpenAILegacy;

use crate::core::config::BrainConfig;
use crate::core::errors::BrainError;

/// Builds a [`kosong::ChatProvider`] for a Brain instance.
///
/// Applications implement this trait to control how the LLM provider is
/// constructed and reconstructed (e.g. after an OAuth token refresh).
#[async_trait]
pub trait ProviderFactory: Send + Sync {
    async fn create(
        &self,
        config: &BrainConfig,
    ) -> Result<Arc<dyn kosong::ChatProvider>, BrainError>;
}

/// Default factory that builds an [`OpenAILegacy`] provider from the
/// `base_url`, `api_key`, and `model` fields of [`BrainConfig`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultProviderFactory;

#[async_trait]
impl ProviderFactory for DefaultProviderFactory {
    async fn create(
        &self,
        config: &BrainConfig,
    ) -> Result<Arc<dyn kosong::ChatProvider>, BrainError> {
        if config.base_url.is_empty() || config.model.is_empty() {
            return Err(BrainError::NoProvider);
        }

        let provider = OpenAILegacy::new(&config.model)
            .with_base_url(&config.base_url)
            .with_stream(false);

        let provider = if config.api_key.is_empty() {
            provider
        } else {
            provider.with_api_key(&config.api_key)
        };

        Ok(Arc::new(provider))
    }
}
