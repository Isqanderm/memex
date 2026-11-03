use rusqlite::Connection;
use tracing::warn;

use crate::db::repositories::memories::MemoryRepository;
use crate::search::reranker::Reranker;
use crate::search::rrf::{rrf_merge, SearchHit};
use crate::search::tantivy_fts::TantivyStore;
use crate::search::vectors::VectorStore;

/// RRF smoothing constant for memory retrieval.
const RRF_K: usize = 60;

/// Minimum candidates per side when a scope or category filter is applied after
/// retrieval, so a filtered-out majority does not crowd the matching facts out.
const FILTERED_MIN_CANDIDATES: usize = 200;

/// A single memory retrieval hit.
#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub memory_id: String,
    pub content: String,
    /// Cross-encoder score when reranked, otherwise the RRF score.
    pub score: f32,
    pub source: String,
    pub category: Option<String>,
    pub project: Option<String>,
    pub scope: String,
    pub created_at: String,
    pub access_count: i64,
}

/// Hybrid retrieval of memory facts: sqlite-vec ∪ tantivy BM25, fused with RRF,
/// then reranked with the cross-encoder.
#[derive(Clone)]
pub struct MemorySearch {
    /// Minimum cosine similarity for a vector hit.
    pub retrieval_similarity: f32,
    /// Maximum number of memory hits to return.
    pub top_k: usize,
}

impl MemorySearch {
    pub fn new(retrieval_similarity: f32, top_k: usize) -> Self {
        Self {
            retrieval_similarity,
            top_k,
        }
    }

