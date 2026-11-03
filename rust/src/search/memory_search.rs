use rusqlite::Connection;

use crate::db::repositories::memories::MemoryRepository;
use crate::search::rrf::{rrf_merge, SearchHit};
use crate::search::tantivy_fts::TantivyStore;
use crate::search::vectors::VectorStore;

/// RRF smoothing constant for memory retrieval.
const RRF_K: usize = 60;

/// A single memory retrieval hit.
#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub memory_id: String,
    pub content: String,
    /// Reciprocal Rank Fusion score over the vector and BM25 lists.
    pub score: f32,
    pub source: String,
    pub category: Option<String>,
    pub project: Option<String>,
    pub scope: String,
    pub created_at: String,
}

/// Hybrid retrieval of memory facts: sqlite-vec ∪ tantivy BM25, fused with RRF.
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
    /// 3. RRF merge (`chunk_id` = memory id).
    /// 4. Load each memory, keep active ones matching `scope` and `category`.
    #[allow(clippy::too_many_arguments)]
    pub fn search(
        &self,
        conn: &Connection,
        vectors: &VectorStore,
        fts: &TantivyStore,
        query: &str,
        query_vector: &[f32],
        lang: &str,
        scope: Option<&str>,
        category: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryHit>> {
        if self.top_k == 0 {
            return Ok(vec![]);
        }
        let candidates = self.top_k * 2;

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
            });
            if results.len() >= self.top_k {
                break;
            }
        }

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
            .search(&conn, &store, &f.fts, "anything", &query, "en", None, None)
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
            .search(&conn, &VectorStore::new(384), &f.fts, "MEMEX_URL", &query, "en", None, None)
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
            .search(&conn, &VectorStore::new(384), &f.fts, "MEMEX_URL", &query, "en", None, None)
            .unwrap();
        assert!(hits.is_empty());
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
            .search(&conn, &VectorStore::new(384), &f.fts, "coffee", &query, "en", Some("a"), None)
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
            .search(&conn, &VectorStore::new(384), &f.fts, "MEMEX_URL", &[0.0; 384], "en", None, None)
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
            .search(&conn, &VectorStore::new(384), &f.fts, "MEMEX_URL", &[0.0; 384], "en", None, None)
            .unwrap();
        assert!(hits.is_empty());
    }
}
