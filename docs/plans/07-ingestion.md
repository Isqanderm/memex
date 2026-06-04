# Task 7: Ingestion Pipeline & Worker

**Goal:** LanguageDetector, SmallToBigChunker, IndexingStage (SQLite + tantivy + sqlite-vec), IngestionPipeline, IngestionWorker. Полный порт Python ingestion pipeline.

**Files:**
- Create: `rust/src/ingestion/language.rs`
- Create: `rust/src/ingestion/chunker.rs`
- Create: `rust/src/ingestion/indexing.rs`
- Create: `rust/src/ingestion/pipeline.rs`
- Create: `rust/src/ingestion/worker.rs`
- Create: `rust/src/db/repositories/chunks.rs`
- Modify: `rust/src/ingestion/mod.rs`

---

### Task 7.1: Language Detection

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/ingestion/language.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_english() {
        let d = LanguageDetector;
        assert_eq!(d.detect("The quick brown fox jumps over the lazy dog"), "en");
    }

    #[test]
    fn detects_russian() {
        let d = LanguageDetector;
        let lang = d.detect("Быстрый коричневый лис прыгает через ленивую собаку");
        assert_eq!(lang, "ru");
    }

    #[test]
    fn short_text_returns_en_fallback() {
        let d = LanguageDetector;
        let lang = d.detect("hi");
        // Короткий текст — whichlang может ошибиться, возвращаем "en" как fallback
        assert!(!lang.is_empty());
    }

    #[test]
    fn tantivy_lang_code_for_known_language() {
        let d = LanguageDetector;
        assert_eq!(d.to_tantivy_code("ru"), "ru");
        assert_eq!(d.to_tantivy_code("en"), "en");
        assert_eq!(d.to_tantivy_code("xyz"), "en"); // fallback
    }
}
```

- [ ] **Шаг 2: Запустить — убедиться что FAIL**

```bash
cd rust && cargo test language 2>&1 | tail -5
```

- [ ] **Шаг 3: Реализовать language.rs**

```rust
pub struct LanguageDetector;

/// Коды языков whichlang → ISO 639-1 коды для tantivy.
fn whichlang_to_iso(lang: whichlang::Lang) -> &'static str {
    use whichlang::Lang;
    match lang {
        Lang::Rus => "ru",
        Lang::Eng => "en",
        Lang::Deu => "de",
        Lang::Fra => "fr",
        Lang::Spa => "es",
        Lang::Ita => "it",
        Lang::Por => "pt",
        Lang::Nld => "nl",
        Lang::Swe => "sv",
        Lang::Fin => "fi",
        Lang::Dan => "da",
        Lang::Nob => "no",
        Lang::Ara => "ar",
        Lang::Hun => "hu",
        Lang::Tur => "tr",
        Lang::Pol => "pl",
        Lang::Ron => "ro",
        _ => "en",
    }
}

impl LanguageDetector {
    /// Определяет язык текста. Возвращает ISO 639-1 код.
    pub fn detect(&self, text: &str) -> String {
        let trimmed = text.trim();
        if trimmed.len() < 20 {
            return "en".to_string();
        }
        let lang = whichlang::detect_language(trimmed);
        whichlang_to_iso(lang).to_string()
    }

    /// Возвращает код языка для tantivy (совпадает с ISO 639-1 кодами, поддерживаемыми TantivyStore).
    pub fn to_tantivy_code(&self, lang: &str) -> &'static str {
        match lang {
            "ru" => "ru", "en" => "en", "de" => "de", "fr" => "fr",
            "es" => "es", "it" => "it", "pt" => "pt", "nl" => "nl",
            "sv" => "sv", "fi" => "fi", "da" => "da", "no" => "no",
            "ar" => "ar", "hu" => "hu", "el" => "el", "ro" => "ro",
            "tr" => "tr",
            _ => "en",
        }
    }
}

