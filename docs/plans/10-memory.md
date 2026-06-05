# Task 10: Memory Subsystem

**Goal:** FactExtractor (LLM prompt), MemoryService (remember/observe/forget), ProfileService, MemoryWorker. Полный порт Python memory модуля.

**Files:**
- Create: `rust/src/memory/extractor.rs`
- Create: `rust/src/memory/service.rs`
- Create: `rust/src/memory/profile.rs`
- Create: `rust/src/memory/worker.rs`
- Modify: `rust/src/memory/mod.rs`

---

### Task 10.1: FactExtractor

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/memory/extractor.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_facts_from_json() {
        let json = r#"{"facts": [
            {"content": "User works at Acme Corp", "category": "preference"},
            {"content": "User meeting tomorrow", "forget_after": "2026-06-05T09:00:00", "category": "reminder"}
        ]}"#;
        let facts = parse_facts_json(json).unwrap();
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].content, "User works at Acme Corp");
        assert_eq!(facts[0].category.as_deref(), Some("preference"));
        assert!(facts[1].forget_after.is_some());
    }

    #[test]
    fn parse_facts_handles_invalid_json() {
        let facts = parse_facts_json("not json at all").unwrap();
        assert!(facts.is_empty());
    }

    #[test]
    fn parse_relations_from_json() {
        let json = r#"{"relations": [
            {"id": "550e8400-e29b-41d4-a716-446655440000", "type": "updates"}
        ]}"#;
        let rels = parse_relations_json(json).unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, "updates");
    }
}
```

- [ ] **Шаг 2: Запустить — убедиться что FAIL**

```bash
cd rust && cargo test extractor 2>&1 | tail -5
```

- [ ] **Шаг 3: Реализовать extractor.rs**

```rust
use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use crate::llm::LlmProvider;

const EXTRACT_PROMPT: &str = r#"Extract atomic facts about the user from the following text.
Rules:
- Each fact is one statement, no pronouns — use "User" as subject.
- Include: identity, skills, location, work, relationships, projects, preferences, events.
- Exclude: opinions and emotional reactions, third-party info.
- Normalize state: prefer "User uses X" over "User switched from Y to X".
- Time-bound facts: add "forget_after" as ISO datetime. Permanent facts: omit "forget_after".
- Set "category" to: research | reminder | decision | preference | insight (or omit if unclear).
- Set "project" to project/context name if applicable. Omit if unclear.

Text: {text}

Return JSON only:
{"facts": [{"content": "...", "forget_after": "...or omit", "category": "...or omit", "project": "...or omit"}]}"#;

const RESOLVE_PROMPT: &str = r#"New fact: "{new_fact}"

Existing similar facts:
{existing}

For each existing fact determine the relation:
- updates: new fact contradicts and supersedes the old one
- extends: new fact adds detail without contradiction
- derives: new fact is a logical conclusion from the old one
- new: not meaningfully related

Return JSON only:
{"relations": [{"id": "...", "type": "updates|extends|derives|new"}]}"#;

#[derive(Debug)]
pub struct ExtractedFact {
    pub content: String,
    pub forget_after: Option<DateTime<Utc>>,
    pub category: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug)]
pub struct RelationResult {
    pub memory_id: String,
    pub relation: String,
}

const VALID_CATEGORIES: &[&str] = &["research", "reminder", "insight", "decision", "preference"];

pub struct FactExtractor {
    llm: Arc<dyn LlmProvider>,
}

impl FactExtractor {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self { Self { llm } }

    pub async fn extract_facts(&self, text: &str) -> anyhow::Result<Vec<ExtractedFact>> {
        let prompt = EXTRACT_PROMPT.replace("{text}", text);
        let response = self.llm.complete(&prompt).await?;
        Ok(parse_facts_json(&response.answer).unwrap_or_default())
    }

    pub async fn resolve_relations(
        &self,
        new_fact: &str,
        existing: &[(String, String)], // (id, content)
    ) -> anyhow::Result<Vec<RelationResult>> {
        if existing.is_empty() {
            return Ok(vec![]);
        }
        let existing_str: String = existing
            .iter()
            .map(|(id, content)| format!("  id={id}: \"{content}\""))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = RESOLVE_PROMPT
            .replace("{new_fact}", new_fact)
            .replace("{existing}", &existing_str);

        let response = self.llm.complete(&prompt).await?;
        Ok(parse_relations_json(&response.answer).unwrap_or_default())
    }
}

#[derive(Deserialize)]
struct FactsJson {
    #[serde(default)]
    facts: Vec<FactEntry>,
}

#[derive(Deserialize)]
struct FactEntry {
    content: String,
    forget_after: Option<String>,
    category: Option<String>,
    project: Option<String>,
}

