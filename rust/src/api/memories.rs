use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::repositories::memories::{Memory, MemoryRepository};
use crate::error::AppError;
use crate::search::memory_search::{MemoryHit, MemorySearch};

use super::state::AppState;

#[derive(Deserialize)]
pub struct RememberRequest {
    pub content: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_scope() -> String {
    "default".to_string()
}

fn default_source() -> String {
    "explicit".to_string()
}

#[derive(Deserialize)]
pub struct ObserveRequest {
    pub conversation: String,
    #[serde(default = "default_scope")]
    pub scope: String,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub category: Option<String>,
    pub scope: Option<String>,
}

#[derive(Deserialize)]
pub struct ContextQuery {
    pub scope: Option<String>,
}

/// Body of `POST /api/memory/search` (also the parameters of the MCP `recall` tool).
#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub scope: Option<String>,
    pub category: Option<String>,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    10
}

#[derive(Serialize)]
pub struct SearchHitItem {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub scope: String,
    pub category: Option<String>,
    pub project: Option<String>,
    pub source: String,
    pub created_at: String,
    pub access_count: i64,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHitItem>,
}

/// Hybrid memory search without an LLM; records the access of every returned fact.
pub(crate) fn search_memories(
    state: &AppState,
    conn: &Connection,
    q: &SearchRequest,
) -> anyhow::Result<Vec<MemoryHit>> {
    let retrieval = &state.retrieval;
    let query_vector = state.embed.embed_query(&q.query)?;
    let lang = retrieval.lang.detect(&q.query);
    let search = MemorySearch {
        top_k: q.top_k,
        ..retrieval.memory_search.clone()
    };
    let hits = search.search(
        conn,
        &state.vectors,
        &state.memory_fts,
        &q.query,
        &query_vector,
        &lang,
        q.scope.as_deref(),
        q.category.as_deref(),
    )?;
    let ids: Vec<String> = hits.iter().map(|h| h.memory_id.clone()).collect();
    MemoryRepository::new(conn).touch_accessed(&ids)?;
    Ok(hits)
}

#[derive(Serialize)]
pub struct RememberResponse {
    pub facts_extracted: usize,
    pub memories_updated: usize,
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
    pub scope: String,
    pub access_count: i64,
}

#[derive(Serialize)]
pub struct ProfileResponse {
    #[serde(rename = "static")]
    pub static_summary: String,
    #[serde(rename = "dynamic")]
    pub dynamic_summary: String,
    pub raw_count: usize,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/memory/remember", post(remember_handler))
        .route("/api/memory/observe", post(observe_handler))
        .route("/api/memory/search", post(search_handler))
        .route("/api/memory/list", get(list_memories_handler))
        .route("/api/memory/context", get(context_handler))
        .route("/api/memory/:id", delete(forget_handler))
}

async fn remember_handler(
    State(state): State<AppState>,
    Json(body): Json<RememberRequest>,
) -> Result<Json<RememberResponse>, AppError> {
    let pool = state.pool.clone();
    let memory_svc = state.memory_svc.clone();
    let content = body.content.clone();
    let source = body.source.clone();
    let scope = body.scope.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        tokio::runtime::Handle::current()
            .block_on(async move { memory_svc.remember(&conn, &content, &source, &scope).await })
            .map_err(|e| AppError::Llm(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(Json(RememberResponse {
        facts_extracted: result.facts_extracted,
        memories_updated: result.memories_updated,
    }))
}

async fn observe_handler(
    State(state): State<AppState>,
    Json(body): Json<ObserveRequest>,
) -> Result<Json<RememberResponse>, AppError> {
    let pool = state.pool.clone();
    let memory_svc = state.memory_svc.clone();
    let conversation = body.conversation.clone();
    let scope = body.scope.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        tokio::runtime::Handle::current()
            .block_on(async move { memory_svc.observe(&conn, &conversation, &scope).await })
            .map_err(|e| AppError::Llm(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(Json(RememberResponse {
        facts_extracted: result.facts_extracted,
        memories_updated: result.memories_updated,
    }))
}

async fn search_handler(
    State(state): State<AppState>,
    Json(body): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, AppError> {
    let hits = tokio::task::spawn_blocking(move || {
        let conn = state.pool.get().map_err(AppError::Pool)?;
        search_memories(&state, &conn, &body).map_err(|e| AppError::Llm(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(Json(SearchResponse {
        hits: hits
            .into_iter()
            .map(|h| SearchHitItem {
                id: h.memory_id,
                content: h.content,
                score: h.score,
                scope: h.scope,
                category: h.category,
                project: h.project,
                source: h.source,
                created_at: h.created_at,
                access_count: h.access_count,
            })
            .collect(),
    }))
}

async fn list_memories_handler(
    State(state): State<AppState>,
    Query(params): Query<ListQuery>,
) -> Result<Json<Vec<MemoryItem>>, AppError> {
    let pool = state.pool.clone();
    let category = params.category.clone();
    let scope = params.scope.clone();

    let memories = tokio::task::spawn_blocking(move || -> Result<Vec<Memory>, AppError> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let repo = MemoryRepository::new(&conn);
        let all = repo.get_all_active(scope.as_deref()).map_err(AppError::Db)?;

        // Filter by category if provided
        if let Some(cat) = &category {
            Ok(all
                .into_iter()
                .filter(|m| m.category.as_deref() == Some(cat.as_str()))
                .collect())
        } else {
            Ok(all)
        }
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    let items = memories
        .into_iter()
        .map(|m| MemoryItem {
            id: m.id,
            content: m.content,
            source: m.source,
            category: m.category,
            project: m.project,
            relation: m.relation,
            created_at: m.created_at,
            scope: m.scope,
            access_count: m.access_count,
        })
        .collect();

    Ok(Json(items))
}

async fn context_handler(
    State(state): State<AppState>,
    Query(params): Query<ContextQuery>,
) -> Result<Json<ProfileResponse>, AppError> {
    let pool = state.pool.clone();
    let scope = params.scope.clone();

    // Get all active memories (blocking)
    let memories = tokio::task::spawn_blocking(move || -> Result<Vec<Memory>, AppError> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let repo = MemoryRepository::new(&conn);
        repo.get_all_active(scope.as_deref()).map_err(AppError::Db)
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    // Build profile (async, calls LLM) — profile_svc is Arc<ProfileService> which is Send+Sync
    let profile_svc = state.profile_svc.clone();
    let profile = profile_svc
        .build_profile(&memories)
        .await
        .map_err(|e| AppError::Llm(e.to_string()))?;

    Ok(Json(ProfileResponse {
        static_summary: profile.static_summary,
        dynamic_summary: profile.dynamic_summary,
        raw_count: profile.raw_count,
    }))
}

async fn forget_handler(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.pool.clone();
    let memory_svc = state.memory_svc.clone();
    let memory_id_clone = memory_id.clone();

    let found = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        tokio::runtime::Handle::current()
            .block_on(async move { memory_svc.forget(&conn, &memory_id_clone).await })
            .map_err(|e| AppError::Llm(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    if found {
        Ok(Json(serde_json::json!({"status": "deleted"})))
    } else {
        Err(AppError::NotFound(format!("memory {memory_id}")))
    }
}
