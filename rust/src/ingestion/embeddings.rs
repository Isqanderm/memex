//! Embedding client wrapping fastembed's multilingual-e5-small (384d) ONNX model.
//!
//! E5 models require task-specific prefixes:
//! - Documents/passages: "passage: <text>"
//! - Queries: "query: <text>"

use std::sync::{Arc, Mutex};

use anyhow::Context;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

/// Default model name for multilingual embeddings.
pub const DEFAULT_MODEL: &str = "multilingual-e5-small";

/// Wraps fastembed's [`TextEmbedding`] with E5-style prefix handling.
///
/// The inner model requires `&mut self` for inference, so it is protected by a
/// [`Mutex`] to allow sharing across threads (e.g. from the ingestion pipeline).
#[derive(Clone)]
pub struct EmbeddingClient {
    model: Arc<Mutex<TextEmbedding>>,
    dimensions: usize,
}

impl EmbeddingClient {
    /// Construct a new client for the given model name.
    ///
    /// Supported values: `"multilingual-e5-small"` (the default).
    /// The ONNX model is downloaded on first use and cached in `~/.cache/`.
    pub fn new(model_name: &str) -> anyhow::Result<Self> {
        let embedding_model = model_name_to_enum(model_name)
            .with_context(|| format!("Unsupported embedding model: {model_name}"))?;

        let dimensions = model_dimensions(&embedding_model);

        let model = TextEmbedding::try_new(
            TextInitOptions::new(embedding_model).with_show_download_progress(false),
        )
        .with_context(|| format!("Failed to load embedding model: {model_name}"))?;

        Ok(Self {
            model: Arc::new(Mutex::new(model)),
            dimensions,
        })
    }

    /// Number of dimensions in the output vectors.
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Embed a batch of passage/document texts.
    ///
    /// Each text is prefixed with `"passage: "` as required by E5 models.
    pub fn embed_passages(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let prefixed: Vec<String> = texts.iter().map(|t| add_passage_prefix(t)).collect();
        self.run_embed(&prefixed)
    }

    /// Embed a batch of query texts.
    ///
    /// Each text is prefixed with `"query: "` as required by E5 models.
    pub fn embed_queries(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let prefixed: Vec<String> = texts.iter().map(|t| add_query_prefix(t)).collect();
        self.run_embed(&prefixed)
    }

    /// Embed a single query text (convenience wrapper around [`embed_queries`]).
    pub fn embed_query(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let mut results = self.embed_queries(&[text])?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Model returned no embeddings"))
    }

    // ------------------------------------------------------------------ //
    // Internal helpers
    // ------------------------------------------------------------------ //

    fn run_embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let mut guard = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("EmbeddingClient: model mutex is poisoned"))?;

        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let embeddings = guard
            .embed(refs, None)
            .context("TextEmbedding::embed failed")?;

        Ok(embeddings)
    }
}

// -------------------------------------------------------------------------- //
// Free helper functions (also useful for testing)
// -------------------------------------------------------------------------- //

/// Prepend E5 passage prefix.
pub fn add_passage_prefix(text: &str) -> String {
    format!("passage: {text}")
}

/// Prepend E5 query prefix.
pub fn add_query_prefix(text: &str) -> String {
    format!("query: {text}")
}

fn model_name_to_enum(name: &str) -> Option<EmbeddingModel> {
    match name {
        "multilingual-e5-small" => Some(EmbeddingModel::MultilingualE5Small),
        "multilingual-e5-base" => Some(EmbeddingModel::MultilingualE5Base),
        "multilingual-e5-large" => Some(EmbeddingModel::MultilingualE5Large),
        _ => None,
    }
}

fn model_dimensions(model: &EmbeddingModel) -> usize {
    match model {
        EmbeddingModel::MultilingualE5Small => 384,
        EmbeddingModel::MultilingualE5Base => 768,
        EmbeddingModel::MultilingualE5Large => 1024,
        // Fallback — caller should not reach this for unsupported models
        _ => 384,
    }
}

// -------------------------------------------------------------------------- //
// Tests
// -------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------ //
    // Unit tests (no model download required)
    // ------------------------------------------------------------------ //

    #[test]
    fn passage_prefix_is_added() {
        assert_eq!(add_passage_prefix("Hello"), "passage: Hello");
        assert_eq!(add_passage_prefix(""), "passage: ");
    }

    #[test]
    fn query_prefix_is_added() {
        assert_eq!(add_query_prefix("What is Rust?"), "query: What is Rust?");
    }

    #[test]
    fn model_name_unknown_returns_none() {
        assert!(model_name_to_enum("unknown-model").is_none());
    }

    #[test]
    fn multilingual_e5_small_dimensions() {
        assert_eq!(model_dimensions(&EmbeddingModel::MultilingualE5Small), 384);
    }

    // ------------------------------------------------------------------ //
    // Integration tests — download ~200 MB ONNX model on first run.
    // Run with: cargo test -- --ignored
    // ------------------------------------------------------------------ //

    /// Verifies that embed_passages returns 384-dimensional vectors.
    #[test]
    #[ignore]
    fn embed_passages_returns_384_dims() {
        let client = EmbeddingClient::new(DEFAULT_MODEL).expect("Failed to create client");
        assert_eq!(client.dimensions(), 384);

        let texts = vec!["This is a test passage.", "Another passage for testing."];
        let embeddings = client.embed_passages(&texts).expect("embed_passages failed");

        assert_eq!(embeddings.len(), 2);
        for emb in &embeddings {
            assert_eq!(emb.len(), 384, "Expected 384 dimensions");
        }
    }

    /// Verifies that embed_query returns a 384-dimensional normalised vector.
    #[test]
    #[ignore]
    fn embed_query_returns_normalized_vector() {
        let client = EmbeddingClient::new(DEFAULT_MODEL).expect("Failed to create client");
        let vec = client
            .embed_query("What is the capital of France?")
            .expect("embed_query failed");

        assert_eq!(vec.len(), 384);

        // ||v|| should be close to 1.0 for normalised embeddings
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0_f32).abs() < 0.01,
            "Expected unit-norm vector, got norm={norm}"
        );
    }
}
