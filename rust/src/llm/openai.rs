use anyhow::Context;
use async_stream::stream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use super::{LlmProvider, LlmResponse, TokenStream};

pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
}

impl OpenAiProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32, temperature: f32) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to build reqwest client"),
            api_key,
            model,
            max_tokens,
            temperature,
        }
    }
}

// ----- Request types -----

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: f32,
    messages: Vec<OpenAiMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

// ----- Response types -----

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: OpenAiUsage,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessageContent,
}

#[derive(Deserialize)]
struct OpenAiMessageContent {
    content: Option<String>,
}

#[derive(Deserialize, Default)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

// ----- SSE streaming types -----

#[derive(Deserialize)]
struct OpenAiStreamChunk {
    choices: Vec<OpenAiStreamChoice>,
}

#[derive(Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiStreamDelta,
}

#[derive(Deserialize)]
struct OpenAiStreamDelta {
    content: Option<String>,
}

// ----- Error response -----

#[derive(Deserialize)]
struct OpenAiErrorResponse {
    error: OpenAiErrorDetail,
}

#[derive(Deserialize)]
struct OpenAiErrorDetail {
    message: String,
}

// ----- Parse helpers (pub(crate) for unit testing) -----

pub(crate) fn parse_response(json: &str) -> anyhow::Result<LlmResponse> {
    let resp: OpenAiResponse =
        serde_json::from_str(json).context("failed to parse OpenAI response")?;

    let answer = resp
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_default();

    Ok(LlmResponse {
        answer,
        input_tokens: resp.usage.prompt_tokens,
        output_tokens: resp.usage.completion_tokens,
    })
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<LlmResponse> {
        let body = OpenAiRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            messages: vec![OpenAiMessage {
                role: "user",
                content: prompt,
            }],
            stream: None,
        };

        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("OpenAI API request failed")?;

        let status = resp.status();
        let text = resp.text().await.context("reading OpenAI API response body")?;

        if !status.is_success() {
            let msg = serde_json::from_str::<OpenAiErrorResponse>(&text)
                .map(|e| e.error.message)
                .unwrap_or_else(|_| text.clone());
            return Err(anyhow::anyhow!("OpenAI API error {status}: {msg}"));
        }

        parse_response(&text)
    }

    async fn complete_stream(&self, prompt: &str) -> anyhow::Result<TokenStream> {
        let body = OpenAiRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            messages: vec![OpenAiMessage {
                role: "user",
                content: prompt,
            }],
            stream: Some(true),
        };

        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("OpenAI streaming API request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.context("reading OpenAI API error body")?;
            let msg = serde_json::from_str::<OpenAiErrorResponse>(&text)
                .map(|e| e.error.message)
                .unwrap_or(text);
            return Err(anyhow::anyhow!("OpenAI API error {status}: {msg}"));
        }

        let token_stream: TokenStream = Box::pin(stream! {
            let mut byte_stream = resp.bytes_stream();
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
                            match serde_json::from_str::<OpenAiStreamChunk>(data) {
                                Ok(chunk) => {
                                    for choice in chunk.choices {
                                        if let Some(content) = choice.delta.content {
                                            yield Ok(content);
                                        }
                                    }
                                }
                                Err(_e) => {
                                    // Silently skip non-JSON SSE control messages (comments, keepalives)
                                    // but trace unknown data payloads
                                    if !data.starts_with(':') {
                                        tracing::debug!("skipping unparseable SSE data: {}", &data[..data.len().min(100)]);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Process any remaining data in the buffer after stream ends
            if !buffer.trim().is_empty() {
                for line in buffer.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" { return; }
                        match serde_json::from_str::<OpenAiStreamChunk>(data) {
                            Ok(chunk) => {
                                for choice in chunk.choices {
                                    if let Some(content) = choice.delta.content {
                                        yield Ok(content);
                                    }
                                }
                            }
                            Err(_e) => {
                                if !data.starts_with(':') {
                                    tracing::debug!("skipping unparseable SSE data: {}", &data[..data.len().min(100)]);
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
mod tests {
    use super::*;

    #[test]
    fn parse_openai_response() {
        let json = r#"{"choices": [{"message": {"content": "Hi there"}}], "usage": {"prompt_tokens": 8, "completion_tokens": 3}}"#;
        let result = parse_response(json).expect("should parse successfully");
        assert_eq!(result.answer, "Hi there");
        assert_eq!(result.input_tokens, 8);
        assert_eq!(result.output_tokens, 3);
    }

    #[test]
    fn parse_openai_response_null_content() {
        let json = r#"{"choices": [{"message": {"content": null}}], "usage": {"prompt_tokens": 5, "completion_tokens": 0}}"#;
        let result = parse_response(json).expect("should parse successfully");
        assert_eq!(result.answer, "");
        assert_eq!(result.input_tokens, 5);
        assert_eq!(result.output_tokens, 0);
    }

    #[test]
    fn parse_openai_response_empty_choices() {
        let json = r#"{"choices": [], "usage": {"prompt_tokens": 5, "completion_tokens": 0}}"#;
        let result = parse_response(json).expect("should parse successfully");
        assert_eq!(result.answer, "");
    }

    #[test]
    fn parse_openai_response_invalid_json() {
        let result = parse_response("not json");
        assert!(result.is_err());
    }
}
