# Task 11: HTTP API (axum)

**Goal:** Все REST-эндпоинты: загрузка документов, запросы (sync + stream), jobs, memory CRUD. Порт Python FastAPI роутеров.

**Files:**
- Create: `rust/src/api/mod.rs`
- Create: `rust/src/api/documents.rs`
- Create: `rust/src/api/query.rs`
- Create: `rust/src/api/jobs.rs`
- Create: `rust/src/api/memories.rs`
- Create: `rust/src/api/state.rs`
- Modify: `rust/src/main.rs`

---

### Task 11.1: AppState

- [ ] **Шаг 1: Создать rust/src/api/state.rs**

```rust
use std::sync::Arc;
use crate::config::Config;
use crate::db::pool::DbPool;
use crate::ingestion::embeddings::EmbeddingClient;
use crate::ingestion::pipeline::IngestionPipeline;
use crate::llm::LlmProvider;
use crate::memory::extractor::FactExtractor;
use crate::memory::profile::ProfileService;
use crate::memory::service::MemorySvc;
use crate::search::{TantivyStore, VectorStore};
use crate::search::reranker::Reranker;
use crate::search::service::RetrievalService;

/// Разделяемое состояние приложения.
#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<DbPool>,
    pub config: Arc<Config>,
    pub tantivy: Arc<TantivyStore>,
    pub vectors: Arc<VectorStore>,
    pub embed: Arc<EmbeddingClient>,
    pub retrieval: Arc<RetrievalService>,
    pub memory_svc: Arc<MemorySvc>,
    pub profile_svc: Arc<ProfileService>,
}
```

---

### Task 11.2: Documents API

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/api/documents.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_response_serializes() {
        let r = UploadResponse {
            job_id: Some("j1".to_string()),
            doc_id: None,
            status: "pending".to_string(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("pending"));
    }
}
```

- [ ] **Шаг 2: Реализовать documents.rs**

```rust
use std::path::PathBuf;
use axum::extract::{Multipart, Path, State};
use axum::response::Json;
use axum::routing::{get, post, delete};
use axum::Router;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::task;
use uuid::Uuid;

use crate::api::state::AppState;
use crate::db::repositories::{documents::DocumentRepository, jobs::JobRepository};
use crate::error::AppError;

#[derive(Serialize)]
pub struct UploadResponse {
    pub job_id: Option<String>,
    pub doc_id: Option<String>,
    pub status: String,
}

#[derive(Serialize)]
pub struct DocumentInfo {
    pub id: String,
    pub source: String,
    pub mime_type: String,
    pub title: Option<String>,
    pub checksum: String,
    pub indexed_at: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/documents", post(upload_document))
        .route("/api/documents", get(list_documents))
        .route("/api/documents/:id", delete(delete_document))
}

async fn upload_document(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, AppError> {
    let field = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
        .ok_or_else(|| AppError::BadRequest("no file field".to_string()))?;

    let filename = field.file_name()
        .unwrap_or("upload")
        .to_string();
    let bytes = field.bytes().await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let checksum = hex::encode(Sha256::digest(&bytes));
    let pool = state.pool.clone();
    let config = state.config.clone();

    task::spawn_blocking(move || {
        let conn = pool.get()?;
        let doc_repo = DocumentRepository::new(&conn);

        if let Some(doc) = doc_repo.get_by_checksum(&checksum)? {
            return Ok::<_, anyhow::Error>(Json(UploadResponse {
                doc_id: Some(doc.id),
                job_id: None,
                status: "already_indexed".to_string(),
            }));
        }

        let job_repo = JobRepository::new(&conn);
        if let Some(job) = job_repo.get_by_checksum_active(&checksum)? {
            return Ok(Json(UploadResponse {
                job_id: Some(job.id),
                doc_id: None,
                status: "already_queued".to_string(),
            }));
        }

        let upload_dir = PathBuf::from(&config.upload_dir);
        std::fs::create_dir_all(&upload_dir)?;
        let dest = upload_dir.join(format!("{}-{}", Uuid::new_v4(), filename));
        std::fs::write(&dest, &bytes)?;

        let job_id = job_repo.create(dest.to_str().unwrap(), &checksum)?;

        Ok(Json(UploadResponse {
            job_id: Some(job_id),
            doc_id: None,
            status: "pending".to_string(),
        }))
    })
    .await
    .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?
    .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))
}

