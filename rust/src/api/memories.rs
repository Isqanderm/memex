use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::db::repositories::memories::{Memory, MemoryRepository};
use crate::error::AppError;

use super::state::AppState;

#[derive(Deserialize)]
pub struct RememberRequest {
    pub content: String,
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "explicit".to_string()
}

#[derive(Deserialize)]
pub struct ObserveRequest {
    pub conversation: String,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub category: Option<String>,
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

    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        tokio::runtime::Handle::current()
            .block_on(async move { memory_svc.remember(&conn, &content, &source).await })
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

    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        tokio::runtime::Handle::current()
            .block_on(async move { memory_svc.observe(&conn, &conversation).await })
            .map_err(|e| AppError::Llm(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(Json(RememberResponse {
        facts_extracted: result.facts_extracted,
        memories_updated: result.memories_updated,
    }))
}

async fn list_memories_handler(
    State(state): State<AppState>,
    Query(params): Query<ListQuery>,
) -> Result<Json<Vec<MemoryItem>>, AppError> {
    let pool = state.pool.clone();
    let category = params.category.clone();

    let memories = tokio::task::spawn_blocking(move || -> Result<Vec<Memory>, AppError> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let repo = MemoryRepository::new(&conn);
        let all = repo.get_all_active().map_err(AppError::Db)?;

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
        })
        .collect();

    Ok(Json(items))
}

async fn context_handler(
    State(state): State<AppState>,
) -> Result<Json<ProfileResponse>, AppError> {
    let pool = state.pool.clone();

    // Get all active memories (blocking)
    let memories = tokio::task::spawn_blocking(move || -> Result<Vec<Memory>, AppError> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let repo = MemoryRepository::new(&conn);
        repo.get_all_active().map_err(AppError::Db)
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