pub fn parse_facts_json(text: &str) -> anyhow::Result<Vec<ExtractedFact>> {
    let start = text.find('{').unwrap_or(0);
    let end = text.rfind('}').map(|i| i + 1).unwrap_or(text.len());
    if start >= end { return Ok(vec![]); }

    let data: FactsJson = match serde_json::from_str(&text[start..end]) {
        Ok(d) => d,
        Err(_) => return Ok(vec![]),
    };

    let facts = data.facts.into_iter().map(|f| {
        let forget_after = f.forget_after.as_deref().and_then(|s| {
            DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
        });
        let category = f.category.filter(|c| VALID_CATEGORIES.contains(&c.as_str()));
        let project = f.project.map(|p| p.chars().take(100).collect());
        ExtractedFact { content: f.content, forget_after, category, project }
    }).collect();

    Ok(facts)
}

#[derive(Deserialize)]
struct RelationsJson {
    #[serde(default)]
    relations: Vec<RelEntry>,
}

#[derive(Deserialize)]
struct RelEntry {
    id: String,
    #[serde(rename = "type")]
    rel_type: String,
}

pub fn parse_relations_json(text: &str) -> anyhow::Result<Vec<RelationResult>> {
    let start = text.find('{').unwrap_or(0);
    let end = text.rfind('}').map(|i| i + 1).unwrap_or(text.len());
    if start >= end { return Ok(vec![]); }

    let data: RelationsJson = match serde_json::from_str(&text[start..end]) {
        Ok(d) => d,
        Err(_) => return Ok(vec![]),
    };

    Ok(data.relations.into_iter().map(|r| RelationResult {
        memory_id: r.id,
        relation: r.rel_type,
    }).collect())
}

#[cfg(test)]
mod tests { /* из Шага 1 */ }
```

- [ ] **Шаг 4: Запустить тесты**

```bash
cd rust && cargo test extractor 2>&1
```

Ожидаем: 3 теста зелёных.

---

### Task 10.2: MemoryService

- [ ] **Шаг 1: Реализовать service.rs**

```rust
// rust/src/memory/service.rs
use std::sync::Arc;
use rusqlite::Connection;
use tracing::debug;

use crate::db::repositories::memories::MemoryRepository;
use crate::ingestion::embeddings::EmbeddingClient;
use crate::memory::extractor::FactExtractor;
use crate::search::vectors::VectorStore;

pub struct RememberResult {
    pub facts_extracted: usize,
    pub memories_updated: usize,
}

pub struct MemoryService {
    pub repo: MemoryRepository<'static>, // Lifetime управляется через Connection arc
    pub extractor: Arc<FactExtractor>,
    pub embed: Arc<EmbeddingClient>,
    pub vectors: Arc<VectorStore>,
}

/// Версия MemoryService без self lifetime — использует conn параметром.
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
        Self { extractor, embed, vectors }
    }

    pub async fn remember(
        &self,
        conn: &Connection,
        text: &str,
        source: &str,
    ) -> anyhow::Result<RememberResult> {
        let facts = self.extractor.extract_facts(text).await?;
        let mut facts_extracted = facts.len();
        let mut memories_updated = 0usize;

        for fact in facts {
            let vector = self.embed.embed_query(&fact.content)?;

            // Поиск похожих для conflict detection (threshold cosine_sim >= 0.60)
            let similar_hits = self.vectors.find_similar_memories(conn, &vector, 5, 0.60)?;
            let repo = MemoryRepository::new(conn);
            let existing: Vec<(String, String)> = similar_hits.iter()
                .filter_map(|h| {
                    repo.get_by_id(&h.id).ok()?.map(|m| (m.id, m.content))
                })
                .collect();

            let relations = self.extractor.resolve_relations(&fact.content, &existing).await?;

            let mut parent_id: Option<String> = None;
            let mut relation_type: Option<String> = None;

            for rel in &relations {
                if rel.relation == "updates" {
                    repo.deactivate(&rel.memory_id)?;
                    parent_id = Some(rel.memory_id.clone());
                    relation_type = Some("updates".to_string());
                    memories_updated += 1;
                } else if rel.relation == "extends" || rel.relation == "derives" {
                    if parent_id.is_none() {
                        parent_id = Some(rel.memory_id.clone());
                        relation_type = Some(rel.relation.clone());
                    }
                }
            }

            let forget_after_str = fact.forget_after
                .map(|dt| dt.to_rfc3339());

            let memory_id = repo.create(
                &fact.content,
                text,
                source,
                parent_id.as_deref(),
                relation_type.as_deref(),
                forget_after_str.as_deref(),
                fact.category.as_deref(),
            )?;

            self.vectors.insert_memory(conn, &memory_id, &vector)?;
        }

        Ok(RememberResult { facts_extracted, memories_updated })
    }

    pub async fn observe(
        &self,
        conn: &Connection,
        conversation: &str,
    ) -> anyhow::Result<RememberResult> {
        let prompt = format!(
            "What new personal facts about the user did you learn in this conversation?\n\
             Return only new information, not a recap.\n\nConversation:\n{conversation}"
        );
        self.remember(conn, &prompt, "conversation").await
    }

    pub async fn forget(
        &self,
        conn: &Connection,
        memory_id: &str,
    ) -> anyhow::Result<bool> {
        let repo = MemoryRepository::new(conn);
        if repo.get_by_id(memory_id)?.is_none() {
            return Ok(false);
        }
        repo.deactivate(memory_id)?;
        self.vectors.delete_memory(conn, memory_id)?;
        Ok(true)
    }

    pub fn expire_stale(&self, conn: &Connection) -> anyhow::Result<usize> {
        let repo = MemoryRepository::new(conn);
        Ok(repo.expire_stale()?)
    }
}
```

---

### Task 10.3: ProfileService

- [ ] **Шаг 1: Реализовать profile.rs**

```rust
// rust/src/memory/profile.rs
use std::sync::Arc;
use chrono::{Duration, Utc};
use crate::db::repositories::memories::Memory;
use crate::llm::LlmProvider;

