# Task 8: Retrieval Pipeline

**Goal:** Semantic search, RRF слияние, L2 expand (leaf→parent), RetrievalService. Прямой порт Python retrieval pipeline.

**Files:**
- Create: `rust/src/search/rrf.rs`
- Create: `rust/src/search/expand.rs`
- Create: `rust/src/search/memory_search.rs`
- Create: `rust/src/search/context.rs`
- Create: `rust/src/search/service.rs`
- Modify: `rust/src/search/mod.rs`

---

### Task 8.1: RRF Merge

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/search/rrf.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_merges_two_lists() {
        let semantic = vec![
            SearchHit { chunk_id: "a".to_string(), content: "".to_string(), parent_chunk_id: None, doc_id: "".to_string(), score: 0.9, section_heading: None, page_number: None },
            SearchHit { chunk_id: "b".to_string(), content: "".to_string(), parent_chunk_id: None, doc_id: "".to_string(), score: 0.8, section_heading: None, page_number: None },
        ];
        let bm25 = vec![
            SearchHit { chunk_id: "b".to_string(), content: "".to_string(), parent_chunk_id: None, doc_id: "".to_string(), score: 0.7, section_heading: None, page_number: None },
            SearchHit { chunk_id: "c".to_string(), content: "".to_string(), parent_chunk_id: None, doc_id: "".to_string(), score: 0.6, section_heading: None, page_number: None },
        ];
        let merged = rrf_merge(&semantic, &bm25, 60, 10);
        // "b" присутствует в обоих списках — должен быть первым
        assert_eq!(merged[0].chunk_id, "b");
        // Все уникальные chunk_id должны быть в результате
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn rrf_empty_inputs() {
        let merged = rrf_merge(&[], &[], 60, 10);
        assert!(merged.is_empty());
    }
}
```

- [ ] **Шаг 2: Реализовать rrf.rs**

```rust
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub chunk_id: String,
    pub content: String,
    pub parent_chunk_id: Option<String>,
    pub doc_id: String,
    pub score: f32,
    pub section_heading: Option<String>,
    pub page_number: Option<u32>,
}

/// Reciprocal Rank Fusion — объединяет два ranked списка.
pub fn rrf_merge(
    semantic_hits: &[SearchHit],
    bm25_hits: &[SearchHit],
    k: usize,
    top_n: usize,
) -> Vec<SearchHit> {
    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut hit_map: HashMap<String, SearchHit> = HashMap::new();

    for (rank, hit) in semantic_hits.iter().enumerate() {
        *scores.entry(hit.chunk_id.clone()).or_insert(0.0) += 1.0 / (rank + 1 + k) as f64;
        hit_map.entry(hit.chunk_id.clone()).or_insert_with(|| hit.clone());
    }

    for (rank, hit) in bm25_hits.iter().enumerate() {
        *scores.entry(hit.chunk_id.clone()).or_insert(0.0) += 1.0 / (rank + 1 + k) as f64;
        hit_map.entry(hit.chunk_id.clone()).or_insert_with(|| hit.clone());
    }

    let mut ranked: Vec<(String, f64)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    ranked.truncate(top_n);

    ranked
        .into_iter()
        .filter_map(|(chunk_id, _)| hit_map.remove(&chunk_id))
        .collect()
}

#[cfg(test)]
mod tests { /* из Шага 1 */ }
```

---

### Task 8.2: L2 Expand (leaf → parent)

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/search/expand.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use crate::db::repositories::{chunks::ChunkRepository, documents::DocumentRepository};
    use tempfile::tempdir;

    #[test]
    fn expand_returns_parent_content() {
        let pool = build_pool(tempdir().unwrap().path().join("t.db").to_str().unwrap()).unwrap();
        let conn = pool.get().unwrap();

        let doc_repo = DocumentRepository::new(&conn);
        let doc_id = doc_repo.create("test.txt", "text/plain", "ck1", None, "{}").unwrap();

        // Создаём parent и leaf напрямую через SQL
        conn.execute(
            "INSERT INTO chunks (id, doc_id, chunk_role, chunk_index, language, content)
             VALUES ('parent-1', ?1, 'parent', 0, 'en', 'Parent content here')",
            rusqlite::params![doc_id],
        ).unwrap();
        conn.execute(
            "INSERT INTO chunks (id, doc_id, parent_chunk_id, chunk_role, chunk_index, language, content)
             VALUES ('leaf-1', ?1, 'parent-1', 'leaf', 0, 'en', 'Leaf content')",
            rusqlite::params![doc_id],
        ).unwrap();

        let hits = vec![crate::search::rrf::SearchHit {
            chunk_id: "leaf-1".to_string(),
            content: "Leaf content".to_string(),
            parent_chunk_id: Some("parent-1".to_string()),
            doc_id: doc_id.clone(),
            score: 0.9,
            section_heading: None,
            page_number: None,
        }];

        let l2_chunks = expand_to_l2(&conn, &hits).unwrap();
        assert_eq!(l2_chunks.len(), 1);
        assert_eq!(l2_chunks[0].chunk_id, "parent-1");
        assert_eq!(l2_chunks[0].content, "Parent content here");
    }
}
```