    /// Search for relevant memories.
    ///
    /// 1. Vector hits above `retrieval_similarity` (cosine).
    /// 2. BM25 hits from the memory FTS index.
    ///    Each side fetches `top_k * 2` candidates, or at least
    ///    `FILTERED_MIN_CANDIDATES` when `scope` or `category` is given (the filter runs
    ///    after retrieval, on the global candidate list).
    /// 3. RRF merge (`chunk_id` = memory id).
    /// 4. Load each memory, keep active ones matching `scope` and `category`.
    /// 5. Rerank up to `top_k * 3` candidates with the cross-encoder (when given and
    ///    there is more than one candidate); on rerank error keep the RRF order.
    #[allow(clippy::too_many_arguments)]
    pub fn search(
        &self,
        conn: &Connection,
        vectors: &VectorStore,
        fts: &TantivyStore,
        reranker: Option<&Reranker>,
        query: &str,
        query_vector: &[f32],
        lang: &str,
        scope: Option<&str>,
        category: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryHit>> {
        if self.top_k == 0 {
            return Ok(vec![]);
        }
        let candidates = if scope.is_some() || category.is_some() {
            self.top_k.saturating_mul(2).max(FILTERED_MIN_CANDIDATES)
        } else {
            self.top_k.saturating_mul(2)
        };
        let rerank_pool = self.top_k.saturating_mul(3);

        let vec_hits: Vec<SearchHit> = vectors
            .find_similar_memories(conn, query_vector, candidates, self.retrieval_similarity)?
            .into_iter()
            .map(|h| memory_hit_stub(h.id, 1.0 - h.distance))
            .collect();

        let bm25_hits: Vec<SearchHit> = fts
            .search(query, lang, candidates)?
            .into_iter()
            .map(|h| memory_hit_stub(h.chunk_id, h.score))
            .collect();

        // No limit here: filtering below may drop hits, truncate afterwards.
        let merged = rrf_merge(&vec_hits, &bm25_hits, RRF_K, 0);

        let repo = MemoryRepository::new(conn);
        let mut results = Vec::new();

        for hit in merged {
            let Some(memory) = repo.get_by_id(&hit.chunk_id)? else {
                continue;
            };
            if !memory.is_active {
                continue;
            }
            if scope.is_some_and(|s| memory.scope != s) {
                continue;
            }
            if category.is_some_and(|c| memory.category.as_deref() != Some(c)) {
                continue;
            }

            results.push(MemoryHit {
                memory_id: memory.id,
                content: memory.content,
                score: hit.score,
                source: memory.source,
                category: memory.category,
                project: memory.project,
                scope: memory.scope,
                created_at: memory.created_at,
                access_count: memory.access_count,
            });
            if results.len() >= rerank_pool {
                break;
            }
        }

        if let (Some(reranker), true) = (reranker, results.len() > 1) {
            let texts: Vec<&str> = results.iter().map(|h| h.content.as_str()).collect();
            match reranker.rerank(query, &texts, self.top_k) {
                Ok(ranked) => {
                    return Ok(ranked
                        .into_iter()
                        .filter_map(|r| {
                            results.get(r.original_index).cloned().map(|mut h| {
                                h.score = r.score;
                                h
                            })
                        })
                        .collect());
                }
                Err(e) => warn!("Memory rerank failed, keeping RRF order: {e}"),
            }
        }

        results.truncate(self.top_k);
        Ok(results)
    }
}

/// A `SearchHit` carrying only a memory id and score, for `rrf_merge`.
fn memory_hit_stub(memory_id: String, score: f32) -> SearchHit {
    SearchHit {
        chunk_id: memory_id,
        content: String::new(),
        parent_chunk_id: None,
        doc_id: "memory".to_string(),
        score,
        section_heading: None,
        page_number: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use crate::db::repositories::memories::NewMemory;
    use tempfile::tempdir;

    struct Fixture {
        _dir: tempfile::TempDir,
        pool: crate::db::pool::DbPool,
        fts: TantivyStore,
    }

    fn setup() -> Fixture {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = build_pool(path.to_str().unwrap()).unwrap();
        let fts = TantivyStore::open(dir.path().join("fts").to_str().unwrap()).unwrap();
        Fixture { _dir: dir, pool, fts }
    }

    fn add_fact(conn: &Connection, fts: &TantivyStore, content: &str, scope: &str) -> String {
        let id = MemoryRepository::new(conn)
            .create(&NewMemory {
                content,
                source: "explicit",
                source_id: None,
                scope,
                forget_after: None,
                relation: None,
                parent_id: None,
                category: None,
                project: None,
            })
            .unwrap();
        fts.add_chunk(&id, "memory", "en", content).unwrap();
        fts.commit().unwrap();
        id
    }

    #[test]
    fn memory_search_returns_empty_when_no_vectors() {
        let f = setup();
        let conn = f.pool.get().unwrap();
        let store = VectorStore::new(384);
        let searcher = MemorySearch::new(0.60, 10);

        let query = vec![0.0_f32; 384];
        let hits = searcher
            .search(&conn, &store, &f.fts, None, "anything", &query, "en", None, None)
            .unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn exact_token_found_via_bm25_even_with_far_vector() {
        let f = setup();
        let conn = f.pool.get().unwrap();
        let id = add_fact(&conn, &f.fts, "User's MEMEX_URL is http://memex:8000", "default");

        let searcher = MemorySearch::new(0.60, 10);
        let query = vec![0.0_f32; 384];
        let hits = searcher
            .search(&conn, &VectorStore::new(384), &f.fts, None, "MEMEX_URL", &query, "en", None, None)
            .unwrap();
        assert!(!hits.is_empty(), "BM25 should find the exact token");
        assert_eq!(hits[0].memory_id, id);
        assert_eq!(hits[0].scope, "default");
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn deactivated_memory_is_not_returned() {
        let f = setup();
        let conn = f.pool.get().unwrap();
        let id = add_fact(&conn, &f.fts, "User's MEMEX_URL is http://memex:8000", "default");

        MemoryRepository::new(&conn).deactivate(&id).unwrap();
        f.fts.delete_by_chunk_id(&id).unwrap();
        f.fts.commit().unwrap();

        let searcher = MemorySearch::new(0.60, 10);
        let query = vec![0.0_f32; 384];
        let hits = searcher
            .search(&conn, &VectorStore::new(384), &f.fts, None, "MEMEX_URL", &query, "en", None, None)
            .unwrap();
        assert!(hits.is_empty());
        assert!(f.fts.search("MEMEX_URL", "en", 10).unwrap().is_empty());
    }

    #[test]
    fn scoped_search_oversamples_past_other_scopes() {
        let f = setup();
        let conn = f.pool.get().unwrap();
        for i in 0..50 {
            add_fact(&conn, &f.fts, &format!("widget fact number {i} widget"), "default");
        }
        let work = add_fact(&conn, &f.fts, "The work widget is the blue one", "work");

        // top_k = 10: without oversampling only 20 BM25 candidates would be fetched.
        let hits = MemorySearch::new(0.60, 10)
            .search(&conn, &VectorStore::new(384), &f.fts, None, "widget", &[0.0; 384], "en", Some("work"), None)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].memory_id, work);
    }

    #[test]
    fn scope_filter_applies() {
        let f = setup();
        let conn = f.pool.get().unwrap();
        let a = add_fact(&conn, &f.fts, "User prefers dark roast coffee", "a");
        add_fact(&conn, &f.fts, "User prefers dark roast coffee", "b");

        let searcher = MemorySearch::new(0.60, 10);
        let query = vec![0.0_f32; 384];
        let hits = searcher
            .search(&conn, &VectorStore::new(384), &f.fts, None, "coffee", &query, "en", Some("a"), None)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].memory_id, a);
        assert_eq!(hits[0].scope, "a");
    }

