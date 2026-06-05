use std::sync::Arc;

use chrono::Local;
use rusqlite::Connection;

use crate::db::repositories::chunks::ChunkRepository;
use crate::ingestion::embeddings::EmbeddingClient;
use crate::ingestion::language::LanguageDetector;
use crate::llm::LlmProvider;
use crate::search::context::ContextBuilder;
use crate::search::expand::expand_to_l2;
use crate::search::memory_search::MemorySearch;
use crate::search::reranker::Reranker;
use crate::search::rrf::{rrf_merge, SearchHit};
use crate::search::tantivy_fts::{FtsHit, TantivyStore};
use crate::search::vectors::{VectorHit, VectorStore};

/// The result of a retrieval + generation query.
#[derive(Debug)]
pub struct QueryResult {
    pub answer: String,
    pub sources: Vec<serde_json::Value>,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Orchestrates the full retrieval pipeline:
/// embed → vector search → FTS → RRF merge → L2 expand → rerank → memory → LLM.
pub struct RetrievalService {
    pub embed: Arc<EmbeddingClient>,
    pub tantivy: Arc<TantivyStore>,
    pub vectors: Arc<VectorStore>,
    pub reranker: Arc<Reranker>,
    pub llm: Arc<dyn LlmProvider>,
    pub context_builder: ContextBuilder,
    pub memory_search: MemorySearch,
    pub lang: LanguageDetector,
    pub semantic_top_k: usize,
    pub bm25_top_k: usize,
    pub rrf_k: usize,
    pub reranker_top_n: usize,
}

impl RetrievalService {
    /// Execute the full retrieval + generation pipeline.
    ///
    /// This method is **synchronous** and is designed to be called from within
    /// `tokio::task::spawn_blocking`. The async LLM call is bridged via
    /// `tokio::runtime::Handle::current().block_on(...)`.
    pub fn query(
        &self,
        conn: &Connection,
        query: &str,
        memory_category: Option<&str>,
    ) -> anyhow::Result<QueryResult> {
        // Step 1: Embed the query
        let query_vector = self.embed.embed_query(query)?;

        // Step 2: Vector (semantic) search
        let raw_vector_hits = self.vectors.search_chunks(conn, &query_vector, self.semantic_top_k)?;

        // Step 3: Load chunk metadata for each vector hit
        let semantic_hits: Vec<SearchHit> = raw_vector_hits
            .iter()
            .filter_map(|h| load_chunk_for_vector_hit(conn, h))
            .collect();

        // Step 4: BM25 / FTS search
        let lang = self.lang.detect(query);
        let raw_fts_hits = self.tantivy.search(query, &lang, self.bm25_top_k)?;

        // Step 5: Load chunk metadata for each FTS hit
        let bm25_hits: Vec<SearchHit> = raw_fts_hits
            .iter()
            .filter_map(|h| load_chunk_for_fts_hit(conn, h))
            .collect();

        // Step 6: RRF merge
        let merged = rrf_merge(&semantic_hits, &bm25_hits, self.rrf_k, 20);

        // Step 7: Expand to L2 (leaf → parent chunks)
        let l2_chunks = expand_to_l2(conn, &merged)?;

        // Step 8: Rerank
        let texts: Vec<&str> = l2_chunks.iter().map(|c| c.content.as_str()).collect();
        let reranked_chunks = if !texts.is_empty() {
            let rerank_results = self.reranker.rerank(query, &texts, self.reranker_top_n)?;
            rerank_results
                .iter()
                .filter_map(|r| l2_chunks.get(r.original_index))
                .cloned()
                .collect::<Vec<_>>()
        } else {
            l2_chunks
        };

        // Step 9: Memory search
        let mem_hits =
            self.memory_search.search(conn, &self.vectors, &query_vector, memory_category)?;

        // Step 10: Build context
        let today = Local::now().format("%Y-%m-%d").to_string();
        let ctx = self.context_builder.build(query, &reranked_chunks, &mem_hits, &today);

        // Step 11: Call LLM (async, bridged via Handle::current().block_on)
        let llm = Arc::clone(&self.llm);
        let prompt = ctx.prompt.clone();
        let llm_response = tokio::runtime::Handle::current()
            .block_on(async move { llm.complete(&prompt).await })?;

        Ok(QueryResult {
            answer: llm_response.answer,
            sources: ctx.sources,
            input_tokens: llm_response.input_tokens,
            output_tokens: llm_response.output_tokens,
        })
    }
}

/// Load a [`SearchHit`] from the DB for a vector search result.
fn load_chunk_for_vector_hit(conn: &Connection, hit: &VectorHit) -> Option<SearchHit> {
    let repo = ChunkRepository::new(conn);
    let chunks = repo.get_by_ids(std::slice::from_ref(&hit.id)).ok()?;
    let chunk = chunks.into_iter().next()?;

    Some(SearchHit {
        chunk_id: chunk.id,
        content: chunk.content,
        parent_chunk_id: chunk.parent_chunk_id,
        doc_id: chunk.doc_id,
        score: 1.0 - hit.distance, // convert L2 distance to rough similarity
        section_heading: chunk.section_heading,
        page_number: chunk.page_number.map(|p| p as u32),
    })
}

/// Load a [`SearchHit`] from the DB for an FTS search result.
fn load_chunk_for_fts_hit(conn: &Connection, hit: &FtsHit) -> Option<SearchHit> {
    let repo = ChunkRepository::new(conn);
    let chunks = repo.get_by_ids(std::slice::from_ref(&hit.chunk_id)).ok()?;
    let chunk = chunks.into_iter().next()?;

    Some(SearchHit {
        chunk_id: chunk.id,
        content: chunk.content,
        parent_chunk_id: chunk.parent_chunk_id,
        doc_id: chunk.doc_id,
        score: hit.score,
        section_heading: chunk.section_heading,
        page_number: chunk.page_number.map(|p| p as u32),
    })
}
