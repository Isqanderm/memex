use std::pin::Pin;
use std::sync::Arc;
use futures::Stream;

mod claude;
mod openai;

pub use claude::ClaudeProvider;
pub use openai::OpenAiProvider;

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub answer: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub type TokenStream = Pin<Box<dyn Stream<Item = anyhow::Result<String>> + Send>>;

#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> anyhow::Result<LlmResponse>;
    async fn complete_stream(&self, prompt: &str) -> anyhow::Result<TokenStream>;
}

pub fn create_llm_provider(config: &crate::config::Config) -> anyhow::Result<Arc<dyn LlmProvider>> {
    use crate::config::LlmProviderKind;

    match &config.llm_provider {
        LlmProviderKind::Claude => {
            let api_key = config.anthropic_api_key.clone().ok_or_else(|| {
                anyhow::anyhow!("ANTHROPIC_API_KEY is required for Claude provider")
            })?;
            Ok(Arc::new(ClaudeProvider::new(
                api_key,
                config.llm_model.clone(),
                config.llm_max_tokens,
                config.llm_temperature,
            )))
        }
        LlmProviderKind::OpenAI => {
            let api_key = config.openai_llm_api_key.clone().ok_or_else(|| {
                anyhow::anyhow!("OPENAI_LLM_API_KEY is required for OpenAI provider")
            })?;
            Ok(Arc::new(OpenAiProvider::new(
                api_key,
                config.llm_model.clone(),
                config.llm_max_tokens,
                config.llm_temperature,
            )))
        }
    }
}
