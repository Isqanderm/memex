use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::Context;
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING};
use tantivy::tokenizer::{Language, LowerCaser, SimpleTokenizer, Stemmer, TextAnalyzer, TokenizerManager};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

/// A search hit from the full-text index.
#[derive(Debug, Clone)]
pub struct FtsHit {
    pub chunk_id: String,
    pub doc_id: String,
    pub score: f32,
}

/// Build a stemming analyzer for the given language.
fn build_analyzer(lang: Language) -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(LowerCaser)
        .filter(Stemmer::new(lang))
        .build()
}

/// Canonical code for each supported language (used as field-name suffix and tokenizer name).
const LANGUAGES: &[(&str, Language)] = &[
    ("en", Language::English),
    ("ru", Language::Russian),
    ("de", Language::German),
    ("fr", Language::French),
    ("es", Language::Spanish),
    ("it", Language::Italian),
    ("pt", Language::Portuguese),
    ("nl", Language::Dutch),
    ("sv", Language::Swedish),
    ("fi", Language::Finnish),
    ("da", Language::Danish),
    ("no", Language::Norwegian),
    ("ar", Language::Arabic),
    ("hu", Language::Hungarian),
    ("el", Language::Greek),
    ("ro", Language::Romanian),
    ("tr", Language::Turkish),
];

/// Map a language code string (e.g. "en", "ru") to a canonical 2-letter code.
fn canonical_lang_code(code: &str) -> &'static str {
    match code.to_lowercase().as_str() {
        "ru" | "russian" => "ru",
        "de" | "german" => "de",
        "fr" | "french" => "fr",
        "es" | "spanish" => "es",
        "it" | "italian" => "it",
        "pt" | "portuguese" => "pt",
        "nl" | "dutch" => "nl",
        "sv" | "swedish" => "sv",
        "fi" | "finnish" => "fi",
        "da" | "danish" => "da",
        "no" | "norwegian" => "no",
        "ar" | "arabic" => "ar",
        "hu" | "hungarian" => "hu",
        "el" | "greek" => "el",
        "ro" | "romanian" => "ro",
        "tr" | "turkish" => "tr",
        _ => "en", // default
    }
}

/// Per-language tokenizer name used in the schema.
fn lang_tokenizer_name(code: &str) -> String {
    format!("content_{code}")
}

/// Register all language analyzers on the index's tokenizer manager.
///
/// Called once in `open()` and never again — this makes the tokenizer
/// registry immutable after initialization, which is required for
/// thread safety when multiple threads index and search concurrently.
fn register_language_analyzers(index: &Index) {
    for (code, lang) in LANGUAGES {
        index
            .tokenizers()
            .register(&lang_tokenizer_name(code), build_analyzer(*lang));
    }
}

/// Build a fresh, standalone `TokenizerManager` containing all language
/// analyzers. Used to construct per-search `QueryParser` instances without
/// touching the shared index tokenizer registry.
fn build_local_tokenizer_manager() -> TokenizerManager {
    // Start with a completely new manager (not cloned from the index),
    // so mutations here do NOT affect the shared registry.
    let manager = TokenizerManager::new();
    for (code, lang) in LANGUAGES {
        manager.register(&lang_tokenizer_name(code), build_analyzer(*lang));
    }
    manager
}

/// Full-text search store backed by tantivy.
///
/// Thread-safety contract
/// ──────────────────────
/// All language analyzers are pre-registered in `open()` and the tokenizer
/// registry is never mutated after that point.  `add_chunk()` selects the
/// correct per-language indexed field (schema-configured at creation time),
/// and `search()` builds a throwaway local `TokenizerManager` for its
/// `QueryParser` so it never touches the shared registry.
pub struct TantivyStore {
    index: Index,
    reader: IndexReader,
    writer: Arc<RwLock<IndexWriter>>,
    f_chunk_id: tantivy::schema::Field,
    f_doc_id: tantivy::schema::Field,
    /// Per-language content fields: maps canonical 2-letter code → Field.
    /// Each field is indexed with the matching language stemmer and is NOT
    /// stored (the stored copy lives in `f_content_stored`).
    lang_content_fields: HashMap<&'static str, tantivy::schema::Field>,
    /// Stored-only copy of the raw content text (for future retrieval).
    f_content_stored: tantivy::schema::Field,
}