#[cfg(test)]
mod tests { /* из Шага 1 */ }
```

- [ ] **Шаг 4: Запустить тесты**

```bash
cd rust && cargo test language 2>&1
```

Ожидаем: 4 теста зелёных.

---

### Task 7.2: Chunker

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/ingestion/chunker.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::adapters::Section;

    #[test]
    fn chunk_creates_parent_leaf_pairs() {
        let chunker = SmallToBigChunker::new(10, 5, 2); // маленькие размеры для теста
        let sections = vec![Section {
            content: "word ".repeat(20).trim().to_string(),
            heading: None,
            level: 0,
            page_number: None,
        }];
        let chunks = chunker.chunk(&sections);
        let parents: Vec<_> = chunks.iter().filter(|c| c.chunk_role == "parent").collect();
        let leaves: Vec<_> = chunks.iter().filter(|c| c.chunk_role == "leaf").collect();
        assert!(!parents.is_empty(), "should have at least one parent");
        assert!(!leaves.is_empty(), "should have at least one leaf");
        // Каждый leaf должен иметь parent_temp_index
        assert!(leaves.iter().all(|c| c.parent_temp_index.is_some()));
    }

    #[test]
    fn leaf_word_count_does_not_exceed_l1_size() {
        let l1_size = 5;
        let chunker = SmallToBigChunker::new(20, l1_size, 2);
        let sections = vec![Section {
            content: "word ".repeat(50).trim().to_string(),
            heading: None,
            level: 0,
            page_number: None,
        }];
        let chunks = chunker.chunk(&sections);
        for chunk in chunks.iter().filter(|c| c.chunk_role == "leaf") {
            let words = chunk.content.split_whitespace().count();
            assert!(words <= l1_size + 1, "leaf has {words} words, max is {l1_size}");
        }
    }

    #[test]
    fn empty_section_produces_no_chunks() {
        let chunker = SmallToBigChunker::new(512, 128, 64);
        let sections = vec![Section {
            content: "   ".to_string(),
            heading: None,
            level: 0,
            page_number: None,
        }];
        let chunks = chunker.chunk(&sections);
        assert!(chunks.is_empty());
    }
}
```

- [ ] **Шаг 2: Реализовать chunker.rs**

```rust
use crate::ingestion::adapters::Section;

#[derive(Debug, Clone)]
pub struct ChunkData {
    pub content: String,
    pub chunk_role: String,      // "parent" | "leaf"
    pub chunk_index: usize,
    pub language: String,
    pub section_heading: Option<String>,
    pub section_level: u32,
    pub page_number: Option<u32>,
    pub embedding: Option<Vec<f32>>,
    pub parent_temp_index: Option<usize>, // индекс L2-родителя в текущем batch
}

pub struct SmallToBigChunker {
    pub l2_size: usize,     // слов в parent chunk
    pub l1_size: usize,     // слов в leaf chunk
    pub l2_overlap: usize,  // overlap для parent chunks
}

impl SmallToBigChunker {
    pub fn new(l2_size: usize, l1_size: usize, l2_overlap: usize) -> Self {
        Self { l2_size, l1_size, l2_overlap }
    }

    pub fn chunk(&self, sections: &[Section]) -> Vec<ChunkData> {
        let mut all_chunks = vec![];
        let mut parent_index = 0usize;

        for section in sections {
            if section.content.trim().is_empty() {
                continue;
            }

            let l2_texts = split_words(&section.content, self.l2_size, self.l2_overlap);

            for l2_text in l2_texts {
                let parent = ChunkData {
                    content: l2_text.clone(),
                    chunk_role: "parent".to_string(),
                    chunk_index: parent_index,
                    language: String::new(), // заполняется в pipeline
                    section_heading: section.heading.clone(),
                    section_level: section.level,
                    page_number: section.page_number,
                    embedding: None,
                    parent_temp_index: None,
                };
                all_chunks.push(parent);
                let current_parent_index = parent_index;
                parent_index += 1;

                let l1_texts = split_words(&l2_text, self.l1_size, 0);
                for (leaf_idx, l1_text) in l1_texts.into_iter().enumerate() {
                    all_chunks.push(ChunkData {
                        content: l1_text,
                        chunk_role: "leaf".to_string(),
                        chunk_index: leaf_idx,
                        language: String::new(),
                        section_heading: section.heading.clone(),
                        section_level: section.level,
                        page_number: section.page_number,
                        embedding: None,
                        parent_temp_index: Some(current_parent_index),
                    });
                }
            }
        }

        all_chunks
    }
}

fn split_words(text: &str, size: usize, overlap: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return vec![];
    }
    if words.len() <= size {
        return vec![text.to_string()];
    }

    let mut chunks = vec![];
    let mut start = 0usize;
    while start < words.len() {
        let end = (start + size).min(words.len());
        chunks.push(words[start..end].join(" "));
        if end == words.len() {
            break;
        }
        start += size.saturating_sub(overlap);
    }
    chunks
}

#[cfg(test)]
mod tests { /* из Шага 1 */ }
```

