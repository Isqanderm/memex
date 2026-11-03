use std::sync::Arc;

use rusqlite::Connection;
use tracing::{info, warn};

use crate::db::repositories::memories::{MemoryRepository, NewMemory};
use crate::db::repositories::memory_jobs::MemoryJobRepository;
use crate::db::repositories::memory_sources::MemorySourceRepository;
use crate::ingestion::embeddings::EmbeddingClient;
use crate::ingestion::language::LanguageDetector;
use crate::search::tantivy_fts::TantivyStore;
use crate::search::vectors::VectorStore;

use super::extractor::FactExtractor;

const OBSERVE_PROMPT: &str = r#"Extract new facts from the following conversation that are worth remembering about the user.
Focus on personal information, preferences, decisions, and context that would be useful in future interactions.

Conversation:
{conversation}

Summarize the key new facts about the user from this conversation."#;

/// Version of what the memory FTS indexes per fact; bump to force a rebuild at startup.
/// 2: facts are indexed without the leading "User " template prefix.
pub const MEMORY_INDEX_VERSION: &str = "2";
/// Marker file in the memory index directory holding [`MEMORY_INDEX_VERSION`].
pub const INDEX_VERSION_FILE: &str = "INDEX_VERSION";

/// Index every active fact (buffered, one commit). A fact that fails to index is
/// logged and skipped. Returns the number of facts indexed.
fn index_all_active(conn: &Connection, fts: &TantivyStore) -> anyhow::Result<usize> {
    let facts = MemoryRepository::new(conn).get_all_active(None)?;
    if facts.is_empty() {
        return Ok(0);
    }
    let mut n = 0;
    for m in &facts {
        let lang = LanguageDetector.detect(&m.content);
        match fts.add_chunk(&m.id, "memory", &lang, fts_text(&m.content)) {
            Ok(()) => n += 1,
            Err(e) => warn!("Failed to index memory {} for full-text search: {e}", m.id),
        }
    }
    fts.commit()?;
    Ok(n)
}

/// Index every active fact into an empty memory FTS index; commits once.
///
/// Returns the number of facts indexed (0 when the index already has documents).
pub fn backfill_memory_fts(conn: &Connection, fts: &TantivyStore) -> anyhow::Result<usize> {
    if fts.num_docs() > 0 {
        return Ok(0);
    }
    index_all_active(conn, fts)
}

/// Bring the memory FTS index in line with [`MEMORY_INDEX_VERSION`].
///
/// Marker missing or different: clear the index, reindex all active facts, write the
/// marker. Marker current: backfill only when the index is empty.
/// Returns the number of facts indexed.
pub fn ensure_memory_index(
    conn: &Connection,
    fts: &TantivyStore,
    index_dir: &std::path::Path,
) -> anyhow::Result<usize> {
    let marker = index_dir.join(INDEX_VERSION_FILE);
    let current = std::fs::read_to_string(&marker).ok();
    if current.as_deref().map(str::trim) == Some(MEMORY_INDEX_VERSION) {
        let n = backfill_memory_fts(conn, fts)?;
        if n > 0 {
            info!("Backfilled {n} memory facts into the keyword index");
        }
        return Ok(n);
    }
    fts.clear()?;
    let n = index_all_active(conn, fts)?;
    std::fs::write(&marker, MEMORY_INDEX_VERSION)?;
    info!("Rebuilt memory keyword index (version {MEMORY_INDEX_VERSION}): {n} facts");
    Ok(n)
}

/// Text indexed in the memory FTS for a fact: the leading "User " / "Пользователь "
/// template prefix is dropped so the shared token does not dominate BM25.
/// The stored content is unchanged.
pub fn fts_text(content: &str) -> &str {
    ["User ", "Пользователь "]
        .iter()
        .find_map(|p| content.strip_prefix(p))
        .unwrap_or(content)
}

pub struct RememberResult {
    pub facts_extracted: usize,
    pub memories_updated: usize,
}

pub struct MemorySvc {
    pub extractor: Arc<FactExtractor>,
    pub embed: Arc<EmbeddingClient>,
    pub vectors: Arc<VectorStore>,
    /// Full-text index of memory facts; `chunk_id` = memory id.
    pub memory_fts: Arc<TantivyStore>,
    /// Cosine similarity above which an existing fact is a conflict candidate.
    pub conflict_similarity: f32,
}

impl MemorySvc {
    pub fn new(
        extractor: Arc<FactExtractor>,
        embed: Arc<EmbeddingClient>,
        vectors: Arc<VectorStore>,
        memory_fts: Arc<TantivyStore>,
        conflict_similarity: f32,
    ) -> Self {
        Self {
            extractor,
            embed,
            vectors,
            memory_fts,
            conflict_similarity,
        }
    }

    /// Startup check of the memory FTS index; see [`ensure_memory_index`].
    pub fn ensure_memory_index(
        &self,
        conn: &Connection,
        index_dir: &std::path::Path,
    ) -> anyhow::Result<usize> {
        ensure_memory_index(conn, &self.memory_fts, index_dir)
    }

