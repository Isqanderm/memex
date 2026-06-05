# Task 9: LLM Client

**Goal:** LlmProvider трейт с синхронным/стриминговым интерфейсом, реализации для Claude и OpenAI через plain reqwest HTTP (без SDK).

**Files:**
- Create: `rust/src/llm/mod.rs`
- Create: `rust/src/llm/claude.rs`
- Create: `rust/src/llm/openai.rs`

---

### Task 9.1: LlmProvider трейт

- [ ] **Шаг 1: Создать rust/src/llm/mod.rs**

```rust
use std::pin::Pin;
use futures::Stream;

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub answer: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub type TokenStream = Pin<Box<dyn Stream<Item = anyhow::Result<String>> + Send>>;

/// Трейт LLM провайдера — аналог Python LLMProvider Protocol.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> anyhow::Result<LlmResponse>;
    async fn complete_stream(&self, prompt: &str) -> anyhow::Result<TokenStream>;
}

pub mod claude;
pub mod openai;

pub use claude::ClaudeProvider;
pub use openai::OpenAIProvider;

/// Добавить async-trait в Cargo.toml:
/// async-trait = "0.1"
```

Добавить в `rust/Cargo.toml`:
```toml
async-trait = "0.1"
```

---

### Task 9.2: Claude Provider

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/llm/claude.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Требует реальный ANTHROPIC_API_KEY
    async fn complete_returns_answer() {
        let provider = ClaudeProvider::new(
            &std::env::var("ANTHROPIC_API_KEY").unwrap(),
            "claude-haiku-4-5-20251001",
            256,
            0.1,
        );
        let result = provider.complete("Say 'hello' in one word.").await.unwrap();
        assert!(!result.answer.is_empty());
        assert!(result.input_tokens > 0);
        assert!(result.output_tokens > 0);
    }
}
```

- [ ] **Шаг 2: Реализовать claude.rs**

```rust
use async_stream::stream;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{LlmProvider, LlmResponse, TokenStream};

pub struct ClaudeProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
}

impl ClaudeProvider {
    pub fn new(api_key: &str, model: &str, max_tokens: u32, temperature: f32) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            max_tokens,
            temperature,
        }
    }
}

#[derive(Serialize)]
struct ClaudeRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: f32,
    messages: Vec<Message<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ContentBlock>,
    usage: Usage,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Deserialize)]