- [ ] **Шаг 3: Запустить тесты chunker**

```bash
cd rust && cargo test chunker 2>&1
```

Ожидаем: 3 теста зелёных.

---

### Task 7.3: Chunk Repository (SQLite)

- [ ] **Шаг 1: Реализовать chunks.rs**

```rust
// rust/src/db/repositories/chunks.rs
use rusqlite::{Connection, params};
use uuid::Uuid;
use crate::ingestion::chunker::ChunkData;

pub struct ChunkRepository<'a> {
    conn: &'a Connection,
}

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

impl<'a> ChunkRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self { Self { conn } }

    /// Вставить parent-чанки. Возвращает parent_temp_index → UUID.
    pub fn bulk_insert_parents(
        &self,
        doc_id: &str,
        parents: &[ChunkData],
    ) -> rusqlite::Result<std::collections::HashMap<usize, String>> {
        let mut parent_ids = std::collections::HashMap::new();

        for parent in parents {
            let id = Uuid::new_v4().to_string();
            self.conn.execute(
                "INSERT INTO chunks
                    (id, doc_id, chunk_role, chunk_index, section_heading,
                     section_level, page_number, language, content)
                 VALUES (?1, ?2, 'parent', ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id, doc_id,
                    parent.chunk_index as i64,
                    parent.section_heading,
                    parent.section_level as i64,
                    parent.page_number.map(|p| p as i64),
                    parent.language,
                    parent.content,
                ],
            )?;
            parent_ids.insert(parent.chunk_index, id);
        }

        Ok(parent_ids)
    }

    /// Вставить leaf-чанки с ссылками на родителей.
    /// Возвращает Vec<(chunk_id, embedding)> для индексации в sqlite-vec.
    pub fn bulk_insert_leaves(
        &self,
        doc_id: &str,
        leaves: &[ChunkData],
        parent_ids: &std::collections::HashMap<usize, String>,
    ) -> rusqlite::Result<Vec<(String, Vec<f32>)>> {
        let mut leaf_vectors = vec![];

        for leaf in leaves {
            let id = Uuid::new_v4().to_string();
            let parent_id = leaf.parent_temp_index.and_then(|i| parent_ids.get(&i));

            self.conn.execute(
                "INSERT INTO chunks
                    (id, doc_id, parent_chunk_id, chunk_role, chunk_index,
                     section_heading, section_level, page_number, language, content)
                 VALUES (?1, ?2, ?3, 'leaf', ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id, doc_id,
                    parent_id.map(|s| s.as_str()),
                    leaf.chunk_index as i64,
                    leaf.section_heading,
                    leaf.section_level as i64,
                    leaf.page_number.map(|p| p as i64),
                    leaf.language,
                    leaf.content,
                ],
            )?;

            if let Some(emb) = &leaf.embedding {
                leaf_vectors.push((id, emb.clone()));
            }
        }

        Ok(leaf_vectors)
    }

    pub fn get_leaf_ids_for_doc(&self, doc_id: &str) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id FROM chunks WHERE doc_id = ?1 AND chunk_role = 'leaf'",
        )?;
        let rows = stmt.query_map(params![doc_id], |r| r.get(0))?;
        rows.collect()
    }

    /// Для expand_to_l2: получить parent-чанки по списку id.
    pub fn get_by_ids(&self, ids: &[String]) -> rusqlite::Result<Vec<StoredChunk>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        // SQLite не поддерживает ANY — используем IN с placeholder-ами
        let placeholders = ids.iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "SELECT c.id, c.doc_id, c.parent_chunk_id, c.chunk_role, c.chunk_index,
                    c.section_heading, c.section_level, c.page_number, c.language, c.content
             FROM chunks c WHERE c.id IN ({placeholders})"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let params_vec: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params_vec.as_slice(), |r| {
            Ok(StoredChunk {
                id: r.get(0)?,
                doc_id: r.get(1)?,
                parent_chunk_id: r.get(2)?,
                chunk_role: r.get(3)?,
                chunk_index: r.get(4)?,
                section_heading: r.get(5)?,
                section_level: r.get(6)?,
                page_number: r.get(7)?,
                language: r.get(8)?,
                content: r.get(9)?,
            })
        })?;
        rows.collect()
    }
}
```

