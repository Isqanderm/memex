# Task 5: Embeddings & Reranker (fastembed)

**Goal:** EmbeddingClient на базе fastembed ONNX (multilingual-e5-small, 384d) и Reranker (bge-reranker-base). Замена sentence-transformers без потери качества.

**Files:**
- Create: `rust/src/ingestion/embeddings.rs`
- Create: `rust/src/search/reranker.rs`
- Modify: `rust/src/ingestion/mod.rs`
- Modify: `rust/src/search/mod.rs`

---

### Task 5.1: EmbeddingClient

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/ingestion/embeddings.rs
#[cfg(test)]
mod tests {
    use super::*;

    // ВНИМАНИЕ: этот тест скачивает ONNX-модель (~90 MB) при первом запуске.
    // Запускать только вручную: cargo test embeddings -- --ignored
    #[test]
    #[ignore]
    fn embed_returns_384_dims() {
        let client = EmbeddingClient::new("intfloat/multilingual-e5-small").unwrap();
        let vecs = client.embed_passages(&["Hello, world!"]).unwrap();
        assert_eq!(vecs.len(), 1);
        assert_eq!(vecs[0].len(), 384);
        // Проверяем нормализацию: ||v|| ≈ 1.0
        let norm: f32 = vecs[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "vector should be normalized, got norm={norm}");
    }

    #[test]
    #[ignore]
    fn query_prefix_differs_from_passage() {
        let client = EmbeddingClient::new("intfloat/multilingual-e5-small").unwrap();
        let passage = client.embed_passages(&["The cat sat on the mat"]).unwrap();
        let query   = client.embed_queries(&["cat mat"]).unwrap();
        // Не идентичны, но оба нормализованы
        assert_ne!(passage[0], query[0]);
        let norm: f32 = query[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01);
    }
}
```

- [ ] **Шаг 2: Запустить — убедиться что FAIL (компиляция)**

```bash
cd rust && cargo test embeddings 2>&1 | tail -5
```

- [ ] **Шаг 3: Реализовать EmbeddingClient**

```rust
use std::sync::{Arc, Mutex};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub struct EmbeddingClient {
    model: Arc<Mutex<TextEmbedding>>,
    dimensions: usize,
}

