use std::sync::Arc;

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::ingestion::adapters::AdapterRegistry;
use crate::ingestion::chunker::SmallToBigChunker;
use crate::ingestion::embeddings::EmbeddingClient;
use crate::ingestion::indexing::IndexingStage;
use crate::ingestion::language::LanguageDetector;
use crate::search::tantivy_fts::TantivyStore;
use crate::search::vectors::VectorStore;

pub struct IngestionPipeline {
    pub adapters: AdapterRegistry,
    pub chunker: SmallToBigChunker,
    pub embed: Arc<EmbeddingClient>,
    pub lang: LanguageDetector,
    pub batch_size: usize,
}

impl IngestionPipeline {
    /// Process a single file end-to-end:
    /// parse → chunk → detect language → embed → index.
    ///
    /// Returns the newly created document ID.
    pub fn process(
        &self,
        conn: &Connection,
        tantivy: &TantivyStore,
        vectors: &VectorStore,
        source_path: &str,
        checksum: &str,
    ) -> anyhow::Result<String> {
        let path = std::path::Path::new(source_path);

        // 1. Detect MIME type.
        let mime_type = mime_guess::from_path(path)
            .first_raw()
            .unwrap_or("application/octet-stream");

        // 2. Parse document.
        let parsed = self.adapters.parse(path, mime_type)?;

        // 3. Chunk.
        let mut chunks = self.chunker.chunk(&parsed.sections);

        // 4. Detect language for each chunk (sample first 200 chars of content).
        for chunk in &mut chunks {
            let sample: &str = &chunk.content[..chunk.content.len().min(200)];
            chunk.language = self.lang.detect(sample);
        }

        // 5. Gather leaf chunk texts in order for batch embedding.
        let leaf_indices: Vec<usize> = chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| c.chunk_role == "leaf")
            .map(|(i, _)| i)
            .collect();

        // Embed in batches.
        let texts: Vec<&str> = leaf_indices
            .iter()
            .map(|&i| chunks[i].content.as_str())
            .collect();

        let mut all_embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        for batch in texts.chunks(self.batch_size) {
            let embeddings = self.embed.embed_passages(batch)?;
            all_embeddings.extend(embeddings);
        }

        // Assign embeddings back to leaf chunks.
        for (leaf_pos, &chunk_idx) in leaf_indices.iter().enumerate() {
            if let Some(emb) = all_embeddings.get(leaf_pos) {
                chunks[chunk_idx].embedding = Some(emb.clone());
            }
        }

        // 6. Index (DB + tantivy + vectors).
        let stage = IndexingStage;
        let doc_id = stage.index(conn, tantivy, vectors, &parsed, &chunks, checksum)?;

        Ok(doc_id)
    }
}

/// Compute the SHA-256 hex digest of a file.
pub fn file_checksum(path: &str) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("Cannot read file for checksum {path}: {e}"))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash = hasher.finalize();
    Ok(hex::encode(hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn file_checksum_is_deterministic() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        let path = f.path().to_str().unwrap().to_string();

        let c1 = file_checksum(&path).unwrap();
        let c2 = file_checksum(&path).unwrap();
        // Same file → same hash
        assert_eq!(c1, c2);
        // SHA-256 always produces a 64-character hex string
        assert_eq!(c1.len(), 64, "checksum should be 64 hex chars");
        // Must be valid lowercase hex
        assert!(c1.chars().all(|c| c.is_ascii_hexdigit()), "checksum should be hex");
    }

    #[test]
    fn file_checksum_different_for_different_content() {
        let mut f1 = NamedTempFile::new().unwrap();
        f1.write_all(b"content A").unwrap();
        let mut f2 = NamedTempFile::new().unwrap();
        f2.write_all(b"content B").unwrap();

        let c1 = file_checksum(f1.path().to_str().unwrap()).unwrap();
        let c2 = file_checksum(f2.path().to_str().unwrap()).unwrap();
        assert_ne!(c1, c2);
    }
}
