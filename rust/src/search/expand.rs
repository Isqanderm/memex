use rusqlite::Connection;

use crate::db::repositories::chunks::{ChunkRepository, L2Chunk};
use crate::search::rrf::SearchHit;

/// Expand a list of search hits (which are leaves) to their parent (L2) chunks.
///
/// Extracts `parent_chunk_id` from each hit and delegates to
/// [`ChunkRepository::expand_to_l2`], which also joins document metadata.
///
/// Hits that have no `parent_chunk_id` are still passed through — the repo
/// uses `COALESCE(parent_chunk_id, id)` so the leaf itself is returned.
pub fn expand_to_l2(conn: &Connection, hits: &[SearchHit]) -> rusqlite::Result<Vec<L2Chunk>> {
    // Collect leaf chunk IDs (the repo resolves parents internally)
    let leaf_ids: Vec<String> = hits.iter().map(|h| h.chunk_id.clone()).collect();

    if leaf_ids.is_empty() {
        return Ok(vec![]);
    }

    let repo = ChunkRepository::new(conn);
    repo.expand_to_l2(&leaf_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use crate::db::repositories::documents::DocumentRepository;
    use crate::search::rrf::SearchHit;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, crate::db::pool::DbPool) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = build_pool(path.to_str().unwrap()).unwrap();
        (dir, pool)
    }

    #[test]
    fn expand_returns_parent_content() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();

        // Create document
        let doc_repo = DocumentRepository::new(&conn);
        let doc_id = doc_repo
            .create("file://test.txt", "text/plain", "cksum_expand", None, "{}")
            .unwrap();

        // Create parent chunk manually
        let parent_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO chunks (id, doc_id, parent_chunk_id, chunk_role, chunk_index, \
             section_heading, section_level, page_number, language, content) \
             VALUES (?1, ?2, NULL, 'parent', 0, NULL, 0, NULL, 'en', 'parent content here')",
            rusqlite::params![parent_id, doc_id],
        )
        .unwrap();

        // Create leaf chunk pointing to parent
        let leaf_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO chunks (id, doc_id, parent_chunk_id, chunk_role, chunk_index, \
             section_heading, section_level, page_number, language, content) \
             VALUES (?1, ?2, ?3, 'leaf', 1, NULL, 0, NULL, 'en', 'leaf content here')",
            rusqlite::params![leaf_id, doc_id, parent_id],
        )
        .unwrap();

        // Build a SearchHit pointing to the leaf
        let hit = SearchHit {
            chunk_id: leaf_id.clone(),
            content: "leaf content here".to_string(),
            parent_chunk_id: Some(parent_id.clone()),
            doc_id: doc_id.clone(),
            score: 0.9,
            section_heading: None,
            page_number: None,
        };

        let l2_chunks = expand_to_l2(&conn, &[hit]).unwrap();

        assert_eq!(l2_chunks.len(), 1, "should return exactly one L2 chunk");
        assert_eq!(
            l2_chunks[0].content, "parent content here",
            "L2 chunk should have parent's content"
        );
        assert_eq!(l2_chunks[0].chunk_id, parent_id);
    }

    #[test]
    fn expand_empty_hits_returns_empty() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();
        let result = expand_to_l2(&conn, &[]).unwrap();
        assert!(result.is_empty());
    }
}