    #[test]
    fn backfill_indexes_active_facts_once() {
        let f = setup();
        let conn = f.pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);
        let new = |content| NewMemory {
            content,
            source: "explicit",
            source_id: None,
            scope: "default",
            forget_after: None,
            relation: None,
            parent_id: None,
            category: None,
            project: None,
        };
        let id = repo.create(&new("User's MEMEX_URL is http://memex:8000")).unwrap();
        repo.create(&new("User prefers dark roast coffee")).unwrap();

        let n = crate::memory::service::backfill_memory_fts(&conn, &f.fts).unwrap();
        assert_eq!(n, 2);

        let hits = MemorySearch::new(0.60, 10)
            .search(&conn, &VectorStore::new(384), &f.fts, None, "MEMEX_URL", &[0.0; 384], "en", None, None)
            .unwrap();
        assert_eq!(hits.first().map(|h| h.memory_id.as_str()), Some(id.as_str()));

        assert_eq!(crate::memory::service::backfill_memory_fts(&conn, &f.fts).unwrap(), 0);
    }

    #[test]
    fn zero_top_k_returns_empty() {
        let f = setup();
        let conn = f.pool.get().unwrap();
        add_fact(&conn, &f.fts, "User's MEMEX_URL is http://memex:8000", "default");
        let hits = MemorySearch::new(0.60, 0)
            .search(&conn, &VectorStore::new(384), &f.fts, None, "MEMEX_URL", &[0.0; 384], "en", None, None)
            .unwrap();
        assert!(hits.is_empty());
    }

    /// Needs the jina reranker ONNX model (MODEL_CACHE_DIR, default ~/.cache/fastembed).
    #[test]
    #[ignore]
    fn reranker_puts_the_answering_fact_first() {
        let f = setup();
        let conn = f.pool.get().unwrap();
        // Indexed with the "User" prefix so both facts are BM25 candidates for "user".
        let novi_sad = add_fact(&conn, &f.fts, "User lives in Novi Sad", "default");
        add_fact(&conn, &f.fts, "User uses Python", "default");

        let dir = std::env::var("MODEL_CACHE_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/fastembed")
        });
        let reranker = Reranker::new("jina-reranker-v2-base-multilingual", &dir).unwrap();

        let search = |q: &str, r: Option<&Reranker>| {
            MemorySearch::new(0.60, 10)
                .search(&conn, &VectorStore::new(384), &f.fts, r, q, &[0.0; 384], "en", None, None)
                .unwrap()
        };

        let hits = search("where does the user live", Some(&reranker));
        assert_eq!(hits.len(), 2, "both facts are candidates");
        assert_eq!(hits[0].memory_id, novi_sad);

        // Only "user" overlaps lexically: BM25/RRF ranks the shorter Python fact first,
        // the cross-encoder must flip it.
        let q = "which city is the user from";
        assert_ne!(search(q, None)[0].memory_id, novi_sad, "precondition: RRF order is wrong");
        assert_eq!(search(q, Some(&reranker))[0].memory_id, novi_sad);
    }
}
