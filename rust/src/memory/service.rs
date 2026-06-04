use std::sync::Arc;

use rusqlite::Connection;
use tracing::{info, warn};

use crate::db::repositories::memories::MemoryRepository;
use crate::ingestion::embeddings::EmbeddingClient;
use crate::search::vectors::VectorStore;

use super::extractor::FactExtractor;

const OBSERVE_PROMPT: &str = r#"Extract new facts from the following conversation that are worth remembering about the user.
Focus on personal information, preferences, decisions, and context that would be useful in future interactions.

Conversation:
{conversation}

Summarize the key new facts about the user from this conversation."#;

pub struct RememberResult {
    pub facts_extracted: usize,
    pub memories_updated: usize,
}

pub struct MemorySvc {
    pub extractor: Arc<FactExtractor>,
    pub embed: Arc<EmbeddingClient>,
    pub vectors: Arc<VectorStore>,
}

impl MemorySvc {
    pub fn new(
        extractor: Arc<FactExtractor>,
        embed: Arc<EmbeddingClient>,
        vectors: Arc<VectorStore>,
    ) -> Self {
        Self {
            extractor,
            embed,
            vectors,
        }
    }

    pub async fn remember(
        &self,
        conn: &Connection,
        text: &str,
        source: &str,
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
                .find_similar_memories(conn, &vector, 5, 0.60)
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
                                    }
                                    // Set parent only if not already set
                                    if parent_id.is_none() {
                                        parent_id = Some(rel.memory_id.clone());
                                        relation = Some("updates".to_string());
                                    }
                                }
                                "extends" | "derives" => {
                                    // Set parent only if not already set
                                    if parent_id.is_none() {
                                        parent_id = Some(rel.memory_id.clone());
                                        relation = Some(rel.relation.clone());
                                    }
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
            let memory_id = repo.create(
                &fact.content,
                text,
                source,
                forget_after_str.as_deref(),
                relation.as_deref(),
                parent_id.as_deref(),
                fact.category.as_deref(),
                fact.project.as_deref(),
            )?;

            info!("Created memory {memory_id}: {}", fact.content);

            // Insert vector
            if let Err(e) = self.vectors.insert_memory(conn, &memory_id, &vector) {
                warn!("Failed to insert memory vector for {memory_id}: {e}");
            }
        }

        Ok(RememberResult {
            facts_extracted,
            memories_updated,
        })
    }

    pub async fn observe(
        &self,
        conn: &Connection,
        conversation: &str,
    ) -> anyhow::Result<RememberResult> {
        let prompt = OBSERVE_PROMPT.replace("{conversation}", conversation);
        self.remember(conn, &prompt, "conversation").await
    }

    pub async fn forget(
        &self,
        conn: &Connection,
        memory_id: &str,
    ) -> anyhow::Result<bool> {
        let repo = MemoryRepository::new(conn);
        // Check that the memory exists
        match repo.get_by_id(memory_id)? {
            None => return Ok(false),
            Some(_) => {}
        }

        // Deactivate in the DB
        let deactivated = repo.deactivate(memory_id)?;

        // Remove the vector
        if let Err(e) = self.vectors.delete_memory(conn, memory_id) {
            warn!("Failed to delete vector for memory {memory_id}: {e}");
        }

        Ok(deactivated)
    }

    pub fn expire_stale(&self, conn: &Connection) -> anyhow::Result<usize> {
        let repo = MemoryRepository::new(conn);
        let n = repo.expire_stale()?;
        Ok(n)
    }
}