struct StreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<Delta>,
    usage: Option<Usage>,
    message: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Delta {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

#[async_trait::async_trait]
impl LlmProvider for ClaudeProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<LlmResponse> {
        let body = ClaudeRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            messages: vec![Message { role: "user", content: prompt }],
            stream: None,
        };

        let resp = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Claude API error {status}: {text}");
        }

        let data: ClaudeResponse = resp.json().await?;
        let answer = data.content
            .into_iter()
            .filter(|b| b.kind == "text")
            .filter_map(|b| b.text)
            .collect::<Vec<_>>()
            .join("");

        Ok(LlmResponse {
            answer,
            input_tokens: data.usage.input_tokens,
            output_tokens: data.usage.output_tokens,
        })
    }

    async fn complete_stream(&self, prompt: &str) -> anyhow::Result<TokenStream> {
        let body = ClaudeRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            messages: vec![Message { role: "user", content: prompt }],
            stream: Some(true),
        };

        let resp = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Claude stream error {status}: {text}");
        }

        let mut byte_stream = resp.bytes_stream();

        let token_stream: TokenStream = Box::pin(stream! {
            let mut buffer = String::new();

            while let Some(chunk) = byte_stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => { yield Err(anyhow::anyhow!(e)); break; }
                };
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // SSE: события разделены двойным переносом строки
                while let Some(pos) = buffer.find("\n\n") {
                    let event_str = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    for line in event_str.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" { return; }
                            if let Ok(event) = serde_json::from_str::<StreamEvent>(data) {
                                if event.event_type == "content_block_delta" {
                                    if let Some(delta) = event.delta {
                                        if delta.kind.as_deref() == Some("text_delta") {
                                            if let Some(text) = delta.text {
                                                yield Ok(text);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(token_stream)
    }
}

#[cfg(test)]
mod tests { /* из Шага 1 */ }
```

---

### Task 9.3: OpenAI Provider

- [ ] **Шаг 1: Реализовать openai.rs**

```rust
use async_stream::stream;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{LlmProvider, LlmResponse, TokenStream};

pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
}

impl OpenAIProvider {
    pub fn new(api_key: &str, model: &str, max_tokens: u32, temperature: f32) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            max_tokens,
            temperature,
        }
    }
}

#[derive(Serialize)]
struct OpenAIRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: f32,
    messages: Vec<OAIMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct OAIMessage<'a> { role: &'a str, content: &'a str }

#[derive(Deserialize)]
struct OAIResponse {
    choices: Vec<OAIChoice>,
    usage: OAIUsage,
}

#[derive(Deserialize)]
struct OAIChoice {
    message: OAIMessage2,
}

#[derive(Deserialize)]
struct OAIMessage2 { content: String }

#[derive(Deserialize)]
struct OAIUsage { prompt_tokens: u32, completion_tokens: u32 }

#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct StreamDelta {
    content: Option<String>,
}

#[async_trait::async_trait]
impl LlmProvider for OpenAIProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<LlmResponse> {
        let body = OpenAIRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            messages: vec![OAIMessage { role: "user", content: prompt }],
            stream: None,
        };

        let resp = self.client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error {status}: {text}");
        }

        let data: OAIResponse = resp.json().await?;
        let answer = data.choices.into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        Ok(LlmResponse {
            answer,
            input_tokens: data.usage.prompt_tokens,
            output_tokens: data.usage.completion_tokens,
        })
    }

    async fn complete_stream(&self, prompt: &str) -> anyhow::Result<TokenStream> {
        let body = OpenAIRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            messages: vec![OAIMessage { role: "user", content: prompt }],
            stream: Some(true),
        };

        let resp = self.client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI stream error {status}: {text}");
        }

        let mut byte_stream = resp.bytes_stream();

        let token_stream: TokenStream = Box::pin(stream! {
            let mut buffer = String::new();

            while let Some(chunk) = byte_stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => { yield Err(anyhow::anyhow!(e)); break; }
                };
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(pos) = buffer.find("\n\n") {
                    let event = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    for line in event.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" { return; }
                            if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                                for choice in chunk.choices {
                                    if choice.finish_reason.is_some() { return; }
                                    if let Some(text) = choice.delta.content {
                                        yield Ok(text);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(token_stream)
    }
}
```

- [ ] **Шаг 2: Фабрика провайдеров**

```rust
// Добавить в rust/src/llm/mod.rs:

use crate::config::{Config, LlmProviderKind};

pub fn create_llm_provider(config: &Config) -> anyhow::Result<Arc<dyn LlmProvider>> {
    use std::sync::Arc;
    match &config.llm_provider {
        LlmProviderKind::Claude => {
            let key = config.anthropic_api_key.as_ref()
                .ok_or_else(|| anyhow::anyhow!("ANTHROPIC_API_KEY required"))?;
            Ok(Arc::new(ClaudeProvider::new(key, &config.llm_model, config.llm_max_tokens, config.llm_temperature)))
        }
        LlmProviderKind::OpenAI => {
            let key = config.openai_llm_api_key.as_ref()
                .ok_or_else(|| anyhow::anyhow!("OPENAI_LLM_API_KEY required"))?;
            Ok(Arc::new(OpenAIProvider::new(key, &config.llm_model, config.llm_max_tokens, config.llm_temperature)))
        }
    }
}
```

- [ ] **Шаг 3: Проверить компиляцию**

```bash
cd rust && cargo build 2>&1 | tail -5
```

Ожидаем: `Finished` без ошибок.

- [ ] **Шаг 4: Коммит**

```bash
git add rust/src/llm/
git commit -m "feat(rust): LLM клиент — Claude и OpenAI через reqwest (streaming SSE)"
```