- [ ] **Шаг 2: Реализовать expand.rs**

```rust
use rusqlite::{Connection, params};
use crate::search::rrf::SearchHit;

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

pub fn expand_to_l2(
    conn: &Connection,
    hits: &[SearchHit],
) -> rusqlite::Result<Vec<L2Chunk>> {
    let parent_ids: Vec<String> = hits
        .iter()
        .filter_map(|h| h.parent_chunk_id.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if parent_ids.is_empty() {
        return Ok(vec![]);
    }

    let placeholders = (1..=parent_ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "SELECT c.id, c.content, c.doc_id, c.section_heading, c.page_number,
                d.title AS doc_title, d.source AS doc_source
         FROM chunks c
         JOIN documents d ON d.id = c.doc_id
         WHERE c.id IN ({placeholders})"
    );

    let mut stmt = conn.prepare(&sql)?;
    let params_vec: Vec<&dyn rusqlite::ToSql> = parent_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let rows = stmt.query_map(params_vec.as_slice(), |r| {
        Ok(L2Chunk {
            chunk_id: r.get(0)?,
            content: r.get(1)?,
            doc_id: r.get(2)?,
            section_heading: r.get(3)?,
            page_number: r.get::<_, Option<i64>>(4)?.map(|p| p as u32),
            doc_title: r.get(5)?,
            doc_source: r.get(6)?,
        })
    })?;

    rows.collect()
}

#[cfg(test)]
mod tests { /* из Шага 1 */ }
```

---

### Task 8.3: Context Builder

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/search/context.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::expand::L2Chunk;

    #[test]
    fn build_context_includes_chunks() {
        let builder = ContextBuilder;
        let chunks = vec![L2Chunk {
            chunk_id: "c1".to_string(),
            content: "Rust is great".to_string(),
            doc_id: "d1".to_string(),
            section_heading: Some("Overview".to_string()),
            page_number: Some(1),
            doc_title: Some("Rust Book".to_string()),
            doc_source: Some("/data/rust-book.pdf".to_string()),
        }];
        let ctx = builder.build("What is Rust?", &chunks, &[], "2026-06-04");
        assert!(ctx.prompt.contains("Rust is great"));
        assert!(ctx.prompt.contains("Rust Book"));
        assert!(ctx.prompt.contains("What is Rust?"));
        assert_eq!(ctx.sources.len(), 1);
    }
}
```

- [ ] **Шаг 2: Реализовать context.rs**

```rust
use serde_json::Value;
use crate::search::expand::L2Chunk;
use crate::search::memory_search::MemoryHit;

pub struct QueryContext {
    pub prompt: String,
    pub sources: Vec<Value>,
}

pub struct ContextBuilder;

const SYSTEM_V2: &str = r#"You are a question-answering assistant with access to two types of context:

1. PERSONAL MEMORY FACTS — atomic facts about the user (high signal, always current).
   Use these for questions about the user's life, preferences, location, work, etc.

2. DOCUMENT SOURCES — detailed content from indexed documents.
   Use these for specifics, evidence, quotes, and facts from documents.
   This is your primary source for detailed information.

Today's date: {date}

Instructions:
- For questions about the user, prioritize memory facts over documents.
- For questions about topics/documents, use document sources for details.
- Memory facts are summaries — if a document source contains more detail, use it.
- If neither memory nor documents contain the answer, say "I don't know" explicitly.
- Cite document sources as [1], [2], etc. Cite memory facts as [memory]."#;

