use anyhow::Context;
use async_stream::stream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use super::{LlmProvider, LlmResponse, TokenStream};

const DEFAULT_URL: &str = "https://api.openai.com/v1/chat/completions";

pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: Option<String>,
    url: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
}

impl OpenAiProvider {
    pub fn new(
        api_key: Option<String>,
        base_url: Option<String>,
        model: String,
        max_tokens: u32,
        temperature: f32,
    ) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to build reqwest client"),
            api_key,
            url: chat_completions_url(base_url.as_deref()),
            model,
            max_tokens,
            temperature,
        }
    }

    fn request(&self, body: &OpenAiRequest<'_>) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .json(body);
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        req
    }

    async fn send(&self, body: OpenAiRequest<'_>) -> anyhow::Result<String> {
        let resp = self
            .request(&body)
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
        Ok(text)
    }
}

pub(crate) fn chat_completions_url(base_url: Option<&str>) -> String {
    match base_url {
        None => DEFAULT_URL.to_string(),
        Some(base) => {
            let base = base.trim_end_matches('/');
            if base.ends_with("/chat/completions") {
                base.to_string()
            } else {
                format!("{base}/chat/completions")
            }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

impl<'a> OpenAiRequest<'a> {
    fn new(
        model: &'a str,
        max_tokens: u32,
        temperature: f32,
        prompt: &'a str,
        stream: bool,
        json: bool,
    ) -> Self {
        Self {
            model,
            max_tokens,
            temperature,
            messages: vec![OpenAiMessage {
                role: "user",
                content: prompt,
            }],
            stream: stream.then_some(true),
            response_format: json.then_some(ResponseFormat { r#type: "json_object" }),
        }
    }
}

#[derive(Serialize)]
struct ResponseFormat {
    r#type: &'static str,
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
    #[serde(default, deserialize_with = "null_as_default")]
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
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
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
        let body = OpenAiRequest::new(
            &self.model,
            self.max_tokens,
            self.temperature,
            prompt,
            false,
            false,
        );
        parse_response(&self.send(body).await?)
    }

    async fn complete_json(&self, prompt: &str) -> anyhow::Result<LlmResponse> {
        let body = OpenAiRequest::new(
            &self.model,
            self.max_tokens,
            self.temperature,
            prompt,
            false,
            true,
        );
        parse_response(&self.send(body).await?)
    }

    async fn complete_stream(&self, prompt: &str) -> anyhow::Result<TokenStream> {
        let body = OpenAiRequest::new(
            &self.model,
            self.max_tokens,
            self.temperature,
            prompt,
            true,
            false,
        );

        let resp = self
            .request(&body)
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

    use std::sync::{Arc, Mutex};

    type Captured = Arc<Mutex<Option<(Option<String>, serde_json::Value)>>>;

    async fn spawn_fake_openai() -> (String, Captured) {
        use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};

        let captured: Captured = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route(
                "/v1/chat/completions",
                post(
                    |State(cap): State<Captured>,
                     headers: HeaderMap,
                     Json(body): Json<serde_json::Value>| async move {
                        let auth = headers
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .map(String::from);
                        *cap.lock().unwrap() = Some((auth, body));
                        Json(serde_json::json!({
                            "choices": [{"message": {"content": "{\"ok\":true}"}}]
                        }))
                    },
                ),
            )
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://127.0.0.1:{port}/v1"), captured)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn complete_json_hits_base_url_without_auth_header_when_no_key() {
        let (base_url, captured) = spawn_fake_openai().await;
        let p = OpenAiProvider::new(None, Some(base_url), "qwen".into(), 64, 0.0);
        let r = p.complete_json("hi").await.unwrap();
        assert_eq!(r.answer, "{\"ok\":true}");
        let (auth, body) = captured.lock().unwrap().clone().unwrap();
        assert!(auth.is_none());
        assert_eq!(body["model"], "qwen");
        assert_eq!(body["response_format"]["type"], "json_object");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn complete_sends_bearer_when_key_given() {
        let (base_url, captured) = spawn_fake_openai().await;
        let p = OpenAiProvider::new(Some("k1".into()), Some(base_url), "m".into(), 64, 0.0);
        p.complete("hi").await.unwrap();
        let (auth, body) = captured.lock().unwrap().clone().unwrap();
        assert_eq!(auth.as_deref(), Some("Bearer k1"));
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn url_defaults_to_openai() {
        assert_eq!(chat_completions_url(None), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn url_joins_base_without_double_slash() {
        assert_eq!(
            chat_completions_url(Some("http://ollama:11434/v1/")),
            "http://ollama:11434/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url(Some("http://ollama:11434/v1")),
            "http://ollama:11434/v1/chat/completions"
        );
    }

    #[test]
    fn url_keeps_full_path_if_already_given() {
        assert_eq!(
            chat_completions_url(Some("http://x/v1/chat/completions")),
            "http://x/v1/chat/completions"
        );
    }

    #[test]
    fn parse_openai_response_without_usage() {
        let json = r#"{"choices": [{"message": {"content": "Hi"}}]}"#;
        let r = parse_response(json).unwrap();
        assert_eq!(r.answer, "Hi");
        assert_eq!(r.input_tokens, 0);
    }

    #[test]
    fn parse_openai_response_with_null_usage() {
        let json = r#"{"choices": [{"message": {"content": "Hi"}}], "usage": null}"#;
        assert_eq!(parse_response(json).unwrap().answer, "Hi");
    }

    #[test]
    fn json_request_body_has_response_format() {
        let body = OpenAiRequest::new("m", 10, 0.1, "p", false, true);
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["response_format"]["type"], "json_object");
        let body = OpenAiRequest::new("m", 10, 0.1, "p", false, false);
        let v = serde_json::to_value(&body).unwrap();
        assert!(v.get("response_format").is_none());
    }
}