async fn list_documents(
    State(state): State<AppState>,
) -> Result<Json<Vec<DocumentInfo>>, AppError> {
    let pool = state.pool.clone();
    let docs = task::spawn_blocking(move || {
        let conn = pool.get()?;
        DocumentRepository::new(&conn).list_all()
    })
    .await
    .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
    .map_err(AppError::Db)?;

    Ok(Json(docs.into_iter().map(|d| DocumentInfo {
        id: d.id,
        source: d.source,
        mime_type: d.mime_type,
        title: d.title,
        checksum: d.checksum,
        indexed_at: d.indexed_at,
    }).collect()))
}

async fn delete_document(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.pool.clone();
    let tantivy = state.tantivy.clone();
    let vectors = state.vectors.clone();

    task::spawn_blocking(move || {
        let conn = pool.get()?;
        let doc_repo = DocumentRepository::new(&conn);

        if doc_repo.get_by_id(&doc_id)?.is_none() {
            return Err(AppError::NotFound(format!("document {doc_id}")));
        }

        vectors.delete_chunks_for_doc(&conn, &doc_id)?;
        tantivy.delete_by_doc_id(&doc_id)?;
        tantivy.commit()?;
        doc_repo.delete(&doc_id)?;

        Ok(Json(serde_json::json!({"status": "deleted"})))
    })
    .await
    .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
}

#[cfg(test)]
mod tests { /* из Шага 1 */ }
```

---

### Task 11.3: Query API

- [ ] **Шаг 1: Реализовать query.rs**

```rust
use axum::extract::State;
use axum::response::{Json, Sse};
use axum::routing::post;
use axum::Router;
use axum::response::sse::{Event, KeepAlive};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::task;

use crate::api::state::AppState;
use crate::error::AppError;

#[derive(Deserialize)]
pub struct QueryRequest {
    pub query: String,
    #[serde(default)]
    pub top_k: Option<usize>,
    pub memory_category: Option<String>,
}

#[derive(Serialize)]
pub struct QueryResponse {
    pub answer: String,
    pub sources: Vec<serde_json::Value>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/query", post(query_documents))
        .route("/api/search/chunks", post(search_chunks))
}

async fn query_documents(
    State(state): State<AppState>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, AppError> {
    let pool = state.pool.clone();
    let retrieval = state.retrieval.clone();
    let cat = req.memory_category.clone();

    let result = task::spawn_blocking(move || {
        let conn = pool.get()?;
        retrieval.query(&conn, &req.query, cat.as_deref())
    })
    .await
    .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
    .map_err(|e| AppError::Llm(e.to_string()))?;

    Ok(Json(QueryResponse {
        answer: result.answer,
        sources: result.sources,
    }))
}

async fn search_chunks(
    State(state): State<AppState>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let top_k = req.top_k.unwrap_or(5);
    let pool = state.pool.clone();
    let retrieval = state.retrieval.clone();

    let chunks = task::spawn_blocking(move || {
        let conn = pool.get()?;
        retrieval.query(&conn, &req.query, None)
    })
    .await
    .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
    .map_err(|e| AppError::Llm(e.to_string()))?;

    Ok(Json(serde_json::json!({"chunks": chunks.sources.into_iter().take(top_k).collect::<Vec<_>>()})))
}
```

---

### Task 11.4: Jobs + Memories API

- [ ] **Шаг 1: Реализовать jobs.rs**

```rust
use axum::extract::{Path, State};
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use tokio::task;

use crate::api::state::AppState;
use crate::db::repositories::jobs::JobRepository;
use crate::error::AppError;

#[derive(Serialize)]
pub struct JobResponse {
    pub job_id: String,
    pub status: String,
    pub doc_id: Option<String>,
    pub error: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/jobs/:job_id", get(get_job))
}

async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobResponse>, AppError> {
    let pool = state.pool.clone();
    let job = task::spawn_blocking(move || {
        let conn = pool.get()?;
        JobRepository::new(&conn).get_by_id(&job_id)
    })
    .await
    .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
    .map_err(AppError::Db)?
    .ok_or_else(|| AppError::NotFound("job not found".to_string()))?;

    Ok(Json(JobResponse {
        job_id: job.id,
        status: job.status,
        doc_id: job.doc_id,
        error: job.error,
    }))
}
```

- [ ] **Шаг 2: Реализовать memories.rs**

```rust
use axum::extract::{Path, Query, State};
use axum::response::Json;
use axum::routing::{delete, get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use tokio::task;

use crate::api::state::AppState;
use crate::db::repositories::memories::MemoryRepository;
use crate::error::AppError;

#[derive(Deserialize)]
pub struct RememberRequest {
    pub content: String,
    #[serde(default = "default_source")]
    pub source: String,
}
fn default_source() -> String { "explicit".to_string() }

#[derive(Deserialize)]
pub struct ObserveRequest {
    pub conversation: String,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub category: Option<String>,
}

#[derive(Serialize)]
pub struct MemoryItem {
    pub id: String,
    pub content: String,
    pub source: String,
    pub category: Option<String>,
    pub project: Option<String>,
    pub relation: Option<String>,
    pub created_at: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/memory/remember", post(remember))
        .route("/api/memory/observe",  post(observe))
        .route("/api/memory/list",     get(list_memories))
        .route("/api/memory/context",  get(context))
        .route("/api/memory/:id",      delete(forget))
}

async fn remember(
    State(state): State<AppState>,
    Json(req): Json<RememberRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.pool.clone();
    let svc = state.memory_svc.clone();

    let result = svc
        .remember(&pool.get().map_err(AppError::Pool)?, &req.content, &req.source)
        .await
        .map_err(|e| AppError::Llm(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "facts_extracted": result.facts_extracted,
        "memories_updated": result.memories_updated,
    })))
}

