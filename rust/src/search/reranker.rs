//! Reranker wrapping fastembed's BGE Reranker Base ONNX model.
//!
//! Given a query and a list of candidate documents, `Reranker::rerank` returns
//! the documents sorted by relevance score (highest first), limited to `top_n`.

use std::sync::Mutex;

use anyhow::Context;
use fastembed::{RerankInitOptions, RerankerModel, TextRerank};

/// A cross-encoder reranker using BAAI/bge-reranker-base.
///
/// The inner model requires `&mut self` for inference, so it is protected by a
/// [`Mutex`].
pub struct Reranker {
    model: Mutex<TextRerank>,
}

/// A single reranking result.
#[derive(Debug, Clone)]
pub struct RerankResult {
    /// Position of the document in the original input slice.
    pub original_index: usize,
    /// Relevance score (higher = more relevant).
    pub score: f32,
}

impl Reranker {
    /// Load the BGE Reranker Base model.
    ///
    /// The ONNX model (~200 MB) is downloaded on first use and cached in
    /// `~/.cache/`.
    pub fn new() -> anyhow::Result<Self> {
        let model = TextRerank::try_new(
            RerankInitOptions::new(RerankerModel::BGERerankerBase)
                .with_show_download_progress(false),
        )
        .context("Failed to load BGERerankerBase model")?;

        Ok(Self {
            model: Mutex::new(model),
        })
    }

    /// Rerank `documents` with respect to `query`.
    ///
    /// Returns up to `top_n` results in descending score order (most relevant
    /// first).  If `top_n` is 0 or larger than `documents.len()`, all
    /// documents are returned.
    pub fn rerank(
        &self,
        query: &str,
        documents: &[&str],
        top_n: usize,
    ) -> anyhow::Result<Vec<RerankResult>> {
        let mut guard = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("Reranker: model mutex is poisoned"))?;

        // fastembed's rerank sorts by score descending already.
        let raw: Vec<fastembed::RerankResult> = guard
            .rerank(query, documents, false, None)
            .context("TextRerank::rerank failed")?;

        let limit = if top_n == 0 || top_n > raw.len() {
            raw.len()
        } else {
            top_n
        };

        let results = raw
            .into_iter()
            .take(limit)
            .map(|r| RerankResult {
                original_index: r.index,
                score: r.score,
            })
            .collect();

        Ok(results)
    }
}

// -------------------------------------------------------------------------- //
// Tests
// -------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------ //
    // Integration tests — download ~200 MB ONNX model on first run.
    // Run with: cargo test -- --ignored
    // ------------------------------------------------------------------ //

    /// Verifies that the most relevant document ranks first.
    #[test]
    #[ignore]
    fn most_relevant_document_ranks_first() {
        let reranker = Reranker::new().expect("Failed to create reranker");

        let query = "What is the capital of France?";
        let documents = vec![
            "Berlin is the capital of Germany.",
            "Paris is the capital of France.",
            "Tokyo is the capital of Japan.",
        ];

        let results = reranker
            .rerank(query, &documents, 3)
            .expect("rerank failed");

        assert_eq!(results.len(), 3);

        // The document about Paris should have the highest score (index 1).
        let top = &results[0];
        assert_eq!(
            top.original_index, 1,
            "Expected 'Paris is the capital of France.' to rank first, \
             but got original_index={}",
            top.original_index
        );

        // Scores should be in descending order.
        for window in results.windows(2) {
            assert!(
                window[0].score >= window[1].score,
                "Results are not sorted by score descending"
            );
        }
    }

    /// Verifies that top_n correctly limits the number of results.
    #[test]
    #[ignore]
    fn top_n_limits_results() {
        let reranker = Reranker::new().expect("Failed to create reranker");

        let query = "machine learning";
        let documents = vec!["doc 1", "doc 2", "doc 3", "doc 4", "doc 5"];

        let results = reranker
            .rerank(query, &documents, 2)
            .expect("rerank failed");

        assert_eq!(results.len(), 2, "Expected top_n=2 to return 2 results");
    }
}