/// Build the schema.
///
/// Returns the schema plus the field handles needed by `TantivyStore`.
fn build_schema() -> (
    Schema,
    tantivy::schema::Field,
    tantivy::schema::Field,
    HashMap<&'static str, tantivy::schema::Field>,
    tantivy::schema::Field,
) {
    let mut builder = Schema::builder();

    let f_chunk_id = builder.add_text_field("chunk_id", STRING | STORED);
    let f_doc_id = builder.add_text_field("doc_id", STRING | STORED);

    // One indexed-but-not-stored field per language.
    let mut lang_content_fields = HashMap::new();
    for (code, _lang) in LANGUAGES {
        let tokenizer = lang_tokenizer_name(code);
        let opts = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(&tokenizer)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let field = builder.add_text_field(&format!("content_{code}"), opts);
        lang_content_fields.insert(*code, field);
    }

    // Stored-only content field for raw text retrieval (no indexing).
    let f_content_stored = builder.add_text_field("content", STORED);

    let schema = builder.build();
    (schema, f_chunk_id, f_doc_id, lang_content_fields, f_content_stored)
}

impl TantivyStore {
    /// Open (or create) a tantivy index at the given directory path.
    pub fn open(index_path: &str) -> anyhow::Result<Self> {
        let (schema, f_chunk_id, f_doc_id, lang_content_fields, f_content_stored) =
            build_schema();

        let path = Path::new(index_path);
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create index directory: {index_path}"))?;

        let dir = MmapDirectory::open(path)
            .with_context(|| format!("Failed to open MmapDirectory at: {index_path}"))?;

        let index = Index::open_or_create(dir, schema)
            .with_context(|| "Failed to open or create tantivy index")?;

        // Register all language analyzers once at open time — never again.
        register_language_analyzers(&index);

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .context("Failed to build IndexReader")?;

        let writer: IndexWriter = index
            .writer(50 * 1024 * 1024)
            .context("Failed to create IndexWriter")?;

        Ok(Self {
            index,
            reader,
            writer: Arc::new(RwLock::new(writer)),
            f_chunk_id,
            f_doc_id,
            lang_content_fields,
            f_content_stored,
        })
    }

    /// Add a chunk to the index (buffered; call [`commit`] to persist).
    ///
    /// The content is indexed using the language-specific stemmer field that
    /// was configured at schema creation time.  No tokenizer re-registration
    /// occurs here, making this method safe to call from multiple threads.
    pub fn add_chunk(
        &self,
        chunk_id: &str,
        doc_id: &str,
        language: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let code = canonical_lang_code(language);
        let lang_field = self
            .lang_content_fields
            .get(code)
            .copied()
            .unwrap_or_else(|| *self.lang_content_fields.get("en").unwrap());

        // Index into the language-specific field; also store in the plain
        // content field for raw-text retrieval.
        let document = doc!(
            self.f_chunk_id      => chunk_id,
            self.f_doc_id        => doc_id,
            lang_field           => content,
            self.f_content_stored => content
        );

        let w = self
            .writer
            .read()
            .map_err(|e| anyhow::anyhow!("Writer lock poisoned: {e}"))?;
        w.add_document(document)
            .context("Failed to add document to tantivy")?;
        Ok(())
    }

    /// Flush buffered changes to the index.
    pub fn commit(&self) -> anyhow::Result<()> {
        let mut w = self
            .writer
            .write()
            .map_err(|e| anyhow::anyhow!("Writer lock poisoned: {e}"))?;
        w.commit().context("Failed to commit tantivy writer")?;
        self.reader
            .reload()
            .context("Failed to reload tantivy reader")?;
        Ok(())
    }

    /// Delete all chunks that belong to the given `doc_id`.
    pub fn delete_by_doc_id(&self, doc_id: &str) -> anyhow::Result<()> {
        let term = Term::from_field_text(self.f_doc_id, doc_id);
        let w = self
            .writer
            .read()
            .map_err(|e| anyhow::anyhow!("Writer lock poisoned: {e}"))?;
        w.delete_term(term);
        Ok(())
    }

