# Task 3: Vector Search (sqlite-vec)

**Goal:** VectorStore поверх sqlite-vec: индексация векторов (chunks + memories) и HNSW-поиск по косинусному сходству.

**Files:**
- Create: `rust/src/search/vectors.rs`
- Modify: `rust/src/search/mod.rs`

---

### Task 3.1: VectorStore

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/search/vectors.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use tempfile::tempdir;

    fn make_pool() -> crate::db::pool::DbPool {
        build_pool(tempdir().unwrap().path().join("t.db").to_str().unwrap()).unwrap()
    }

    #[test]
    fn insert_and_search_chunks() {
        let pool = make_pool();
        let conn = pool.get().unwrap();
        let store = VectorStore::new(384);

        // Вектор: единичный по первому измерению
        let v1: Vec<f32> = (0..384).map(|i| if i == 0 { 1.0 } else { 0.0 }).collect();
        // Вектор: единичный по второму измерению
        let v2: Vec<f32> = (0..384).map(|i| if i == 1 { 1.0 } else { 0.0 }).collect();

        store.insert_chunk(&conn, "chunk-1", &v1).unwrap();
        store.insert_chunk(&conn, "chunk-2", &v2).unwrap();

        // Запрос похожий на v1 → chunk-1 должен быть первым
        let results = store.search_chunks(&conn, &v1, 2).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "chunk-1");
        assert!(results[0].distance < results[1].distance);
    }

    #[test]
    fn insert_and_search_memories() {
        let pool = make_pool();
        let conn = pool.get().unwrap();
        let store = VectorStore::new(384);

        let v: Vec<f32> = (0..384).map(|i| if i == 5 { 1.0 } else { 0.0 }).collect();
        store.insert_memory(&conn, "mem-1", &v).unwrap();

        let results = store.search_memories(&conn, &v, 5, 0.1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "mem-1");
    }

    #[test]
    fn delete_chunk_removes_from_search() {
        let pool = make_pool();
        let conn = pool.get().unwrap();
        let store = VectorStore::new(384);

        let v: Vec<f32> = (0..384).map(|i| if i == 2 { 1.0 } else { 0.0 }).collect();
        store.insert_chunk(&conn, "chunk-del", &v).unwrap();
        store.delete_chunk(&conn, "chunk-del").unwrap();

        let results = store.search_chunks(&conn, &v, 10).unwrap();
        assert!(results.iter().all(|r| r.id != "chunk-del"));
    }
}
```

- [ ] **Шаг 2: Запустить — убедиться что FAIL**

```bash
cd rust && cargo test vectors 2>&1 | tail -5
```

- [ ] **Шаг 3: Реализовать VectorStore**

```rust
use rusqlite::{Connection, params};

/// Результат поиска векторов.
#[derive(Debug, Clone)]
pub struct VectorHit {
    pub id: String,
    pub distance: f32,
}

/// Обёртка над sqlite-vec для chunks и memories.
pub struct VectorStore {
    dimensions: usize,
}

impl VectorStore {
    pub fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }

    // ── Chunks ──────────────────────────────────────────────────────────────

    pub fn insert_chunk(
        &self,
        conn: &Connection,
        chunk_id: &str,
        embedding: &[f32],
    ) -> rusqlite::Result<()> {
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

    /// Удалить все векторы чанков для заданного doc_id.
    /// Нужно сначала получить chunk_id-ы из таблицы chunks.
    pub fn delete_chunks_for_doc(
        &self,
        conn: &Connection,
        doc_id: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM chunk_vectors
             WHERE chunk_id IN (SELECT id FROM chunks WHERE doc_id = ?1)",
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
        let mut stmt = conn.prepare_cached(
            "SELECT chunk_id, distance
             FROM chunk_vectors
             WHERE embedding MATCH ?1
             ORDER BY distance
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![blob, top_k as i64], |r| {
            Ok(VectorHit {
                id: r.get(0)?,
                distance: r.get::<_, f64>(1)? as f32,
            })
        })?;
        rows.collect()
    }

    // ── Memories ─────────────────────────────────────────────────────────────

    pub fn insert_memory(
        &self,
        conn: &Connection,
        memory_id: &str,
        embedding: &[f32],
    ) -> rusqlite::Result<()> {
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

    /// Поиск похожих воспоминаний с порогом по расстоянию.
    /// distance_threshold: 0.0 = идентичные, 2.0 = полностью разные (L2-норма).
    /// Для cosine distance: 0 = identical, 1 = orthogonal, 2 = opposite.
    /// Типичный threshold для "похожих" = 0.7 (т.е. cosine sim > 0.3).
    pub fn search_memories(
        &self,
        conn: &Connection,
        query_vector: &[f32],
        top_k: usize,
        distance_threshold: f32,
    ) -> rusqlite::Result<Vec<VectorHit>> {
        let blob = f32_slice_to_bytes(query_vector);
        // sqlite-vec distance: cosine distance = 1 - cosine_similarity
        // threshold 0.7 → similarity > 0.3
        let mut stmt = conn.prepare_cached(
            "SELECT memory_id, distance
             FROM memory_vectors
             WHERE embedding MATCH ?1
               AND distance <= ?3
             ORDER BY distance
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            params![blob, top_k as i64, distance_threshold as f64],
            |r| {
                Ok(VectorHit {
                    id: r.get(0)?,
                    distance: r.get::<_, f64>(1)? as f32,
                })
            },
        )?;
        rows.collect()
    }

    /// Количество векторов для conflict detection в MemoryService.
    /// Возвращает hits с threshold для "потенциальных конфликтов".
    pub fn find_similar_memories(
        &self,
        conn: &Connection,
        query_vector: &[f32],
        limit: usize,
        similarity_threshold: f32,
    ) -> rusqlite::Result<Vec<VectorHit>> {
        // cosine sim threshold → distance threshold: dist = 1 - sim
        let dist_threshold = 1.0 - similarity_threshold;
        self.search_memories(conn, query_vector, limit, dist_threshold)
    }
}

/// Конвертирует f32 срез в байтовый буфер (little-endian) для sqlite-vec.
fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

#[cfg(test)]
mod tests {
    // (код тестов из Шага 1 выше)
}
```

- [ ] **Шаг 4: Запустить тесты**

```bash
cd rust && cargo test vectors 2>&1
```

Ожидаем: 3 теста зелёных.

> **Важно:** sqlite-vec `MATCH` синтаксис и формат векторов нужно проверить против установленной версии `sqlite-vec 0.1`. Если API отличается — сверить с документацией на https://alexgarcia.xyz/sqlite-vec.

- [ ] **Шаг 5: Добавить в search/mod.rs**

```rust
pub mod vectors;
pub use vectors::VectorStore;
```

- [ ] **Шаг 6: Коммит**

```bash
git add rust/src/search/
git commit -m "feat(rust): VectorStore поверх sqlite-vec — HNSW поиск chunks и memories"
```
