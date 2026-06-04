use anyhow::Context;
use async_stream::stream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use super::{LlmProvider, LlmResponse, TokenStream};

pub struct ClaudeProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
}

impl ClaudeProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32, temperature: f32) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            max_tokens,
            temperature,
        }
    }
}

// ----- Request types -----

#[derive(Serialize)]
struct ClaudeRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: f32,
    messages: Vec<ClaudeMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct ClaudeMessage<'a> {
    role: &'a str,
    content: &'a str,
}

// ----- Response types -----

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
    #[serde(default)]
    usage: ClaudeUsage,
}

#[derive(Deserialize)]
struct ClaudeContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Deserialize, Default)]
struct ClaudeUsage {
    input_tokens: u32,
    output_tokens: u32,
}

// ----- SSE streaming types -----

#[derive(Deserialize)]
struct ClaudeStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<ClaudeStreamDelta>,
}

#[derive(Deserialize)]
struct ClaudeStreamDelta {
    #[serde(rename = "type")]
    delta_type: String,
    text: Option<String>,
}

// ----- Error response -----

#[derive(Deserialize)]
struct ClaudeErrorResponse {
    error: ClaudeErrorDetail,
}

#[derive(Deserialize)]
struct ClaudeErrorDetail {
    message: String,
}

// ----- Parse helpers (pub(crate) for unit testing) -----

pub(crate) fn parse_response(json: &str) -> anyhow::Result<LlmResponse> {
    let resp: ClaudeResponse =
        serde_json::from_str(json).context("failed to parse Claude response")?;

    let answer = resp
        .content
        .into_iter()
        .filter(|c| c.content_type == "text")
        .filter_map(|c| c.text)
        .collect::<Vec<_>>()
        .join("");

    Ok(LlmResponse {
        answer,
        input_tokens: resp.usage.input_tokens,
        output_tokens: resp.usage.output_tokens,
    })
}

#[async_trait::async_trait]
impl LlmProvider for ClaudeProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<LlmResponse> {
        let body = ClaudeRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            messages: vec![ClaudeMessage {
                role: "user",
                content: prompt,
            }],
            stream: None,
        };

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Claude API request failed")?;

        let status = resp.status();
        let text = resp.text().await.context("reading Claude API response body")?;

        if !status.is_success() {
            let msg = serde_json::from_str::<ClaudeErrorResponse>(&text)
                .map(|e| e.error.message)
                .unwrap_or_else(|_| text.clone());
            return Err(anyhow::anyhow!("Claude API error {status}: {msg}"));
        }

        parse_response(&text)
    }

    async fn complete_stream(&self, prompt: &str) -> anyhow::Result<TokenStream> {
        let body = ClaudeRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            messages: vec![ClaudeMessage {
                role: "user",
                content: prompt,
            }],
            stream: Some(true),
        };

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Claude streaming API request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.context("reading Claude API error body")?;
            let msg = serde_json::from_str::<ClaudeErrorResponse>(&text)
                .map(|e| e.error.message)
                .unwrap_or(text);
            return Err(anyhow::anyhow!("Claude API error {status}: {msg}"));
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
                            match serde_json::from_str::<ClaudeStreamEvent>(data) {
                                Ok(ev) => {
                                    if ev.event_type == "content_block_delta" {
                                        if let Some(delta) = ev.delta {
                                            if delta.delta_type == "text_delta" {
                                                if let Some(text) = delta.text {
                                                    yield Ok(text);
                                                }
                                            }
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
                        match serde_json::from_str::<ClaudeStreamEvent>(data) {
                            Ok(ev) => {
                                if ev.event_type == "content_block_delta" {
                                    if let Some(delta) = ev.delta {
                                        if delta.delta_type == "text_delta" {
                                            if let Some(text) = delta.text {
                                                yield Ok(text);
                                            }
                                        }
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
    fn parse_claude_response() {
        let json = r#"{"content": [{"type": "text", "text": "Hello"}], "usage": {"input_tokens": 10, "output_tokens": 5}}"#;
        let result = parse_response(json).expect("should parse successfully");
        assert_eq!(result.answer, "Hello");
        assert_eq!(result.input_tokens, 10);
        assert_eq!(result.output_tokens, 5);
    }

    #[test]
    fn parse_claude_response_multiple_content_blocks() {
        let json = r#"{"content": [{"type": "text", "text": "Hello"}, {"type": "text", "text": " World"}], "usage": {"input_tokens": 15, "output_tokens": 8}}"#;
        let result = parse_response(json).expect("should parse successfully");
        assert_eq!(result.answer, "Hello World");
        assert_eq!(result.input_tokens, 15);
        assert_eq!(result.output_tokens, 8);
    }

    #[test]
    fn parse_claude_response_skips_non_text_blocks() {
        let json = r#"{"content": [{"type": "tool_use", "text": null}, {"type": "text", "text": "Answer"}], "usage": {"input_tokens": 20, "output_tokens": 3}}"#;
        let result = parse_response(json).expect("should parse successfully");
        assert_eq!(result.answer, "Answer");
    }

    #[test]
    fn parse_claude_response_invalid_json() {
        let result = parse_response("not json");
        assert!(result.is_err());
    }
}