    /// Buffer a fact for the memory FTS index (committed by the caller).
    fn index_fact(&self, memory_id: &str, content: &str) -> anyhow::Result<()> {
        let lang = LanguageDetector.detect(content);
        self.memory_fts.add_chunk(memory_id, "memory", &lang, fts_text(content))
    }

    /// Commit buffered FTS changes; failures are logged, SQLite stays the source of truth.
    fn commit_fts(&self) {
        if let Err(e) = self.memory_fts.commit() {
            warn!("Failed to commit memory full-text index: {e}");
        }
    }

    pub async fn remember(
        &self,
        conn: &Connection,
        text: &str,
        source: &str,
        scope: &str,
    ) -> anyhow::Result<RememberResult> {
        // memory_sources.kind is a closed set; memories.source stays free-form.
        let kind = match source {
            "explicit" | "conversation" | "document" => source,
            _ => "explicit",
        };
        let (source_id, _) = MemorySourceRepository::new(conn).get_or_create(kind, text)?;
        self.remember_with_source(conn, text, source, scope, &source_id)
            .await
    }

    /// Run extraction over an already stored source (used by the extraction worker).
    ///
    /// Conversation sources hold the raw conversation; the observe prompt is applied here.
    pub async fn remember_from_source(
        &self,
        conn: &Connection,
        source_id: &str,
        source: &str,
        scope: &str,
    ) -> anyhow::Result<RememberResult> {
        let stored = MemorySourceRepository::new(conn)
            .get_by_id(source_id)?
            .ok_or_else(|| anyhow::anyhow!("memory source {source_id} not found"))?;
        let text = if source == "conversation" {
            OBSERVE_PROMPT.replace("{conversation}", &stored.text)
        } else {
            stored.text
        };
        self.remember_with_source(conn, &text, source, scope, source_id)
            .await
    }

    /// Queue fact extraction for a conversation; returns `(job_id, created)`.
    ///
    /// The raw conversation is stored as a `memory_sources` row (deduplicated by checksum).
    /// When that source already has a job in the same scope that did not fail, its id is
    /// returned with `created = false`; a failed job is retried with a new job.
    pub fn enqueue_observe(
        &self,
        conn: &Connection,
        conversation: &str,
        scope: &str,
    ) -> rusqlite::Result<(String, bool)> {
        let (source_id, _) =
            MemorySourceRepository::new(conn).get_or_create("conversation", conversation)?;
        let jobs = MemoryJobRepository::new(conn);
        if let Some(job) = jobs.get_latest_by_source_ref_and_scope(&source_id, scope)? {
            if job.status != "error" {
                return Ok((job.id, false));
            }
        }
        Ok((jobs.create(&source_id, "conversation", scope)?, true))
    }

    async fn remember_with_source(
        &self,
        conn: &Connection,
        text: &str,
        source: &str,
        scope: &str,
        source_id: &str,
    ) -> anyhow::Result<RememberResult> {
        let facts = self.extractor.extract_facts(text).await?;
        let facts_extracted = facts.len();
        let mut memories_updated = 0;

        for fact in &facts {
            // Embed the fact content
            let vector = match self.embed.embed_query(&fact.content) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Failed to embed fact '{}': {e}", fact.content);
                    continue;
                }
            };

            // Find similar existing memories
            let similar_hits = self
                .vectors
                .find_similar_memories(conn, &vector, 5, self.conflict_similarity)
                .unwrap_or_default();

            // Load similar memory contents
            let repo = MemoryRepository::new(conn);
            let mut existing: Vec<(String, String)> = Vec::new();
            for hit in &similar_hits {
                if let Ok(Some(mem)) = repo.get_by_id(&hit.id) {
                    existing.push((mem.id, mem.content));
                }
            }

            // Resolve relations with existing memories
            let mut parent_id: Option<String> = None;
            let mut relation: Option<String> = None;

            if !existing.is_empty() {
                match self
                    .extractor
                    .resolve_relations(&fact.content, &existing)
                    .await
                {
                    Ok(relations) => {
                        for rel in &relations {
                            match rel.relation.as_str() {
                                "updates" => {
                                    // Deactivate the old memory
                                    if let Err(e) = repo.deactivate(&rel.memory_id) {
                                        warn!("Failed to deactivate memory {}: {e}", rel.memory_id);
                                    } else {
                                        memories_updated += 1;
                                        if let Err(e) = self.memory_fts.delete_by_chunk_id(&rel.memory_id) {
                                            warn!("Failed to deindex memory {}: {e}", rel.memory_id);
                                        }
                                    }
                                    // Set parent only if not already set
                                    if parent_id.is_none() {
                                        parent_id = Some(rel.memory_id.clone());
                                        relation = Some("updates".to_string());
                                    }
                                }
                                "extends" | "derives"
                                    // Set parent only if not already set
                                    if parent_id.is_none() => {
                                        parent_id = Some(rel.memory_id.clone());
                                        relation = Some(rel.relation.clone());
                                    }
                                _ => {} // "new" — no action needed
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to resolve relations: {e}");
                    }
                }
            }

            // Format forget_after as ISO string if present
            let forget_after_str = fact
                .forget_after
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());

            // Create the new memory
            let memory_id = repo.create(&NewMemory {
                content: &fact.content,
                source,
                source_id: Some(source_id),
                scope,
                forget_after: forget_after_str.as_deref(),
                relation: relation.as_deref(),
                parent_id: parent_id.as_deref(),
                category: fact.category.as_deref(),
                project: fact.project.as_deref(),
            })?;

            info!("Created memory {memory_id}: {}", fact.content);

            // Insert vector
            if let Err(e) = self.vectors.insert_memory(conn, &memory_id, &vector) {
                warn!("Failed to insert memory vector for {memory_id}: {e}");
            }
            if let Err(e) = self.index_fact(&memory_id, &fact.content) {
                warn!("Failed to index memory {memory_id} for full-text search: {e}");
            }
        }