const STATIC_THRESHOLD_DAYS: i64 = 30;

const PROFILE_PROMPT: &str = r#"Summarize the following facts about a user into a concise profile (2-4 sentences max, ≤150 tokens).
Write in third person. Include only factual information from the list.

Facts:
{facts}

Profile summary:"#;

pub struct UserProfile {
    pub static_summary: String,
    pub dynamic_summary: String,
    pub raw_count: usize,
}

pub struct ProfileService {
    llm: Arc<dyn LlmProvider>,
}

impl ProfileService {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self { Self { llm } }

    pub async fn build_profile(&self, memories: &[Memory]) -> anyhow::Result<UserProfile> {
        if memories.is_empty() {
            return Ok(UserProfile {
                static_summary: String::new(),
                dynamic_summary: String::new(),
                raw_count: 0,
            });
        }

        let cutoff = Utc::now() - Duration::days(STATIC_THRESHOLD_DAYS);
        let (static_mems, dynamic_mems): (Vec<_>, Vec<_>) = memories.iter().partition(|m| {
            // parse created_at as UTC datetime
            chrono::DateTime::parse_from_rfc3339(&m.created_at)
                .map(|dt| dt.with_timezone(&Utc) < cutoff)
                .unwrap_or(false)
        });

        let static_summary = if static_mems.is_empty() {
            String::new()
        } else {
            self.summarize(&static_mems).await?
        };

        let dynamic_summary = if dynamic_mems.is_empty() {
            String::new()
        } else {
            self.summarize(&dynamic_mems).await?
        };

        Ok(UserProfile {
            static_summary,
            dynamic_summary,
            raw_count: memories.len(),
        })
    }

    async fn summarize(&self, memories: &[&Memory]) -> anyhow::Result<String> {
        if memories.is_empty() { return Ok(String::new()); }
        let facts: String = memories.iter()
            .map(|m| format!("- {}", m.content))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = PROFILE_PROMPT.replace("{facts}", &facts);
        let response = self.llm.complete(&prompt).await?;
        Ok(response.answer.trim().to_string())
    }
}
```

---

### Task 10.4: Memory Worker (expiry loop)

- [ ] **Шаг 1: Реализовать worker.rs**

```rust
// rust/src/memory/worker.rs
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::db::pool::DbPool;
use crate::memory::service::MemorySvc;

pub struct MemoryExpiryWorker {
    pub pool: Arc<DbPool>,
    pub svc: Arc<MemorySvc>,
    pub interval_secs: u64,
}

impl MemoryExpiryWorker {
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    info!("MemoryExpiryWorker shutting down");
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(self.interval_secs)) => {
                    let pool = self.pool.clone();
                    let svc = self.svc.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        let conn = pool.get()?;
                        svc.expire_stale(&conn)
                    }).await;

                    match result {
                        Ok(Ok(count)) if count > 0 => info!("Expired {count} stale memories"),
                        Ok(Err(e)) => warn!("Memory expiry error: {e}"),
                        _ => {}
                    }
                }
            }
        }
    }
}
```

- [ ] **Шаг 2: Обновить memory/mod.rs**

```rust
// rust/src/memory/mod.rs
pub mod extractor;
pub mod profile;
pub mod service;
pub mod worker;

pub use extractor::FactExtractor;
pub use profile::ProfileService;
pub use service::MemorySvc;
pub use worker::MemoryExpiryWorker;
```

- [ ] **Шаг 3: Запустить все тесты**

```bash
cd rust && cargo test 2>&1
```

Ожидаем: все юнит-тесты проходят.

- [ ] **Шаг 4: Коммит**

```bash
git add rust/src/memory/
git commit -m "feat(rust): memory subsystem — FactExtractor, MemoryService, ProfileService, ExpiryWorker"
```
