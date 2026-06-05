# Task 12: UI, SSE Streaming, main.rs

**Goal:** Web UI через minijinja (копия Python шаблонов), SSE стриминг для поиска, финальный main.rs с полным жизненным циклом приложения.

**Files:**
- Create: `rust/src/api/ui.rs`
- Modify: `rust/src/main.rs`
- Copy: `templates/` → `rust/templates/` (без изменений)
- Copy: `static/` → `rust/static/` (без изменений)

---

### Task 12.1: Скопировать шаблоны и статику

- [ ] **Шаг 1: Скопировать ресурсы**

```bash
cp -r templates/ rust/templates/
cp -r static/    rust/static/
```

Шаблоны уже написаны на Jinja2 — minijinja полностью совместим, изменений не нужно.

---

### Task 12.2: UI обработчики

- [ ] **Шаг 1: Реализовать ui.rs**

```rust
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use axum::extract::{Form, State};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use minijinja::{Environment, Value};
use tokio::task;

use crate::api::state::AppState;
use crate::db::repositories::{documents::DocumentRepository, jobs::JobRepository, memories::MemoryRepository};
use crate::error::AppError;

static TEMPLATES: OnceLock<Environment<'static>> = OnceLock::new();

fn get_env() -> &'static Environment<'static> {
    TEMPLATES.get_or_init(|| {
        let mut env = Environment::new();
        env.set_loader(minijinja::path_loader("templates"));
        env
    })
}

fn render(template_name: &str, ctx: impl serde::Serialize) -> Result<Html<String>, AppError> {
    let env = get_env();
    let tmpl = env.get_template(template_name)
        .map_err(|e| AppError::Parse(format!("template {template_name}: {e}")))?;
    let html = tmpl.render(ctx)
        .map_err(|e| AppError::Parse(format!("render {template_name}: {e}")))?;
    Ok(Html(html))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/",           get(index))
        .route("/documents",  get(documents_page))
        .route("/upload",     get(upload_page))
        .route("/jobs-fragment", get(jobs_fragment))
        .route("/search",     post(search_html))
        .route("/search/stream", post(search_stream))
}

async fn index(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let pool = state.pool.clone();
    let count = task::spawn_blocking(move || {
        let conn = pool.get()?;
        let mut stmt = conn.prepare("SELECT count(*) FROM documents")?;
        let n: i64 = stmt.query_row([], |r| r.get(0))?;
        Ok::<_, anyhow::Error>(n)
    })
    .await
    .map_err(|e| AppError::Parse(e.to_string()))?
    .unwrap_or(0);

    render("index.html", serde_json::json!({ "active_page": "search", "doc_count": count }))
}

async fn documents_page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let pool = state.pool.clone();
    let (docs, active_jobs) = task::spawn_blocking(move || {
        let conn = pool.get()?;
        let docs = DocumentRepository::new(&conn).list_all()?;
        let jobs = JobRepository::new(&conn).list_active()?;
        Ok::<_, anyhow::Error>((docs, jobs))
    })
    .await
    .map_err(|e| AppError::Parse(e.to_string()))?
    .map_err(|e| AppError::Parse(e.to_string()))?;

    render("documents.html", serde_json::json!({
        "docs": docs.iter().map(|d| serde_json::json!({
            "id": d.id, "source": d.source, "mime_type": d.mime_type,
            "title": d.title, "indexed_at": d.indexed_at,
        })).collect::<Vec<_>>(),
        "active_jobs": active_jobs.iter().map(|j| serde_json::json!({
            "id": j.id, "status": j.status, "source": j.source, "error": j.error,
        })).collect::<Vec<_>>(),
        "active_page": "documents",
        "doc_count": docs.len(),
    }))
}

async fn upload_page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    render("upload.html", serde_json::json!({ "active_page": "upload", "doc_count": 0 }))
}

async fn jobs_fragment(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let pool = state.pool.clone();
    let jobs = task::spawn_blocking(move || {
        let conn = pool.get()?;
        JobRepository::new(&conn).list_active()
    })
    .await
    .map_err(|e| AppError::Parse(e.to_string()))?
    .map_err(AppError::Db)?;

    render("_jobs_fragment.html", serde_json::json!({
        "active_jobs": jobs.iter().map(|j| serde_json::json!({
            "id": j.id, "status": j.status, "source": j.source, "error": j.error,
        })).collect::<Vec<_>>(),
    }))
}

#[derive(serde::Deserialize)]
pub struct SearchForm {
    pub query: String,
}

async fn search_html(
    State(state): State<AppState>,
    Form(form): Form<SearchForm>,
) -> Result<Html<String>, AppError> {
    let pool = state.pool.clone();
    let retrieval = state.retrieval.clone();
    let query = form.query.clone();

    let result = task::spawn_blocking(move || {
        let conn = pool.get()?;
        retrieval.query(&conn, &query, None)
    })
    .await
    .map_err(|e| AppError::Parse(e.to_string()))?
    .map_err(|e| AppError::Llm(e.to_string()))?;

    render("_results.html", serde_json::json!({
        "query": form.query,
        "answer": result.answer,
        "sources": result.sources,
    }))
}

async fn search_stream(
    State(state): State<AppState>,
    Form(form): Form<SearchForm>,
) -> Response {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use async_stream::stream;
    use futures::StreamExt;

    let pool = state.pool.clone();
    let retrieval = state.retrieval.clone();
    let query = form.query.clone();

    let stream = stream! {
        // Выполняем retrieval синхронно в spawn_blocking
        let result = tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            retrieval.query(&conn, &query, None)
        }).await;

        match result {
            Ok(Ok(r)) => {
                // Стриминг ответа посимвольно (приближение — реальный streaming потребует
                // отдельного stream-endpoint в RetrievalService)
                yield Ok(Event::default()
                    .event("token")
                    .data(serde_json::to_string(&r.answer).unwrap()));
                yield Ok(Event::default()
                    .event("sources")
                    .data(serde_json::to_string(&r.sources).unwrap()));
                yield Ok(Event::default().event("done").data("{}"));
            }
            Ok(Err(e)) => {
                yield Ok(Event::default()
                    .event("error")
                    .data(serde_json::json!({"message": e.to_string()}).to_string()));
            }
            Err(e) => {
                yield Ok(Event::default()
                    .event("error")
                    .data(serde_json::json!({"message": e.to_string()}).to_string()));
            }
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
```

