//! Reranker wrapping a fastembed cross-encoder ONNX model (multilingual by default).
//!
//! Given a query and a list of candidate documents, `Reranker::rerank` returns
//! the documents sorted by relevance score (highest first), limited to `top_n`.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Context;
use fastembed::{RerankInitOptions, RerankerModel, TextRerank};

/// A cross-encoder reranker (default: jina-reranker-v2-base-multilingual).
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
    /// Load the reranker model named `model_name` from `cache_dir`.
    ///
    /// Supported: `jina-reranker-v2-base-multilingual`, `bge-reranker-v2-m3`,
    /// `bge-reranker-base`. The ONNX files are downloaded into `cache_dir` on
    /// first use; later loads need no network.
    pub fn new(model_name: &str, cache_dir: &Path) -> anyhow::Result<Self> {
        let reranker_model = reranker_name_to_enum(model_name)
            .with_context(|| format!("Unsupported reranker model: {model_name}"))?;

        let model = TextRerank::try_new(
            RerankInitOptions::new(reranker_model)
                .with_cache_dir(cache_dir.to_path_buf())
                .with_show_download_progress(false),
        )
        .with_context(|| {
            format!(
                "Failed to load reranker model: {model_name} (cache dir: {})",
                cache_dir.display()
            )
        })?;

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

pub(crate) fn reranker_name_to_enum(name: &str) -> Option<RerankerModel> {
    match name {
        "jina-reranker-v2-base-multilingual" => Some(RerankerModel::JINARerankerV2BaseMultiligual),
        "bge-reranker-v2-m3" => Some(RerankerModel::BGERerankerV2M3),
        "bge-reranker-base" => Some(RerankerModel::BGERerankerBase),
        _ => None,
    }
}

// -------------------------------------------------------------------------- //
// Tests
// -------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reranker_name_mapping() {
        assert!(matches!(reranker_name_to_enum("jina-reranker-v2-base-multilingual"), Some(RerankerModel::JINARerankerV2BaseMultiligual)));
        assert!(matches!(reranker_name_to_enum("bge-reranker-v2-m3"), Some(RerankerModel::BGERerankerV2M3)));
        assert!(matches!(reranker_name_to_enum("bge-reranker-base"), Some(RerankerModel::BGERerankerBase)));
        assert!(reranker_name_to_enum("nope").is_none());
    }

    fn test_reranker() -> Reranker {
        let dir = match std::env::var("MODEL_CACHE_DIR") {
            Ok(d) => std::path::PathBuf::from(d),
            Err(_) => std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                .join(".cache/fastembed"),
        };
        Reranker::new("jina-reranker-v2-base-multilingual", &dir).expect("Failed to create reranker")
    }

    // ------------------------------------------------------------------ //
    // Integration tests — download the ONNX model (~1.1 GB) on first run.
    // Run with: cargo test -- --ignored
    // ------------------------------------------------------------------ //

    /// Verifies that the most relevant document ranks first.
    #[test]
    #[ignore]
    fn most_relevant_document_ranks_first() {
        let reranker = test_reranker();

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
        let reranker = test_reranker();

        let query = "machine learning";
        let documents = vec!["doc 1", "doc 2", "doc 3", "doc 4", "doc 5"];

        let results = reranker
            .rerank(query, &documents, 2)
            .expect("rerank failed");

        assert_eq!(results.len(), 2, "Expected top_n=2 to return 2 results");
    }
}