async fn observe(
    State(state): State<AppState>,
    Json(req): Json<ObserveRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.pool.clone();
    let svc = state.memory_svc.clone();

    let result = svc
        .observe(&pool.get().map_err(AppError::Pool)?, &req.conversation)
        .await
        .map_err(|e| AppError::Llm(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "facts_extracted": result.facts_extracted,
        "memories_updated": result.memories_updated,
    })))
}

async fn list_memories(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<MemoryItem>>, AppError> {
    let pool = state.pool.clone();
    let cat = q.category;
    let memories = task::spawn_blocking(move || {
        let conn = pool.get()?;
        MemoryRepository::new(&conn).get_all_active(cat.as_deref())
    })
    .await
    .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
    .map_err(AppError::Db)?;

    Ok(Json(memories.into_iter().map(|m| MemoryItem {
        id: m.id,
        content: m.content,
        source: m.source,
        category: m.category,
        project: m.project,
        relation: m.relation,
        created_at: m.created_at,
    }).collect()))
}

async fn context(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.pool.clone();
    let profile_svc = state.profile_svc.clone();

    let memories = task::spawn_blocking(move || {
        let conn = pool.get()?;
        MemoryRepository::new(&conn).get_all_active(None)
    })
    .await
    .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
    .map_err(AppError::Db)?;

    let profile = profile_svc.build_profile(&memories).await
        .map_err(|e| AppError::Llm(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "static": profile.static_summary,
        "dynamic": profile.dynamic_summary,
        "raw_count": profile.raw_count,
    })))
}

async fn forget(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.pool.clone();
    let svc = state.memory_svc.clone();
    let conn = pool.get().map_err(AppError::Pool)?;
    let ok = svc.forget(&conn, &memory_id).await
        .map_err(|e| AppError::Llm(e.to_string()))?;

    if !ok {
        return Err(AppError::NotFound(format!("memory {memory_id}")));
    }
    Ok(Json(serde_json::json!({"status": "deleted"})))
}
```

- [ ] **Шаг 3: Создать api/mod.rs**

```rust
pub mod documents;
pub mod jobs;
pub mod memories;
pub mod query;
pub mod state;

use axum::Router;
use state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(documents::router())
        .merge(query::router())
        .merge(jobs::router())
        .merge(memories::router())
}
```

- [ ] **Шаг 4: Запустить тесты**

```bash
cd rust && cargo test api 2>&1
```

- [ ] **Шаг 5: Коммит**

```bash
git add rust/src/api/
git commit -m "feat(rust): HTTP API — documents, query, jobs, memory endpoints (axum)"
```
