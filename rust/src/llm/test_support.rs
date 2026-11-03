use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::{LlmProvider, LlmResponse, TokenStream};

/// Test double: returns scripted answers in order and counts `complete` calls.
pub struct ScriptedLlm {
    pub answers: Mutex<Vec<&'static str>>,
    pub calls: AtomicUsize,
}

impl ScriptedLlm {
    pub fn new(answers: Vec<&'static str>) -> Self {
        Self { answers: Mutex::new(answers), calls: AtomicUsize::new(0) }
    }
}

#[async_trait::async_trait]
impl LlmProvider for ScriptedLlm {
    async fn complete(&self, _p: &str) -> anyhow::Result<LlmResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let a = self.answers.lock().unwrap().remove(0);
        Ok(LlmResponse { answer: a.into(), input_tokens: 0, output_tokens: 0 })
    }
    async fn complete_stream(&self, _p: &str) -> anyhow::Result<TokenStream> {
        unimplemented!()
    }
}