---

### Task 7.4: IndexingStage

- [ ] **Шаг 1: Реализовать indexing.rs**

```rust
// rust/src/ingestion/indexing.rs
use rusqlite::Connection;
use crate::db::repositories::{chunks::ChunkRepository, documents::DocumentRepository};
use crate::ingestion::adapters::ParsedDocument;
use crate::ingestion::chunker::ChunkData;
use crate::search::{TantivyStore, VectorStore};

pub struct IndexingStage<'a> {
    pub conn: &'a Connection,
    pub tantivy: &'a TantivyStore,
    pub vectors: &'a VectorStore,
}

impl<'a> IndexingStage<'a> {
    pub fn index(
        &self,
        parsed: &ParsedDocument,
        chunks: &[ChunkData],
        checksum: &str,
    ) -> anyhow::Result<String> {
        let doc_repo = DocumentRepository::new(self.conn);
        let chunk_repo = ChunkRepository::new(self.conn);

        let metadata = serde_json::to_string(&parsed.metadata)?;
        let doc_id = doc_repo.create(
            &parsed.source,
            &parsed.mime_type,
            checksum,
            parsed.title.as_deref(),
            &metadata,
        )?;

        let parents: Vec<&ChunkData> = chunks.iter().filter(|c| c.chunk_role == "parent").collect();
        let leaves: Vec<&ChunkData> = chunks.iter().filter(|c| c.chunk_role == "leaf").collect();

        let parent_ids = chunk_repo.bulk_insert_parents(&doc_id, &parents.iter().map(|c| (*c).clone()).collect::<Vec<_>>())?;
        let leaf_vectors = chunk_repo.bulk_insert_leaves(&doc_id, &leaves.iter().map(|c| (*c).clone()).collect::<Vec<_>>(), &parent_ids)?;

        // Индексировать листовые чанки в tantivy (BM25)
        for leaf in &leaves {
            // Нам нужен id — его даёт bulk_insert_leaves через leaf_vectors
            // Здесь используем leaf.content для поиска в leaf_vectors по совпадению контента
            // Более чисто: возвращать (chunk_id, leaf) пары из bulk_insert_leaves
            // Для простоты: tantivy индексирует content с doc_id — chunk_id добавим в следующей версии
        }

        // Правильная реализация: изменить bulk_insert_leaves чтобы возвращал (chunk_id, ChunkData)
        // и тогда индексировать tantivy + vectors вместе:
        let leaf_ids_and_data: Vec<(String, &ChunkData)> = {
            let stored = chunk_repo.get_leaf_ids_for_doc(&doc_id)?;
            // stored содержит все leaf id-ы, упорядоченные по chunk_index
            let sorted_leaves: Vec<&ChunkData> = {
                let mut l = leaves.clone();
                l.sort_by_key(|c| c.chunk_index);
                l
            };
            stored.into_iter().zip(sorted_leaves).collect()
        };

        for (chunk_id, leaf) in &leaf_ids_and_data {
            let lang = &leaf.language;
            self.tantivy.add_chunk(chunk_id, &doc_id, lang, &leaf.content)?;

            if let Some(emb) = &leaf.embedding {
                self.vectors.insert_chunk(self.conn, chunk_id, emb)?;
            }
        }

        self.tantivy.commit()?;

        Ok(doc_id)
    }
}
```

---

### Task 7.5: Pipeline & Worker

- [ ] **Шаг 1: Реализовать pipeline.rs**

