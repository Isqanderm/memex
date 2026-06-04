use rusqlite::Connection;

use crate::db::repositories::{documents::DocumentRepository, chunks::ChunkRepository};
use crate::ingestion::adapters::ParsedDocument;
use crate::ingestion::chunker::ChunkData;
use crate::search::{tantivy_fts::TantivyStore, vectors::VectorStore};

pub struct IndexingStage;

impl IndexingStage {
    /// Index a parsed document and all its chunks.
    ///
    /// 1. Creates a document row.
    /// 2. Inserts parent chunks; obtains temp-index → UUID map.
    /// 3. Inserts leaf chunks (with resolved parent refs); obtains (chunk_id, embedding) pairs.
    /// 4. Adds each leaf to the tantivy FTS index.
    /// 5. Inserts each leaf embedding into the vector store.
    /// 6. Commits the tantivy writer.
    ///
    /// Returns the new document ID.
    pub fn index(
        &self,
        conn: &Connection,
        tantivy: &TantivyStore,
        vectors: &VectorStore,
        parsed: &ParsedDocument,
        chunks: &[ChunkData],
        checksum: &str,
    ) -> anyhow::Result<String> {
        // 1. Create document row.
        let doc_repo = DocumentRepository::new(conn);
        let metadata_str = parsed.metadata.to_string();
        let doc_id = doc_repo.create(
            &parsed.source,
            &parsed.mime_type,
            checksum,
            parsed.title.as_deref(),
            &metadata_str,
        )?;

        // 2 & 3. Split chunks into parents / leaves, insert.
        let parents: Vec<_> = chunks.iter().filter(|c| c.chunk_role == "parent").cloned().collect();
        let leaves: Vec<_> = chunks.iter().filter(|c| c.chunk_role == "leaf").cloned().collect();

        let chunk_repo = ChunkRepository::new(conn);
        let parent_ids = chunk_repo.bulk_insert_parents(&doc_id, &parents)?;
        let leaf_vectors = chunk_repo.bulk_insert_leaves(&doc_id, &leaves, &parent_ids)?;

        // 4. Collect all leaf chunk IDs from the DB (ordered by chunk_index)
        //    and add each leaf to the tantivy FTS index.
        let leaf_db_ids = chunk_repo.get_leaf_ids_for_doc(&doc_id)?;

        // Build a map: chunk_index → db_id for the leaves we just inserted.
        // leaf_db_ids is ordered by chunk_index (ORDER BY chunk_index in the query),
        // and our leaves slice is also ordered by chunk_index.
        for (leaf, leaf_db_id) in leaves.iter().zip(leaf_db_ids.iter()) {
            let lang = leaf.language.as_str();
            tantivy.add_chunk(leaf_db_id, &doc_id, lang, &leaf.content)?;
        }

        // 5. Insert embeddings into the vector store.
        for (chunk_id, embedding) in &leaf_vectors {
            vectors.insert_chunk(conn, chunk_id, embedding)?;
        }

        // 6. Commit tantivy.
        tantivy.commit()?;

        Ok(doc_id)
    }
}