---

### Task 12.3: Финальный main.rs

- [ ] **Шаг 1: Обновить rust/src/main.rs**

```rust
mod api;
mod config;
mod db;
mod error;
mod ingestion;
mod llm;
mod memory;
mod search;

use std::sync::Arc;
use tokio::sync::watch;
use tower_http::services::ServeDir;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use api::state::AppState;
use config::Config;
use db::pool::build_pool;
use ingestion::adapters::build_default_registry;
use ingestion::chunker::SmallToBigChunker;
use ingestion::embeddings::EmbeddingClient;
use ingestion::language::LanguageDetector;
use ingestion::pipeline::IngestionPipeline;
use ingestion::worker::IngestionWorker;
use llm::create_llm_provider;
use memory::extractor::FactExtractor;
use memory::profile::ProfileService;
use memory::service::MemorySvc;
use memory::worker::MemoryExpiryWorker;
use search::context::ContextBuilder;
use search::reranker::Reranker;
use search::service::RetrievalService;
use search::{TantivyStore, VectorStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "memex=info,warn".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env()
        .map_err(|e| anyhow::anyhow!("config error: {e}"))?;

    info!("Starting Memex on {}:{}", config.host, config.port);

    // Создать директории
    std::fs::create_dir_all(&config.upload_dir)?;
    std::fs::create_dir_all(
        std::path::Path::new(&config.database_path).parent().unwrap_or(std::path::Path::new(".")),
    )?;

    // Инициализация компонентов
    let pool = Arc::new(build_pool(&config.database_path)?);
    let tantivy = Arc::new(TantivyStore::open(&config.tantivy_path)?);
    let vectors = Arc::new(VectorStore::new(config.embedding_dimensions));

    info!("Loading embedding model {}...", config.local_embedding_model);
    let embed = Arc::new(EmbeddingClient::new(&config.local_embedding_model)?);
    info!("Embedding model loaded ({} dims)", embed.dimensions());

    info!("Loading reranker model...");
    let reranker = Arc::new(Reranker::new()?);
    info!("Reranker loaded");

    let llm = create_llm_provider(&config)?;
    let extractor = Arc::new(FactExtractor::new(llm.clone()));
    let profile_svc = Arc::new(ProfileService::new(llm.clone()));
    let memory_svc = Arc::new(MemorySvc::new(extractor.clone(), embed.clone(), vectors.clone()));

    let retrieval = Arc::new(RetrievalService {
        embed: embed.clone(),
        tantivy: tantivy.clone(),
        vectors: vectors.clone(),
        reranker,
        llm,
        context_builder: ContextBuilder,
        memory_search: search::memory_search::MemorySearch::new(),
        lang: LanguageDetector,
        semantic_top_k: config.semantic_top_k,
        bm25_top_k: config.bm25_top_k,
        rrf_k: config.rrf_k,
        reranker_top_n: config.reranker_top_n,
    });

    let pipeline = Arc::new(IngestionPipeline {
        adapters: build_default_registry(),
        chunker: SmallToBigChunker::new(
            config.l2_chunk_size,
            config.l1_chunk_size,
            config.l2_chunk_overlap,
        ),
        embed: embed.clone(),
        lang: LanguageDetector,
        batch_size: 64,
    });

    // Канал отключения
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Фоновые задачи
    let ingestion_worker = Arc::new(IngestionWorker {
        pool: pool.clone(),
        pipeline,
        tantivy: tantivy.clone(),
        vectors: vectors.clone(),
    });
    let expiry_worker = Arc::new(MemoryExpiryWorker {
        pool: pool.clone(),
        svc: memory_svc.clone(),
        interval_secs: 3600,
    });

    let mut rx1 = shutdown_rx.clone();
    let mut rx2 = shutdown_rx.clone();
    let w1 = ingestion_worker.clone();
    let w2 = expiry_worker.clone();

    tokio::spawn(async move { w1.run(rx1).await });
    tokio::spawn(async move { w2.run(rx2).await });

    // HTTP сервер
    let state = AppState {
        pool,
        config: Arc::new(config.clone()),
        tantivy,
        vectors,
        embed,
        retrieval,
        memory_svc,
        profile_svc,
    };

    let app = axum::Router::new()
        .merge(api::router())
        .merge(api::ui::router())
        .nest_service("/static", ServeDir::new("static"))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .with_state(state);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Memex ready at http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("Shutting down...");
            let _ = shutdown_tx.send(true);
        })
        .await?;

    Ok(())
}
```

- [ ] **Шаг 2: Проверить что проект собирается**

```bash
cd rust && cargo build 2>&1 | tail -10
```

Ожидаем: `Finished`.

- [ ] **Шаг 3: Запустить полный тест**

```bash
cd rust && cargo test 2>&1
```

Ожидаем: все юнит-тесты зелёные.

- [ ] **Шаг 4: Smoke тест сервера (требует .env)**

```bash
cd rust && cargo run &
sleep 3
curl http://localhost:8000/health
kill %1
```

Ожидаем: `ok`

- [ ] **Шаг 5: Коммит**

```bash
git add rust/src/main.rs rust/src/api/ui.rs rust/templates/ rust/static/
git commit -m "feat(rust): UI handlers (minijinja), SSE streaming, complete main.rs lifecycle"
```