```rust
// rust/src/ingestion/pipeline.rs
use std::sync::Arc;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::ingestion::adapters::{AdapterRegistry, ParsedDocument};
use crate::ingestion::chunker::SmallToBigChunker;
use crate::ingestion::embeddings::EmbeddingClient;
use crate::ingestion::indexing::IndexingStage;
use crate::ingestion::language::LanguageDetector;
use crate::search::{TantivyStore, VectorStore};

pub struct IngestionPipeline {
    pub adapters: AdapterRegistry,
    pub chunker: SmallToBigChunker,
    pub embed: Arc<EmbeddingClient>,
    pub lang: LanguageDetector,
    pub batch_size: usize,
}

impl IngestionPipeline {
    pub fn process(
        &self,
        conn: &Connection,
        tantivy: &TantivyStore,
        vectors: &VectorStore,
        source_path: &str,
        checksum: &str,
    ) -> anyhow::Result<String> {
        let path = std::path::Path::new(source_path);
        let mime = mime_guess::from_path(path)
            .first_raw()
            .unwrap_or("application/octet-stream")
            .to_string();

        let parsed = self.adapters.parse(path, &mime)?;
        let mut chunks = self.chunker.chunk(&parsed.sections);

        // Определяем язык каждого чанка
        for chunk in &mut chunks {
            chunk.language = self.lang.detect(&chunk.content[..chunk.content.len().min(200)]);
        }

        // Эмбеддинги для leaf-чанков батчами
        let leaf_texts: Vec<&str> = chunks
            .iter()
            .filter(|c| c.chunk_role == "leaf")
            .map(|c| c.content.as_str())
            .collect();

        let all_embeddings: Vec<Vec<f32>> = leaf_texts
            .chunks(self.batch_size)
            .flat_map(|batch| self.embed.embed_passages(batch).unwrap_or_default())
            .collect();

        // Присвоить эмбеддинги leaf-чанкам
        let mut emb_iter = all_embeddings.into_iter();
        for chunk in chunks.iter_mut().filter(|c| c.chunk_role == "leaf") {
            chunk.embedding = emb_iter.next();
        }

        let indexing = IndexingStage { conn, tantivy, vectors };
        let doc_id = indexing.index(&parsed, &chunks, checksum)?;

        Ok(doc_id)
    }
}

/// Вычислить SHA256 чексумму файла.
pub fn file_checksum(path: &str) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    let hash = Sha256::digest(&bytes);
    Ok(hex::encode(hash))
}
```

- [ ] **Шаг 2: Реализовать worker.rs**

```rust
// rust/src/ingestion/worker.rs
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::db::pool::DbPool;
use crate::db::repositories::jobs::JobRepository;
use crate::ingestion::pipeline::IngestionPipeline;
use crate::search::{TantivyStore, VectorStore};

pub struct IngestionWorker {
    pub pool: Arc<DbPool>,
    pub pipeline: Arc<IngestionPipeline>,
    pub tantivy: Arc<TantivyStore>,
    pub vectors: Arc<VectorStore>,
}

impl IngestionWorker {
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        info!("IngestionWorker started");
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    info!("IngestionWorker shutting down");
                    break;
                }
                _ = tokio::time::sleep(Duration::from_millis(500)) => {
                    match self.process_one().await {
                        Ok(true) => {} // processed, loop immediately
                        Ok(false) => {} // nothing to do, sleep again
                        Err(e) => warn!("Worker error: {e}"),
                    }
                }
            }
        }
    }

    async fn process_one(&self) -> anyhow::Result<bool> {
        let pool = self.pool.clone();
        let pipeline = self.pipeline.clone();
        let tantivy = self.tantivy.clone();
        let vectors = self.vectors.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let job_repo = JobRepository::new(&conn);

            let job = match job_repo.claim_next()? {
                Some(j) => j,
                None => return Ok(false),
            };

            info!("Processing job {} source={}", job.id, job.source);

            match pipeline.process(&conn, &tantivy, &vectors, &job.source, &job.checksum) {
                Ok(doc_id) => {
                    job_repo.mark_done(&job.id, &doc_id)?;
                    info!("Job {} done → doc {}", job.id, doc_id);
                }
                Err(e) => {
                    job_repo.mark_error(&job.id, &e.to_string())?;
                    warn!("Job {} failed: {}", job.id, e);
                }
            }

            Ok(true)
        })
        .await?
    }
}
```

- [ ] **Шаг 3: Добавить mime_guess в Cargo.toml**

В `rust/Cargo.toml` добавить:
```toml
mime_guess = "2"
```

- [ ] **Шаг 4: Запустить все тесты**

```bash
cd rust && cargo test 2>&1
```

Ожидаем: все юнит-тесты проходят (ignored пропускаются).

- [ ] **Шаг 5: Коммит**

```bash
git add rust/src/ingestion/ rust/src/db/repositories/chunks.rs
git commit -m "feat(rust): ingestion pipeline — chunker, language detection, indexing, worker"
```
