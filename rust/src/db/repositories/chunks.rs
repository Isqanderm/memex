use std::collections::HashMap;

use rusqlite::{types::ToSql, Connection, params};
use uuid::Uuid;

use crate::ingestion::chunker::ChunkData;

/// A chunk row retrieved from the database.
#[derive(Debug, Clone)]
pub struct StoredChunk {
    pub id: String,
    pub doc_id: String,
    pub parent_chunk_id: Option<String>,
    pub chunk_role: String,
    pub chunk_index: i64,
    pub section_heading: Option<String>,
    pub section_level: Option<i64>,
    pub page_number: Option<i64>,
    pub language: String,
    pub content: String,
}

/// An expanded L2 chunk with document metadata — used by the retrieval pipeline.
#[derive(Debug, Clone)]
pub struct L2Chunk {
    pub chunk_id: String,
    pub content: String,
    pub doc_id: String,
    pub section_heading: Option<String>,
    pub page_number: Option<u32>,
    pub doc_title: Option<String>,
    pub doc_source: Option<String>,
}

pub struct ChunkRepository<'a> {
    conn: &'a Connection,
}

impl<'a> ChunkRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Insert parent chunks for `doc_id`.
    /// Returns a map from each parent's `chunk_index` (temp) to its database UUID.
    pub fn bulk_insert_parents(
        &self,
        doc_id: &str,
        parents: &[ChunkData],
    ) -> rusqlite::Result<HashMap<usize, String>> {
        let mut map = HashMap::new();
        for chunk in parents {
            let id = Uuid::new_v4().to_string();
            self.conn.execute(
                "INSERT INTO chunks \
                 (id, doc_id, parent_chunk_id, chunk_role, chunk_index, \
                  section_heading, section_level, page_number, language, content) \
                 VALUES (?1,?2,NULL,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    id,
                    doc_id,
                    chunk.chunk_role,
                    chunk.chunk_index as i64,
                    chunk.section_heading,
                    chunk.section_level as i64,
                    chunk.page_number.map(|p| p as i64),
                    chunk.language,
                    chunk.content,
                ],
            )?;
            map.insert(chunk.chunk_index, id);
        }
        Ok(map)
    }

    /// Insert leaf chunks for `doc_id`, resolving parent UUIDs from `parent_ids`.
    /// Returns `Vec<(chunk_id, embedding)>` for vector indexing (only leaves that
    /// have an embedding set).
    pub fn bulk_insert_leaves(
        &self,
        doc_id: &str,
        leaves: &[ChunkData],
        parent_ids: &HashMap<usize, String>,
    ) -> rusqlite::Result<Vec<(String, Vec<f32>)>> {
        let mut leaf_vectors: Vec<(String, Vec<f32>)> = Vec::new();

        for chunk in leaves {
            let id = Uuid::new_v4().to_string();

            let parent_id: Option<&str> = chunk
                .parent_temp_index
                .and_then(|idx| parent_ids.get(&idx))
                .map(String::as_str);

            self.conn.execute(
                "INSERT INTO chunks \
                 (id, doc_id, parent_chunk_id, chunk_role, chunk_index, \
                  section_heading, section_level, page_number, language, content) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    id,
                    doc_id,
                    parent_id,
                    chunk.chunk_role,
                    chunk.chunk_index as i64,
                    chunk.section_heading,
                    chunk.section_level as i64,
                    chunk.page_number.map(|p| p as i64),
                    chunk.language,
                    chunk.content,
                ],
            )?;

            if let Some(emb) = &chunk.embedding {
                leaf_vectors.push((id, emb.clone()));
            }
        }

        Ok(leaf_vectors)
    }

    /// Get all leaf chunk IDs for a document (used when deleting a document).
    pub fn get_leaf_ids_for_doc(&self, doc_id: &str) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM chunks WHERE doc_id = ?1 AND chunk_role = 'leaf' ORDER BY chunk_index",
        )?;
        let rows = stmt.query_map(params![doc_id], |row| row.get(0))?;
        rows.collect()
    }

    /// Get chunks by a list of IDs.
    pub fn get_by_ids(&self, ids: &[String]) -> rusqlite::Result<Vec<StoredChunk>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let placeholders = (1..=ids.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT id, doc_id, parent_chunk_id, chunk_role, chunk_index, \
                    section_heading, section_level, page_number, language, content \
             FROM chunks WHERE id IN ({placeholders})"
        );

        let params: Vec<&dyn ToSql> = ids.iter().map(|s| s as &dyn ToSql).collect();

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok(StoredChunk {
                id: row.get(0)?,
                doc_id: row.get(1)?,
                parent_chunk_id: row.get(2)?,
                chunk_role: row.get(3)?,
                chunk_index: row.get(4)?,
                section_heading: row.get(5)?,
                section_level: row.get(6)?,
                page_number: row.get(7)?,
                language: row.get(8)?,
                content: row.get(9)?,
            })
        })?;
        rows.collect()
    }

    /// Get all leaf chunk contents for a document (for memory extraction).
    pub fn get_leaf_content_for_doc(&self, doc_id: &str) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT content FROM chunks \
             WHERE doc_id = ?1 AND chunk_role = 'leaf' \
             ORDER BY chunk_index",
        )?;
        let rows = stmt.query_map(params![doc_id], |row| row.get(0))?;
        rows.collect()
    }

    /// Expand leaf chunk IDs to their parent (L2) chunks with document metadata.
    ///
    /// Given a list of leaf chunk IDs from vector/FTS search, this method:
    /// 1. Looks up each leaf's `parent_chunk_id`.
    /// 2. Returns the parent chunk content plus document title/source.
    ///
    /// If a leaf has no parent (unlikely but possible), the leaf itself is
    /// returned in its place.
    pub fn expand_to_l2(&self, leaf_ids: &[String]) -> rusqlite::Result<Vec<L2Chunk>> {
        if leaf_ids.is_empty() {
            return Ok(vec![]);
        }

        // Build dynamic IN clause for leaf IDs
        let placeholders = (1..=leaf_ids.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");

        // Collect parent chunk IDs from the leaves
        let sql_parents = format!(
            "SELECT DISTINCT COALESCE(parent_chunk_id, id) \
             FROM chunks WHERE id IN ({placeholders})"
        );
        let params_leaf: Vec<&dyn ToSql> = leaf_ids.iter().map(|s| s as &dyn ToSql).collect();

        let mut stmt = self.conn.prepare(&sql_parents)?;
        let parent_ids: Vec<String> = stmt
            .query_map(params_leaf.as_slice(), |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;

        if parent_ids.is_empty() {
            return Ok(vec![]);
        }

        // Now fetch those parent chunks joined with documents
        let placeholders2 = (1..=parent_ids.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT c.id, c.content, c.doc_id, c.section_heading, c.page_number, \
                    d.title AS doc_title, d.source AS doc_source \
             FROM chunks c \
             JOIN documents d ON d.id = c.doc_id \
             WHERE c.id IN ({placeholders2})"
        );

        let params2: Vec<&dyn ToSql> = parent_ids.iter().map(|s| s as &dyn ToSql).collect();
        let mut stmt2 = self.conn.prepare(&sql)?;
        let rows = stmt2.query_map(params2.as_slice(), |row| {
            Ok(L2Chunk {
                chunk_id: row.get(0)?,
                content: row.get(1)?,
                doc_id: row.get(2)?,
                section_heading: row.get(3)?,
                page_number: row.get::<_, Option<i64>>(4)?.map(|p| p as u32),
                doc_title: row.get(5)?,
                doc_source: row.get(6)?,
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use crate::db::repositories::documents::DocumentRepository;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, crate::db::pool::DbPool) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = build_pool(path.to_str().unwrap()).unwrap();
        (dir, pool)
    }

    fn make_chunk(idx: usize, role: &str, parent: Option<usize>) -> ChunkData {
        ChunkData {
            content: format!("content of chunk {idx}"),
            chunk_role: role.to_string(),
            chunk_index: idx,
            language: "en".to_string(),
            section_heading: None,
            section_level: 0,
            page_number: None,
            embedding: if role == "leaf" {
                Some(vec![0.1_f32; 4])
            } else {
                None
            },
            parent_temp_index: parent,
        }
    }

    #[test]
    fn bulk_insert_parents_and_leaves() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();

        let doc_repo = DocumentRepository::new(&conn);
        let doc_id = doc_repo
            .create("file://test.txt", "text/plain", "cksum_1", None, "{}")
            .unwrap();

        let repo = ChunkRepository::new(&conn);

        let parents = vec![make_chunk(0, "parent", None), make_chunk(1, "parent", None)];
        let parent_ids = repo.bulk_insert_parents(&doc_id, &parents).unwrap();
        assert_eq!(parent_ids.len(), 2);

        let leaves = vec![
            make_chunk(2, "leaf", Some(0)),
            make_chunk(3, "leaf", Some(1)),
        ];
        let leaf_vectors = repo.bulk_insert_leaves(&doc_id, &leaves, &parent_ids).unwrap();
        assert_eq!(leaf_vectors.len(), 2, "both leaves have embeddings");
    }

    #[test]
    fn get_leaf_ids_for_doc_returns_only_leaves() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();

        let doc_repo = DocumentRepository::new(&conn);
        let doc_id = doc_repo
            .create("file://test2.txt", "text/plain", "cksum_2", None, "{}")
            .unwrap();

        let repo = ChunkRepository::new(&conn);

        let parents = vec![make_chunk(0, "parent", None)];
        let parent_ids = repo.bulk_insert_parents(&doc_id, &parents).unwrap();

        let leaves = vec![make_chunk(1, "leaf", Some(0))];
        repo.bulk_insert_leaves(&doc_id, &leaves, &parent_ids).unwrap();

        let leaf_ids = repo.get_leaf_ids_for_doc(&doc_id).unwrap();
        assert_eq!(leaf_ids.len(), 1);
    }

    #[test]
    fn get_by_ids_returns_correct_chunks() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();

        let doc_repo = DocumentRepository::new(&conn);
        let doc_id = doc_repo
            .create("file://test3.txt", "text/plain", "cksum_3", None, "{}")
            .unwrap();

        let repo = ChunkRepository::new(&conn);
        let parents = vec![make_chunk(0, "parent", None)];
        let parent_ids = repo.bulk_insert_parents(&doc_id, &parents).unwrap();

        let leaves = vec![make_chunk(1, "leaf", Some(0))];
        let leaf_vectors = repo.bulk_insert_leaves(&doc_id, &leaves, &parent_ids).unwrap();

        // Look up the leaf by its ID
        let leaf_id = leaf_vectors[0].0.clone();
        let fetched = repo.get_by_ids(&[leaf_id.clone()]).unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].id, leaf_id);
        assert_eq!(fetched[0].chunk_role, "leaf");
    }

    #[test]
    fn get_by_ids_empty_returns_empty() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();
        let repo = ChunkRepository::new(&conn);
        let result = repo.get_by_ids(&[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn get_leaf_content_for_doc() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();

        let doc_repo = DocumentRepository::new(&conn);
        let doc_id = doc_repo
            .create("file://test4.txt", "text/plain", "cksum_4", None, "{}")
            .unwrap();

        let repo = ChunkRepository::new(&conn);
        let parents = vec![make_chunk(0, "parent", None)];
        let parent_ids = repo.bulk_insert_parents(&doc_id, &parents).unwrap();
        let leaves = vec![
            make_chunk(1, "leaf", Some(0)),
            make_chunk(2, "leaf", Some(0)),
        ];
        repo.bulk_insert_leaves(&doc_id, &leaves, &parent_ids).unwrap();

        let contents = repo.get_leaf_content_for_doc(&doc_id).unwrap();
        assert_eq!(contents.len(), 2);
    }
}
