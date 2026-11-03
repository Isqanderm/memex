use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::repositories::chunks::ChunkRepository;
use crate::error::AppError;

use super::state::AppState;

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

#[derive(Serialize)]
pub struct ChunkHit {
    pub chunk_id: String,
    pub doc_id: String,
    pub content: String,
    pub score: f32,
    pub section_heading: Option<String>,
    pub page_number: Option<u32>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/query", post(query_handler))
        .route("/api/search/chunks", post(search_chunks_handler))
}

async fn query_handler(
    State(state): State<AppState>,
    Json(body): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, AppError> {
    let pool = state.pool.clone();
    let retrieval = state.retrieval.clone();
    let query = body.query.clone();
    let memory_category = body.memory_category.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        retrieval
            .query(&conn, &query, memory_category.as_deref())
            .map_err(|e| AppError::Llm(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(Json(QueryResponse {
        answer: result.answer,
        sources: result.sources,
    }))
}

#[derive(Serialize)]
struct ChunksResponse {
    chunks: Vec<ChunkHit>,
}

/// Vector-only chunk search: embeds `query`, takes the `top_k` nearest chunks and
/// loads their metadata. Shared by `POST /api/search/chunks` and the MCP
/// `search_documents` tool.
pub(crate) fn search_chunks(
    state: &AppState,
    conn: &Connection,
    query: &str,
    top_k: usize,
) -> Result<Vec<ChunkHit>, AppError> {
    let query_vector = state
        .embed
        .embed_query(query)
        .map_err(|e| AppError::Embedding(e.to_string()))?;

    let vector_hits = state
        .vectors
        .search_chunks(conn, &query_vector, top_k)
        .map_err(AppError::Db)?;

    let ids: Vec<String> = vector_hits.iter().map(|h| h.id.clone()).collect();
    let chunks = ChunkRepository::new(conn).get_by_ids(&ids).map_err(AppError::Db)?;

    let mut hits: Vec<ChunkHit> = chunks
        .into_iter()
        .map(|c| {
            let score = vector_hits
                .iter()
                .find(|h| h.id == c.id)
                .map(|h| 1.0 - h.distance)
                .unwrap_or(0.0);
            ChunkHit {
                chunk_id: c.id,
                doc_id: c.doc_id,
                content: c.content,
                score,
                section_heading: c.section_heading,
                page_number: c.page_number.map(|p| p as u32),
            }
        })
        .collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(hits)
}

async fn search_chunks_handler(
    State(state): State<AppState>,
    Json(body): Json<QueryRequest>,
) -> Result<Json<ChunksResponse>, AppError> {
    let top_k = body.top_k.unwrap_or(10);
    let hits = tokio::task::spawn_blocking(move || {
        let conn = state.pool.get().map_err(AppError::Pool)?;
        search_chunks(&state, &conn, &body.query, top_k)
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(Json(ChunksResponse { chunks: hits }))
}
