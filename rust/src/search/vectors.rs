use rusqlite::{params, Connection};

/// Result from a vector search
#[derive(Debug, Clone)]
pub struct VectorHit {
    pub id: String,
    pub distance: f32,
}

/// Wraps sqlite-vec virtual tables for vector similarity search on chunks and memories.
///
/// The virtual tables `chunk_vectors` and `memory_vectors` are created in `pool.rs`
/// using `USING vec0(... embedding float[N])`.  The default distance metric for
/// `float[]` columns is **L2** (Euclidean squared distance).
pub struct VectorStore {
    dimensions: usize,
}

/// Serialize a `&[f32]` slice to a raw little-endian byte blob as required by
/// sqlite-vec when binding a BLOB parameter.
fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

impl VectorStore {
    pub fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }

    // ── Chunk operations ──────────────────────────────────────────────────────

    pub fn insert_chunk(
        &self,
        conn: &Connection,
        chunk_id: &str,
        embedding: &[f32],
    ) -> rusqlite::Result<()> {
        if embedding.len() != self.dimensions {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "embedding length mismatch: expected {}, got {}",
                self.dimensions,
                embedding.len()
            )));
        }
        let blob = f32_slice_to_bytes(embedding);
        conn.execute(
            "INSERT OR REPLACE INTO chunk_vectors (chunk_id, embedding) VALUES (?1, ?2)",
            params![chunk_id, blob],
        )?;
        Ok(())
    }

    pub fn delete_chunk(&self, conn: &Connection, chunk_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM chunk_vectors WHERE chunk_id = ?1",
            params![chunk_id],
        )?;
        Ok(())
    }

    /// Delete all chunk vectors for a document by joining with the chunks table.
    pub fn delete_chunks_for_doc(
        &self,
        conn: &Connection,
        doc_id: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM chunk_vectors WHERE chunk_id IN (
                 SELECT id FROM chunks WHERE doc_id = ?1 AND chunk_role = 'leaf'
             )",
            params![doc_id],
        )?;
        Ok(())
    }

    pub fn search_chunks(
        &self,
        conn: &Connection,
        query_vector: &[f32],
        top_k: usize,
    ) -> rusqlite::Result<Vec<VectorHit>> {
        let blob = f32_slice_to_bytes(query_vector);
        let mut stmt = conn.prepare(
            "SELECT chunk_id, distance
             FROM chunk_vectors
             WHERE embedding MATCH ?1
             ORDER BY distance
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![blob, top_k as i64], |row| {
            Ok(VectorHit {
                id: row.get(0)?,
                distance: row.get::<_, f64>(1)? as f32,
            })
        })?;
        rows.collect()
    }

    // ── Memory operations ─────────────────────────────────────────────────────

    pub fn insert_memory(
        &self,
        conn: &Connection,
        memory_id: &str,
        embedding: &[f32],
    ) -> rusqlite::Result<()> {
        if embedding.len() != self.dimensions {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "embedding length mismatch: expected {}, got {}",
                self.dimensions,
                embedding.len()
            )));
        }
        let blob = f32_slice_to_bytes(embedding);
        conn.execute(
            "INSERT OR REPLACE INTO memory_vectors (memory_id, embedding) VALUES (?1, ?2)",
            params![memory_id, blob],
        )?;
        Ok(())
    }

    pub fn delete_memory(&self, conn: &Connection, memory_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM memory_vectors WHERE memory_id = ?1",
            params![memory_id],
        )?;
        Ok(())
    }

    /// Search memories by vector similarity and filter by distance threshold.
    ///
    /// `distance_threshold` is expressed as an L2 distance upper bound —
    /// results with `distance > distance_threshold` are excluded.
    /// sqlite-vec supports `WHERE distance <= ?` constraints natively.
    pub fn search_memories(
        &self,
        conn: &Connection,
        query_vector: &[f32],
        top_k: usize,
        distance_threshold: f32,
    ) -> rusqlite::Result<Vec<VectorHit>> {
        let blob = f32_slice_to_bytes(query_vector);
        let mut stmt = conn.prepare(
            "SELECT memory_id, distance
             FROM memory_vectors
             WHERE embedding MATCH ?1
               AND distance <= ?3
             ORDER BY distance
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            params![blob, top_k as i64, distance_threshold as f64],
            |row| {
                Ok(VectorHit {
                    id: row.get(0)?,
                    distance: row.get::<_, f64>(1)? as f32,
                })
            },
        )?;
        rows.collect()
    }

    /// Find similar memories using a cosine-similarity threshold.
    ///
    /// **Assumes unit-normalized embeddings** (as produced by multilingual-e5).
    /// For unit vectors, L2 distance and cosine distance are equivalent up to a
    /// monotone transformation, so the conversion used here is exact.
    ///
    /// **Note:** The virtual tables use the default L2 distance metric.
    /// `similarity_threshold` (0–1 cosine similarity) is converted to an L2
    /// distance bound via the formula: `dist_threshold = sqrt(2 * (1 - cosine))`.
    /// This is the exact conversion for unit-normalized vectors.
    pub fn find_similar_memories(
        &self,
        conn: &Connection,
        query_vector: &[f32],
        limit: usize,
        similarity_threshold: f32,
    ) -> rusqlite::Result<Vec<VectorHit>> {
        // For unit-normalized vectors: L2 = sqrt(2*(1-cosine))
        let dist_threshold = (2.0_f32 * (1.0 - similarity_threshold)).sqrt();
        self.search_memories(conn, query_vector, limit, dist_threshold)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use tempfile::tempdir;

    /// The virtual tables are always created with `float[384]`.
    const DIMS: usize = 384;

    fn make_store() -> VectorStore {
        VectorStore::new(DIMS)
    }

    fn open_conn() -> (tempfile::TempDir, crate::db::pool::DbConn) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = build_pool(db_path.to_str().unwrap()).expect("pool build failed");
        let conn = pool.get().expect("get conn");
        // Keep `dir` alive by returning it alongside the connection.
        (dir, conn)
    }

    /// Build a 384-dim zero vector with `val` at position `dim`.
    fn unit_at(dim: usize, val: f32) -> Vec<f32> {
        let mut v = vec![0.0_f32; DIMS];
        v[dim] = val;
        v
    }

    #[test]
    fn insert_and_search_chunks() {
        let store = make_store();
        let (_dir, conn) = open_conn();

        // v1 = 1.0 at dim 0, v2 = 1.0 at dim 1
        let v1 = unit_at(0, 1.0);
        let v2 = unit_at(1, 1.0);

        store.insert_chunk(&conn, "chunk-1", &v1).unwrap();
        store.insert_chunk(&conn, "chunk-2", &v2).unwrap();

        // Query with v1 — chunk-1 should be closest (distance 0)
        let results = store.search_chunks(&conn, &v1, 2).unwrap();
        assert_eq!(results.len(), 2, "should return 2 results");
        assert_eq!(results[0].id, "chunk-1", "chunk-1 should be first");
        assert!(
            results[0].distance < results[1].distance,
            "chunk-1 distance ({}) should be less than chunk-2 distance ({})",
            results[0].distance,
            results[1].distance
        );
    }

    #[test]
    fn insert_and_search_memories() {
        let store = make_store();
        let (_dir, conn) = open_conn();

        let vec = unit_at(5, 1.0);
        store.insert_memory(&conn, "mem-1", &vec).unwrap();

        let results = store.search_memories(&conn, &vec, 5, f32::MAX).unwrap();
        assert!(!results.is_empty(), "should find at least one memory");
        assert_eq!(results[0].id, "mem-1");
        assert!(results[0].distance < 1e-5, "exact match should have near-zero distance");
    }

    #[test]
    fn delete_chunk_removes_from_search() {
        let store = make_store();
        let (_dir, conn) = open_conn();

        let v = unit_at(0, 1.0);
        store.insert_chunk(&conn, "chunk-del", &v).unwrap();

        // Verify it's there first
        let before = store.search_chunks(&conn, &v, 10).unwrap();
        assert!(before.iter().any(|h| h.id == "chunk-del"), "chunk should exist before delete");

        store.delete_chunk(&conn, "chunk-del").unwrap();

        let after = store.search_chunks(&conn, &v, 10).unwrap();
        assert!(
            !after.iter().any(|h| h.id == "chunk-del"),
            "chunk-del should not appear after deletion"
        );
    }

    #[test]
    fn delete_chunks_for_doc_removes_all_vectors() {
        let store = make_store();
        let (_dir, conn) = open_conn();

        // Insert a document and leaf chunk in the chunks table
        conn.execute(
            "INSERT INTO documents (id, source, mime_type, checksum, metadata) VALUES ('doc1', 's', 't', 'ck1', '{}')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO chunks (id, doc_id, chunk_role, chunk_index, language, content) VALUES ('chunk1', 'doc1', 'leaf', 0, 'en', 'content')",
            [],
        ).unwrap();

        let v: Vec<f32> = (0..384).map(|i| if i == 0 { 1.0f32 } else { 0.0 }).collect();
        store.insert_chunk(&conn, "chunk1", &v).unwrap();

        // Verify it's there
        let results = store.search_chunks(&conn, &v, 5).unwrap();
        assert_eq!(results.len(), 1);

        // Delete by doc_id
        store.delete_chunks_for_doc(&conn, "doc1").unwrap();

        // Verify it's gone
        let results = store.search_chunks(&conn, &v, 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn cosine_to_l2_threshold_correct() {
        // cosine=1.0 (identical) → L2=0.0
        let t = (2.0_f32 * (1.0 - 1.0_f32)).sqrt();
        assert!((t - 0.0).abs() < 1e-6);

        // cosine=0.0 (orthogonal) → L2=sqrt(2)≈1.414
        let t = (2.0_f32 * (1.0 - 0.0_f32)).sqrt();
        assert!((t - 1.4142135).abs() < 1e-4);

        // cosine=0.60 → L2≈0.894, NOT 0.40
        let t = (2.0_f32 * (1.0 - 0.6_f32)).sqrt();
        assert!(t > 0.88 && t < 0.91, "expected ~0.894, got {t}");
    }
}