impl ContextBuilder {
    pub fn build(
        &self,
        query: &str,
        chunks: &[L2Chunk],
        memory_hits: &[MemoryHit],
        today: &str,
    ) -> QueryContext {
        let system = SYSTEM_V2.replace("{date}", today);
        let mut sources_text = String::new();
        let mut sources_meta = vec![];

        if !memory_hits.is_empty() {
            sources_text.push_str("\nPersonal memory facts:\n");
            for hit in memory_hits.iter().take(5) {
                let mut parts = vec!["memory".to_string()];
                if let Some(cat) = &hit.category { parts.push(cat.clone()); }
                if let Some(proj) = &hit.project { parts.push(proj.clone()); }
                sources_text.push_str(&format!("  [{}] {}\n", parts.join(" | "), hit.content));
            }
        }

        if !chunks.is_empty() {
            sources_text.push_str("\nDocument sources:\n");
            for (i, chunk) in chunks.iter().enumerate() {
                let mut header = format!("[{}]", i + 1);
                if let Some(title) = &chunk.doc_title {
                    header.push(' ');
                    header.push_str(title);
                }
                if let Some(heading) = &chunk.section_heading {
                    header.push_str(&format!(" — {heading}"));
                }
                if let Some(page) = chunk.page_number {
                    header.push_str(&format!(" (p. {page})"));
                }
                sources_text.push_str(&format!("\n{header}\n---\n{}\n", chunk.content));

                let filename = chunk.doc_source.as_deref()
                    .and_then(|s| std::path::Path::new(s).file_name())
                    .and_then(|f| f.to_str())
                    .and_then(|f| f.splitn(6, '-').last())
                    .map(|s| s.to_string());

                sources_meta.push(serde_json::json!({
                    "index": i + 1,
                    "doc_id": chunk.doc_id,
                    "title": chunk.doc_title,
                    "section": chunk.section_heading,
                    "page": chunk.page_number,
                    "preview": &chunk.content[..chunk.content.len().min(200)],
                    "filename": filename,
                }));
            }
        }

        let prompt = format!("{system}\n{sources_text}\nQuestion: {query}");
        QueryContext { prompt, sources: sources_meta }
    }
}

#[cfg(test)]
mod tests { /* из Шага 1 */ }
```

---

### Task 8.4: MemorySearch + RetrievalService

- [ ] **Шаг 1: Реализовать memory_search.rs**

```rust
// rust/src/search/memory_search.rs
use rusqlite::Connection;
use crate::db::repositories::memories::MemoryRepository;
use crate::search::vectors::VectorStore;

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

pub struct MemorySearch {
    pub retrieval_threshold: f32,
    pub top_k: usize,
}

impl MemorySearch {
    pub fn new() -> Self {
        Self {
            retrieval_threshold: 0.70, // distance <= 0.70 → similarity >= 0.30
            top_k: 10,
        }
    }

    pub fn search(
        &self,
        conn: &Connection,
        vectors: &VectorStore,
        query_vector: &[f32],
        category: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryHit>> {
        let hits = vectors.search_memories(conn, query_vector, self.top_k, self.retrieval_threshold)?;
        if hits.is_empty() {
            return Ok(vec![]);
        }

        let repo = MemoryRepository::new(conn);
        let mut results = vec![];

        for hit in hits {
            let Some(mem) = repo.get_by_id(&hit.id)? else { continue };
            if !mem.is_active { continue }
            if let Some(cat) = category {
                if mem.category.as_deref() != Some(cat) { continue }
            }
            results.push(MemoryHit {
                memory_id: mem.id,
                content: mem.content,
                score: 1.0 - hit.distance, // конвертируем distance → similarity
                source: mem.source,
                category: mem.category,
                project: mem.project,
                created_at: mem.created_at,
            });
        }

        Ok(results)
    }
}
```

- [ ] **Шаг 2: Реализовать service.rs**

```rust
// rust/src/search/service.rs
use std::sync::Arc;
use rusqlite::Connection;

use crate::ingestion::embeddings::EmbeddingClient;
use crate::llm::LlmProvider;
use crate::search::{
    context::ContextBuilder,
    expand::expand_to_l2,
    memory_search::MemorySearch,
    reranker::Reranker,
    rrf::rrf_merge,
    tantivy_fts::TantivyStore,
    vectors::VectorStore,
};
use crate::ingestion::language::LanguageDetector;

pub struct QueryResult {
    pub answer: String,
    pub sources: Vec<serde_json::Value>,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub struct RetrievalService {
    pub embed: Arc<EmbeddingClient>,
    pub tantivy: Arc<TantivyStore>,
    pub vectors: Arc<VectorStore>,
    pub reranker: Arc<Reranker>,
    pub llm: Arc<dyn LlmProvider>,
    pub context_builder: ContextBuilder,
    pub memory_search: MemorySearch,
    pub lang: LanguageDetector,
    pub semantic_top_k: usize,
    pub bm25_top_k: usize,
    pub rrf_k: usize,
    pub reranker_top_n: usize,
}

impl RetrievalService {
    pub fn query(
        &self,
        conn: &Connection,
        query: &str,
        memory_category: Option<&str>,
    ) -> anyhow::Result<QueryResult> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        // 1. Embed запроса
        let query_vector = self.embed.embed_query(query)?;

