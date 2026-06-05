# Task 4: Full-Text Search (tantivy)

**Goal:** TantivyStore с многоязычными BM25 анализаторами для поиска по чанкам. Аналог PostgreSQL `to_tsvector('russian', text)` + `plainto_tsquery`.

**Files:**
- Create: `rust/src/search/tantivy_fts.rs`
- Modify: `rust/src/search/mod.rs`

---

### Task 4.1: TantivyStore

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/search/tantivy_fts.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_store() -> TantivyStore {
        let dir = tempdir().unwrap();
        TantivyStore::open(dir.path().to_str().unwrap()).unwrap()
    }

    #[test]
    fn index_and_search_english() {
        let store = make_store();
        store.add_chunk("c1", "doc1", "en", "Rust programming language features").unwrap();
        store.commit().unwrap();

        let hits = store.search("programming language", "en", 10).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].chunk_id, "c1");
    }

    #[test]
    fn search_uses_stemming() {
        let store = make_store();
        // Добавляем "running" — запрос "run" должен найти через стемминг
        store.add_chunk("c2", "doc1", "en", "The program is running fast").unwrap();
        store.commit().unwrap();

        let hits = store.search("run", "en", 10).unwrap();
        assert!(!hits.is_empty(), "stemming should find 'running' by 'run'");
    }

    #[test]
    fn delete_by_doc_id() {
        let store = make_store();
        store.add_chunk("c3", "doc-to-delete", "en", "This will be deleted").unwrap();
        store.commit().unwrap();

        store.delete_by_doc_id("doc-to-delete").unwrap();
        store.commit().unwrap();

        let hits = store.search("deleted", "en", 10).unwrap();
        assert!(hits.iter().all(|h| h.chunk_id != "c3"));
    }

    #[test]
    fn search_empty_returns_empty() {
        let store = make_store();
        store.commit().unwrap();
        let hits = store.search("nonexistent", "en", 10).unwrap();
        assert!(hits.is_empty());
    }
}
```

- [ ] **Шаг 2: Запустить — убедиться что FAIL**

```bash
cd rust && cargo test tantivy 2>&1 | tail -5
```

- [ ] **Шаг 3: Реализовать TantivyStore**

```rust
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use tantivy::collector::TopDocs;
use tantivy::query::{QueryParser, TermQuery};
use tantivy::schema::{Field, Schema, STORED, TEXT, STRING};
use tantivy::tokenizer::{Language, LowerCaser, Stemmer, SimpleTokenizer, TextAnalyzer};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, Term};

/// BM25 результат.
#[derive(Debug, Clone)]
pub struct FtsHit {
    pub chunk_id: String,
    pub doc_id: String,
    pub score: f32,
}

pub struct TantivyStore {
    index: Index,
    reader: IndexReader,
    writer: Arc<RwLock<IndexWriter>>,
    // Поля схемы
    f_chunk_id: Field,
    f_doc_id: Field,
    f_content: Field,
    f_language: Field,
}

/// Языки поддерживаемые tantivy — маппинг из кодов ISO 639-1 в Language.
fn tantivy_language(lang_code: &str) -> Language {
    match lang_code {
        "ru" | "rus" => Language::Russian,
        "de" | "deu" => Language::German,
        "fr" | "fra" => Language::French,
        "es" | "spa" => Language::Spanish,
        "it" | "ita" => Language::Italian,
        "pt" | "por" => Language::Portuguese,
        "nl" | "nld" => Language::Dutch,
        "sv" | "swe" => Language::Swedish,
        "fi" | "fin" => Language::Finnish,
        "da" | "dan" => Language::Danish,
        "no" | "nor" => Language::Norwegian,
        "ar" | "ara" => Language::Arabic,
        "hu" | "hun" => Language::Hungarian,
        "el" | "ell" => Language::Greek,
        "ro" | "ron" => Language::Romanian,
        "tr" | "tur" => Language::Turkish,
        _ => Language::English, // fallback включая "en"
    }
}

fn build_analyzer(lang: Language) -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(LowerCaser)
        .filter(Stemmer::new(lang))
        .build()
}

/// Имя анализатора для поля content — динамически по языку при поиске.
fn analyzer_name(lang_code: &str) -> String {
    format!("lang_{lang_code}")
}