impl EmbeddingClient {
    pub fn new(model_name: &str) -> anyhow::Result<Self> {
        let embedding_model = match model_name {
            "intfloat/multilingual-e5-small" => EmbeddingModel::MultilingualE5Small,
            "intfloat/multilingual-e5-base"  => EmbeddingModel::MultilingualE5Base,
            "intfloat/multilingual-e5-large" => EmbeddingModel::MultilingualE5Large,
            other => anyhow::bail!("unsupported embedding model: {other}"),
        };

        let model = TextEmbedding::try_new(
            InitOptions::new(embedding_model).with_show_download_progress(true),
        )
        .map_err(|e| anyhow::anyhow!("failed to load embedding model: {e}"))?;

        // Определяем размерность через пробный embed
        let probe = model
            .embed(vec!["probe"], None)
            .map_err(|e| anyhow::anyhow!("embedding probe failed: {e}"))?;
        let dimensions = probe[0].len();

        Ok(Self {
            model: Arc::new(Mutex::new(model)),
            dimensions,
        })
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Embed passages (с префиксом "passage: " для e5-стиля).
    pub fn embed_passages(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let prefixed: Vec<String> = texts
            .iter()
            .map(|t| format!("passage: {t}"))
            .collect();
        let text_refs: Vec<&str> = prefixed.iter().map(|s| s.as_str()).collect();
        let model = self.model.lock().unwrap();
        model
            .embed(text_refs, None)
            .map_err(|e| anyhow::anyhow!("embedding failed: {e}"))
    }

    /// Embed запросы (с префиксом "query: " для e5-стиля).
    pub fn embed_queries(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let prefixed: Vec<String> = texts
            .iter()
            .map(|t| format!("query: {t}"))
            .collect();
        let text_refs: Vec<&str> = prefixed.iter().map(|s| s.as_str()).collect();
        let model = self.model.lock().unwrap();
        model
            .embed(text_refs, None)
            .map_err(|e| anyhow::anyhow!("embedding failed: {e}"))
    }

    /// Embed один запрос.
    pub fn embed_query(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let vecs = self.embed_queries(&[text])?;
        Ok(vecs.into_iter().next().unwrap())
    }
}
```

> **Примечание:** API fastembed `TextEmbedding::try_new` и `model.embed()` нужно проверить против установленной версии `fastembed 4.x`. Если имена вариантов `EmbeddingModel` отличаются — сверить с `cargo doc --open`.

---

### Task 5.2: Reranker

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/search/reranker.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Скачивает ~100 MB модель при первом запуске
    fn reranker_puts_most_relevant_first() {
        let reranker = Reranker::new().unwrap();
        let docs = vec![
            "Rust is a systems programming language focused on safety",
            "Python is great for data science and machine learning",
            "Rust memory safety is guaranteed by the borrow checker",
        ];
        let results = reranker.rerank("Rust memory safety", &docs, 3).unwrap();

        assert_eq!(results.len(), 3);
        // Первый результат должен быть про borrow checker
        assert!(
            docs[results[0].original_index].contains("borrow checker"),
            "most relevant doc should be about borrow checker, got: {}",
            docs[results[0].original_index]
        );
    }

    #[test]
    #[ignore]
    fn rerank_empty_returns_empty() {
        let reranker = Reranker::new().unwrap();
        let results = reranker.rerank("query", &[], 5).unwrap();
        assert!(results.is_empty());
    }
}
```

- [ ] **Шаг 2: Реализовать Reranker**

```rust
use fastembed::reranking::{RerankInitOptions, RerankerModel, TextRerank};

/// Результат переранжирования.
#[derive(Debug, Clone)]
pub struct RerankResult {
    pub original_index: usize,
    pub score: f32,
}

pub struct Reranker {
    model: TextRerank,
}

impl Reranker {
    /// Загрузить bge-reranker-base (многоязычный, ~100 MB ONNX).
    pub fn new() -> anyhow::Result<Self> {
        let model = TextRerank::try_new(
            RerankInitOptions::new(RerankerModel::BGERerankerBase)
                .with_show_download_progress(true),
        )
        .map_err(|e| anyhow::anyhow!("failed to load reranker: {e}"))?;
        Ok(Self { model })
    }

    /// Переранжировать документы по релевантности к запросу.
    /// Возвращает top_n результатов, отсортированных от наиболее к наименее релевантному.
    pub fn rerank(
        &self,
        query: &str,
        documents: &[&str],
        top_n: usize,
    ) -> anyhow::Result<Vec<RerankResult>> {
        if documents.is_empty() {
            return Ok(vec![]);
        }

        let results = self.model
            .rerank(query, documents.to_vec(), false, None)
            .map_err(|e| anyhow::anyhow!("reranking failed: {e}"))?;

        let mut ranked: Vec<RerankResult> = results
            .into_iter()
            .map(|r| RerankResult {
                original_index: r.index,
                score: r.score as f32,
            })
            .collect();

        ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        ranked.truncate(top_n);

        Ok(ranked)
    }
}

#[cfg(test)]
mod tests {
    // (код тестов из Шага 1 выше)
}
```

> **Примечание:** API `TextRerank::try_new`, `RerankInitOptions`, `RerankerModel::BGERerankerBase` — проверить против `fastembed 4.x`. Поле `r.index` может называться иначе в конкретной версии.

- [ ] **Шаг 3: Обновить mod.rs**

```rust
// rust/src/ingestion/mod.rs — добавить:
pub mod embeddings;
pub use embeddings::EmbeddingClient;

// rust/src/search/mod.rs — добавить:
pub mod reranker;
pub use reranker::Reranker;
```

- [ ] **Шаг 4: Проверить компиляцию (тесты помечены #[ignore] — достаточно cargo build)**

```bash
cd rust && cargo build 2>&1 | tail -5
```

Ожидаем: `Finished` без ошибок.

- [ ] **Шаг 5: Запустить ignored тесты вручную (опционально, требует интернета)**

```bash
cd rust && cargo test embeddings -- --ignored 2>&1
cd rust && cargo test reranker -- --ignored 2>&1
```

- [ ] **Шаг 6: Коммит**

```bash
git add rust/src/ingestion/embeddings.rs rust/src/search/reranker.rs
git commit -m "feat(rust): fastembed EmbeddingClient (multilingual-e5-small) + Reranker (bge-base)"
```