        // 2. Семантический поиск
        let semantic_hits_raw = self.vectors.search_chunks(conn, &query_vector, self.semantic_top_k)?;
        let semantic_hits = raw_hits_to_search_hits(conn, semantic_hits_raw)?;

        // 3. BM25 поиск
        let lang = self.lang.detect(query);
        let bm25_raw = self.tantivy.search(query, &lang, self.bm25_top_k)?;
        let bm25_hits = fts_hits_to_search_hits(conn, bm25_raw)?;

        // 4. RRF слияние
        let merged = rrf_merge(&semantic_hits, &bm25_hits, self.rrf_k, 20);

        // 5. L2 expand (leaf → parent)
        let l2_chunks = expand_to_l2(conn, &merged)?;

        // 6. Reranking
        let texts: Vec<&str> = l2_chunks.iter().map(|c| c.content.as_str()).collect();
        let rerank_results = self.reranker.rerank(query, &texts, self.reranker_top_n)?;
        let reranked: Vec<_> = rerank_results
            .iter()
            .filter_map(|r| l2_chunks.get(r.original_index))
            .cloned()
            .collect();

        // 7. Memory поиск
        let mem_hits = self.memory_search.search(conn, &self.vectors, &query_vector, memory_category)?;

        // 8. Построить контекст
        let ctx = self.context_builder.build(query, &reranked, &mem_hits, &today);

        // 9. LLM
        let llm_response = tokio::runtime::Handle::current()
            .block_on(self.llm.complete(&ctx.prompt))?;

        Ok(QueryResult {
            answer: llm_response.answer,
            sources: ctx.sources,
            input_tokens: llm_response.input_tokens,
            output_tokens: llm_response.output_tokens,
        })
    }
}

fn raw_hits_to_search_hits(
    conn: &Connection,
    hits: Vec<crate::search::vectors::VectorHit>,
) -> rusqlite::Result<Vec<crate::search::rrf::SearchHit>> {
    use crate::search::rrf::SearchHit;
    let mut result = vec![];
    for h in hits {
        let mut stmt = conn.prepare_cached(
            "SELECT id, content, parent_chunk_id, doc_id, section_heading, page_number
             FROM chunks WHERE id = ?1",
        )?;
        if let Ok(row) = stmt.query_row(rusqlite::params![h.id], |r| {
            Ok(SearchHit {
                chunk_id: r.get(0)?,
                content: r.get(1)?,
                parent_chunk_id: r.get(2)?,
                doc_id: r.get(3)?,
                score: 1.0 - h.distance,
                section_heading: r.get(4)?,
                page_number: r.get::<_, Option<i64>>(5)?.map(|p| p as u32),
            })
        }) {
            result.push(row);
        }
    }
    Ok(result)
}

fn fts_hits_to_search_hits(
    conn: &Connection,
    hits: Vec<crate::search::tantivy_fts::FtsHit>,
) -> rusqlite::Result<Vec<crate::search::rrf::SearchHit>> {
    use crate::search::rrf::SearchHit;
    let mut result = vec![];
    for h in hits {
        let mut stmt = conn.prepare_cached(
            "SELECT id, content, parent_chunk_id, doc_id, section_heading, page_number
             FROM chunks WHERE id = ?1",
        )?;
        if let Ok(row) = stmt.query_row(rusqlite::params![h.chunk_id], |r| {
            Ok(SearchHit {
                chunk_id: r.get(0)?,
                content: r.get(1)?,
                parent_chunk_id: r.get(2)?,
                doc_id: r.get(3)?,
                score: h.score,
                section_heading: r.get(4)?,
                page_number: r.get::<_, Option<i64>>(5)?.map(|p| p as u32),
            })
        }) {
            result.push(row);
        }
    }
    Ok(result)
}
```

- [ ] **Шаг 3: Обновить search/mod.rs**

```rust
pub mod context;
pub mod expand;
pub mod memory_search;
pub mod reranker;
pub mod rrf;
pub mod service;
pub mod tantivy_fts;
pub mod vectors;

pub use tantivy_fts::TantivyStore;
pub use vectors::VectorStore;
```

- [ ] **Шаг 4: Запустить тесты**

```bash
cd rust && cargo test rrf expand context 2>&1
```

Ожидаем: тесты rrf (2), expand (1), context (1) — все зелёные.

- [ ] **Шаг 5: Коммит**

```bash
git add rust/src/search/
git commit -m "feat(rust): retrieval pipeline — RRF, L2 expand, context builder, memory search, service"
```
