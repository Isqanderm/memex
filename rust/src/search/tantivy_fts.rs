use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::Context;
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING};
use tantivy::tokenizer::{Language, LowerCaser, SimpleTokenizer, Stemmer, TextAnalyzer};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

/// A search hit from the full-text index.
#[derive(Debug, Clone)]
pub struct FtsHit {
    pub chunk_id: String,
    pub doc_id: String,
    pub score: f32,
}

/// Tokenizer name used for the content field.
const CONTENT_TOKENIZER: &str = "content_lang";

/// Full-text search store backed by tantivy.
pub struct TantivyStore {
    index: Index,
    reader: IndexReader,
    writer: Arc<RwLock<IndexWriter>>,
    f_chunk_id: tantivy::schema::Field,
    f_doc_id: tantivy::schema::Field,
    f_content: tantivy::schema::Field,
    #[allow(dead_code)]
    f_language: tantivy::schema::Field,
}

/// Build a stemming analyzer for the given language.
fn build_analyzer(lang: Language) -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(LowerCaser)
        .filter(Stemmer::new(lang))
        .build()
}

/// Map a language code string (e.g. "en", "ru") to a tantivy `Language`.
fn lang_from_code(code: &str) -> Language {
    match code.to_lowercase().as_str() {
        "ru" | "russian" => Language::Russian,
        "de" | "german" => Language::German,
        "fr" | "french" => Language::French,
        "es" | "spanish" => Language::Spanish,
        "it" | "italian" => Language::Italian,
        "pt" | "portuguese" => Language::Portuguese,
        "nl" | "dutch" => Language::Dutch,
        "sv" | "swedish" => Language::Swedish,
        "fi" | "finnish" => Language::Finnish,
        "da" | "danish" => Language::Danish,
        "no" | "norwegian" => Language::Norwegian,
        "ar" | "arabic" => Language::Arabic,
        "hu" | "hungarian" => Language::Hungarian,
        "el" | "greek" => Language::Greek,
        "ro" | "romanian" => Language::Romanian,
        "tr" | "turkish" => Language::Turkish,
        _ => Language::English, // default / "en"
    }
}

/// Register language analyzers for all 17 supported languages.
fn register_language_analyzers(index: &Index) {
    let languages: &[(&str, Language)] = &[
        ("lang_en", Language::English),
        ("lang_ru", Language::Russian),
        ("lang_de", Language::German),
        ("lang_fr", Language::French),
        ("lang_es", Language::Spanish),
        ("lang_it", Language::Italian),
        ("lang_pt", Language::Portuguese),
        ("lang_nl", Language::Dutch),
        ("lang_sv", Language::Swedish),
        ("lang_fi", Language::Finnish),
        ("lang_da", Language::Danish),
        ("lang_no", Language::Norwegian),
        ("lang_ar", Language::Arabic),
        ("lang_hu", Language::Hungarian),
        ("lang_el", Language::Greek),
        ("lang_ro", Language::Romanian),
        ("lang_tr", Language::Turkish),
    ];
    for (name, lang) in languages {
        index.tokenizers().register(name, build_analyzer(*lang));
    }
    // Default content tokenizer (English).
    index
        .tokenizers()
        .register(CONTENT_TOKENIZER, build_analyzer(Language::English));
}

/// Build the schema used by the full-text store.
fn build_schema() -> (Schema, tantivy::schema::Field, tantivy::schema::Field, tantivy::schema::Field, tantivy::schema::Field) {
    let mut schema_builder = Schema::builder();

    let f_chunk_id = schema_builder.add_text_field("chunk_id", STRING | STORED);
    let f_doc_id = schema_builder.add_text_field("doc_id", STRING | STORED);
    let f_language = schema_builder.add_text_field("language", STRING | STORED);

    // Content field uses our named tokenizer so we can swap it per-query.
    let content_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(CONTENT_TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();

    let f_content = schema_builder.add_text_field("content", content_options);
    let schema = schema_builder.build();

    (schema, f_chunk_id, f_doc_id, f_content, f_language)
}

impl TantivyStore {
    /// Open (or create) a tantivy index at the given directory path.
    pub fn open(index_path: &str) -> anyhow::Result<Self> {
        let (schema, f_chunk_id, f_doc_id, f_content, f_language) = build_schema();

        // Ensure the directory exists.
        let path = Path::new(index_path);
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create index directory: {index_path}"))?;

        let dir = MmapDirectory::open(path)
            .with_context(|| format!("Failed to open MmapDirectory at: {index_path}"))?;

        let index = Index::open_or_create(dir, schema)
            .with_context(|| "Failed to open or create tantivy index")?;

        // Register all language analyzers on the freshly opened index.
        register_language_analyzers(&index);

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .context("Failed to build IndexReader")?;

        // 50 MB heap budget.
        let writer: IndexWriter = index
            .writer(50 * 1024 * 1024)
            .context("Failed to create IndexWriter")?;

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

    /// Add a chunk to the index (buffered; call [`commit`] to persist).
    pub fn add_chunk(
        &self,
        chunk_id: &str,
        doc_id: &str,
        language: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        // Register the language-specific analyzer as CONTENT_TOKENIZER so
        // it is used during indexing of this chunk.
        let lang = lang_from_code(language);
        self.index
            .tokenizers()
            .register(CONTENT_TOKENIZER, build_analyzer(lang));

        let document = doc!(
            self.f_chunk_id => chunk_id,
            self.f_doc_id   => doc_id,
            self.f_language => language,
            self.f_content  => content
        );

        let w = self.writer.read().map_err(|e| anyhow::anyhow!("Writer lock poisoned: {e}"))?;
        w.add_document(document).context("Failed to add document to tantivy")?;
        Ok(())
    }

    /// Flush buffered changes to the index.
    pub fn commit(&self) -> anyhow::Result<()> {
        let mut w = self
            .writer
            .write()
            .map_err(|e| anyhow::anyhow!("Writer lock poisoned: {e}"))?;
        w.commit().context("Failed to commit tantivy writer")?;
        // Explicitly reload the reader so tests using Manual/OnCommit see fresh data.
        self.reader.reload().context("Failed to reload tantivy reader")?;
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
    pub fn search(
        &self,
        query_text: &str,
        language: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<FtsHit>> {
        if query_text.trim().is_empty() {
            return Ok(vec![]);
        }

        // Swap the content tokenizer to the requested language before parsing.
        let lang = lang_from_code(language);
        self.index
            .tokenizers()
            .register(CONTENT_TOKENIZER, build_analyzer(lang));

        let searcher = self.reader.searcher();

        let query_parser = QueryParser::for_index(&self.index, vec![self.f_content]);
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
            w.delete_all_documents().context("Failed to delete all documents")?;
            w.commit().context("Failed to commit after clear")?;
        }
        self.reader.reload().context("Failed to reload reader after clear")?;
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