    /// Search the index for `query_text` using the stemmer for `language`.
    ///
    /// Returns at most `top_k` results sorted by BM25 score (descending).
    ///
    /// Thread safety: a fresh local `TokenizerManager` is built for each
    /// call so the shared index tokenizer registry is never mutated.
    pub fn search(
        &self,
        query_text: &str,
        language: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<FtsHit>> {
        if query_text.trim().is_empty() {
            return Ok(vec![]);
        }

        let code = canonical_lang_code(language);
        let lang_field = self
            .lang_content_fields
            .get(code)
            .copied()
            .unwrap_or_else(|| *self.lang_content_fields.get("en").unwrap());

        let searcher = self.reader.searcher();

        // Build a local (non-shared) TokenizerManager so we never touch the
        // shared registry.  QueryParser::new() accepts any TokenizerManager,
        // so we can use the per-language analyzer for query tokenization
        // without data races.
        let local_tokenizers = build_local_tokenizer_manager();
        let query_parser = QueryParser::new(
            self.index.schema(),
            vec![lang_field],
            local_tokenizers,
        );

        let query = query_parser
            .parse_query(query_text)
            .map_err(|e| anyhow::anyhow!("Query parse error: {e:?}"))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(top_k).order_by_score())
            .context("Search failed")?;

        let mut hits = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let retrieved: TantivyDocument = searcher
                .doc(doc_address)
                .context("Failed to retrieve document")?;

            let chunk_id = retrieved
                .get_first(self.f_chunk_id)
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let doc_id = retrieved
                .get_first(self.f_doc_id)
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();

            hits.push(FtsHit { chunk_id, doc_id, score });
        }
        Ok(hits)
    }

    /// Delete all documents from the index and commit.
    pub fn clear(&self) -> anyhow::Result<()> {
        {
            let mut w = self
                .writer
                .write()
                .map_err(|e| anyhow::anyhow!("Writer lock poisoned: {e}"))?;
            w.delete_all_documents()
                .context("Failed to delete all documents")?;
            w.commit().context("Failed to commit after clear")?;
        }
        self.reader
            .reload()
            .context("Failed to reload reader after clear")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_store() -> (TantivyStore, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let store = TantivyStore::open(dir.path().to_str().unwrap()).expect("open");
        (store, dir)
    }

    #[test]
    fn index_and_search_english() {
        let (store, _dir) = make_store();
        store
            .add_chunk("chunk-1", "doc-1", "en", "The quick brown fox jumps over the lazy dog")
            .expect("add_chunk");
        store.commit().expect("commit");

        let hits = store.search("fox", "en", 10).expect("search");
        assert!(!hits.is_empty(), "Expected to find at least one hit for 'fox'");
        assert_eq!(hits[0].chunk_id, "chunk-1");
        assert_eq!(hits[0].doc_id, "doc-1");
    }

    #[test]
    fn search_uses_stemming() {
        let (store, _dir) = make_store();
        store
            .add_chunk("chunk-stem", "doc-stem", "en", "She was running through the park every morning")
            .expect("add_chunk");
        store.commit().expect("commit");

        // Search with stem "run" should find the document that contains "running".
        let hits = store.search("run", "en", 10).expect("search");
        assert!(
            !hits.is_empty(),
            "Expected stemming to find 'running' when searching for 'run'"
        );
        assert_eq!(hits[0].chunk_id, "chunk-stem");
    }

    #[test]
    fn delete_by_doc_id() {
        let (store, _dir) = make_store();
        store
            .add_chunk("chunk-del", "doc-del", "en", "This document should be deleted")
            .expect("add_chunk");
        store.commit().expect("commit");

        // Verify it's present first.
        let hits = store.search("deleted", "en", 10).expect("search before delete");
        assert!(!hits.is_empty(), "Document should exist before deletion");

        // Delete and commit.
        store.delete_by_doc_id("doc-del").expect("delete");
        store.commit().expect("commit after delete");

        // Now it should be gone.
        let hits = store.search("deleted", "en", 10).expect("search after delete");
        assert!(hits.is_empty(), "Document should be absent after deletion");
    }

    #[test]
    fn search_empty_returns_empty() {
        let (store, _dir) = make_store();
        // No documents, no commit — just search.
        let hits = store.search("anything", "en", 10).expect("search empty");
        assert!(hits.is_empty(), "Empty index should return empty results");
    }
}