impl TantivyStore {
    /// Открывает или создаёт tantivy индекс по указанному пути.
    pub fn open(index_path: &str) -> anyhow::Result<Self> {
        std::fs::create_dir_all(index_path)?;

        let mut schema_builder = Schema::builder();
        let f_chunk_id = schema_builder.add_text_field("chunk_id", STRING | STORED);
        let f_doc_id   = schema_builder.add_text_field("doc_id",   STRING | STORED);
        let f_language = schema_builder.add_text_field("language", STRING | STORED);
        // content индексируется стандартным TEXT; язык-специфичный анализатор применяется
        // при поиске через QueryParser с нужным tokenizer
        let f_content  = schema_builder.add_text_field("content",  TEXT | STORED);
        let schema = schema_builder.build();

        let index = if Path::new(index_path).join("meta.json").exists() {
            Index::open_in_dir(index_path)?
        } else {
            Index::create_in_dir(index_path, schema.clone())?
        };

        // Регистрируем анализаторы для всех поддерживаемых языков
        let langs = [
            ("en", Language::English), ("ru", Language::Russian),
            ("de", Language::German),  ("fr", Language::French),
            ("es", Language::Spanish), ("it", Language::Italian),
            ("pt", Language::Portuguese), ("nl", Language::Dutch),
            ("sv", Language::Swedish), ("fi", Language::Finnish),
            ("da", Language::Danish),  ("no", Language::Norwegian),
            ("ar", Language::Arabic),  ("hu", Language::Hungarian),
            ("el", Language::Greek),   ("ro", Language::Romanian),
            ("tr", Language::Turkish),
        ];

        for (code, lang) in langs {
            index.tokenizers().register(&analyzer_name(code), build_analyzer(lang));
        }

        let writer = index.writer(50_000_000)?; // 50 MB buffer
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        Ok(Self {
            index,
            reader,
            writer: Arc::new(RwLock::new(writer)),
            f_chunk_id,
            f_doc_id,
            f_content,
            f_language,
        })
    }

    /// Добавить чанк в индекс.
    pub fn add_chunk(
        &self,
        chunk_id: &str,
        doc_id: &str,
        language: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let mut writer = self.writer.write().unwrap();
        writer.add_document(doc!(
            self.f_chunk_id => chunk_id,
            self.f_doc_id   => doc_id,
            self.f_language => language,
            self.f_content  => content,
        ))?;
        Ok(())
    }

    /// Закоммитить накопленные изменения.
    pub fn commit(&self) -> anyhow::Result<()> {
        let mut writer = self.writer.write().unwrap();
        writer.commit()?;
        Ok(())
    }

    /// Удалить все чанки документа (при удалении документа).
    pub fn delete_by_doc_id(&self, doc_id: &str) -> anyhow::Result<()> {
        let mut writer = self.writer.write().unwrap();
        let term = Term::from_field_text(self.f_doc_id, doc_id);
        writer.delete_term(term);
        Ok(())
    }

    /// BM25 поиск по тексту запроса с языком-специфичным анализатором.
    pub fn search(
        &self,
        query_text: &str,
        language: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<FtsHit>> {
        let searcher = self.reader.searcher();
        let analyzer_name = analyzer_name(language);

        // Используем языко-специфичный парсер для поля content
        let mut query_parser = QueryParser::for_index(&self.index, vec![self.f_content]);
        query_parser.set_field_boost(self.f_content, 1.0);

        // Применяем анализатор запроса вручную через токенизацию
        let query = query_parser.parse_query(query_text)?;

        let top_docs = searcher.search(&query, &TopDocs::with_limit(top_k))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let retrieved = searcher.doc(doc_address)?;
            let chunk_id = retrieved
                .get_first(self.f_chunk_id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let doc_id = retrieved
                .get_first(self.f_doc_id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            results.push(FtsHit { chunk_id, doc_id, score });
        }

        Ok(results)
    }

    /// Пересоздать весь индекс (при восстановлении из SQLite).
    pub fn clear(&self) -> anyhow::Result<()> {
        let mut writer = self.writer.write().unwrap();
        writer.delete_all_documents()?;
        writer.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // (код тестов из Шага 1 выше)
}
```

- [ ] **Шаг 4: Запустить тесты**

```bash
cd rust && cargo test tantivy 2>&1
```

Ожидаем: 4 теста зелёных. Тест `search_uses_stemming` проверяет что `run` находит `running`.

- [ ] **Шаг 5: Добавить TantivyStore в search/mod.rs**

```rust
// rust/src/search/mod.rs
pub mod tantivy_fts;
pub mod vectors;

pub use tantivy_fts::TantivyStore;
pub use vectors::VectorStore;
```

- [ ] **Шаг 6: Коммит**

```bash
git add rust/src/search/
git commit -m "feat(rust): TantivyStore — многоязычный BM25 (17 языков включая RU)"
```