        self.commit_fts();

        Ok(RememberResult {
            facts_extracted,
            memories_updated,
        })
    }

    pub async fn forget(
        &self,
        conn: &Connection,
        memory_id: &str,
    ) -> anyhow::Result<bool> {
        let repo = MemoryRepository::new(conn);
        // Check that the memory exists
        if repo.get_by_id(memory_id)?.is_none() {
            return Ok(false);
        }

        // Deactivate in the DB
        let deactivated = repo.deactivate(memory_id)?;

        // Remove the vector
        if let Err(e) = self.vectors.delete_memory(conn, memory_id) {
            warn!("Failed to delete vector for memory {memory_id}: {e}");
        }
        if let Err(e) = self.memory_fts.delete_by_chunk_id(memory_id) {
            warn!("Failed to deindex memory {memory_id}: {e}");
        }
        self.commit_fts();

        Ok(deactivated)
    }

    pub fn expire_stale(&self, conn: &Connection) -> anyhow::Result<usize> {
        let repo = MemoryRepository::new(conn);
        let n = repo.expire_stale()?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repositories::memories::NewMemory;

    #[test]
    fn fts_text_strips_the_user_template_prefix() {
        assert_eq!(fts_text("User lives in Novi Sad"), "lives in Novi Sad");
        assert_eq!(fts_text("Пользователь живёт в Нови-Саде"), "живёт в Нови-Саде");
        assert_eq!(fts_text("User's MEMEX_URL is http://memex:8000"), "User's MEMEX_URL is http://memex:8000");
        assert_eq!(fts_text("Prefers dark roast"), "Prefers dark roast");
        assert_eq!(fts_text("Username is alex"), "Username is alex");
    }

    struct Fixture {
        dir: tempfile::TempDir,
        pool: crate::db::pool::DbPool,
        fts: TantivyStore,
        ids: Vec<String>,
    }

    /// Two active facts; only the first is in the index, with the stale "User " prefix.
    fn stale_index() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let pool = crate::db::pool::build_pool(dir.path().join("t.db").to_str().unwrap()).unwrap();
        let fts = TantivyStore::open(dir.path().join("memories").to_str().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);
        let ids: Vec<String> = ["User lives in Novi Sad", "User uses Python"]
            .iter()
            .map(|c| {
                repo.create(&NewMemory {
                    content: c,
                    source: "explicit",
                    source_id: None,
                    scope: "default",
                    forget_after: None,
                    relation: None,
                    parent_id: None,
                    category: None,
                    project: None,
                })
                .unwrap()
            })
            .collect();
        fts.add_chunk(&ids[0], "memory", "en", "User lives in Novi Sad").unwrap();
        fts.commit().unwrap();
        drop(conn);
        Fixture { dir, pool, fts, ids }
    }

    fn ids_for(fts: &TantivyStore, q: &str) -> Vec<String> {
        fts.search(q, "en", 10).unwrap().into_iter().map(|h| h.chunk_id).collect()
    }

    #[test]
    fn missing_index_version_rebuilds_the_index() {
        let f = stale_index();
        let conn = f.pool.get().unwrap();
        let index_dir = f.dir.path().join("memories");

        let n = ensure_memory_index(&conn, &f.fts, &index_dir).unwrap();

        assert_eq!(n, 2);
        assert_eq!(ids_for(&f.fts, "Python"), vec![f.ids[1].clone()]);
        assert_eq!(ids_for(&f.fts, "Novi"), vec![f.ids[0].clone()]);
        assert!(ids_for(&f.fts, "user").is_empty(), "stale prefixed entry must be gone");
        let marker = std::fs::read_to_string(index_dir.join(INDEX_VERSION_FILE)).unwrap();
        assert_eq!(marker.trim(), MEMORY_INDEX_VERSION);
    }

    #[test]
    fn current_index_version_does_not_reindex() {
        let f = stale_index();
        let conn = f.pool.get().unwrap();
        let index_dir = f.dir.path().join("memories");
        std::fs::write(index_dir.join(INDEX_VERSION_FILE), MEMORY_INDEX_VERSION).unwrap();

        let n = ensure_memory_index(&conn, &f.fts, &index_dir).unwrap();

        assert_eq!(n, 0);
        assert!(ids_for(&f.fts, "Python").is_empty());
        assert_eq!(f.fts.num_docs(), 1);
    }
}
