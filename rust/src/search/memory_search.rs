use rusqlite::Connection;

use crate::db::repositories::memories::MemoryRepository;
use crate::search::vectors::VectorStore;

/// A single memory retrieval hit.
#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub memory_id: String,
    pub content: String,
    pub score: f32,
    pub source: String,
    pub category: Option<String>,
    pub project: Option<String>,
    pub created_at: String,
}

/// Retrieves relevant memory facts for a given query vector.
pub struct MemorySearch {
    /// Distance threshold (L2 distance) — memories with distance > threshold are excluded.
    /// Default: 0.70 (corresponds to cosine similarity ≥ ~0.30 for unit-normalized vectors).
    pub retrieval_threshold: f32,
    /// Maximum number of memory hits to return.
    pub top_k: usize,
}

impl Default for MemorySearch {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySearch {
    /// Create with default parameters (threshold=0.70, top_k=10).
    pub fn new() -> Self {
        Self {
            retrieval_threshold: 0.70,
            top_k: 10,
        }
    }

    /// Search for relevant memories.
    ///
    /// Steps:
    /// 1. Vector search with distance threshold.
    /// 2. Load full memory from repository, skip inactive.
    /// 3. Optionally filter by category.
    /// 4. Convert distance to score: score = 1.0 - distance.
    pub fn search(
        &self,
        conn: &Connection,
        vectors: &VectorStore,
        query_vector: &[f32],
        category: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryHit>> {
        let raw_hits =
            vectors.search_memories(conn, query_vector, self.top_k, self.retrieval_threshold)?;

        let repo = MemoryRepository::new(conn);
        let mut results = Vec::new();

        for hit in &raw_hits {
            let memory = match repo.get_by_id(&hit.id)? {
                Some(m) => m,
                None => continue,
            };

            // Skip inactive memories
            if !memory.is_active {
                continue;
            }

            // Filter by category if provided
            if let Some(cat) = category {
                match &memory.category {
                    Some(mc) if mc == cat => {}
                    _ => continue,
                }
            }

            let score = 1.0 - hit.distance;

            results.push(MemoryHit {
                memory_id: memory.id,
                content: memory.content,
                score,
                source: memory.source,
                category: memory.category,
                project: memory.project,
                created_at: memory.created_at,
            });
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, crate::db::pool::DbPool) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = build_pool(path.to_str().unwrap()).unwrap();
        (dir, pool)
    }

    #[test]
    fn memory_search_returns_empty_when_no_vectors() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();
        let store = VectorStore::new(384);
        let searcher = MemorySearch::new();

        let query = vec![0.0_f32; 384];
        let hits = searcher.search(&conn, &store, &query, None).unwrap();
        assert!(hits.is_empty());
    }
}
