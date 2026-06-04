use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::db::repositories::jobs::JobRepository;
use crate::error::AppError;

use super::state::AppState;

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
    let job_id_clone = job_id.clone();

    let job = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        let job_repo = JobRepository::new(&conn);
        job_repo
            .get_by_id(&job_id_clone)
            .map_err(AppError::Db)?
            .ok_or_else(|| AppError::NotFound(format!("job {job_id_clone}")))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(Json(JobResponse {
        job_id: job.id,
        status: job.status,
        doc_id: job.doc_id,
        error: job.error,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_response_serializes() {
        let r = JobResponse {
            job_id: "j1".to_string(),
            status: "done".to_string(),
            doc_id: None,
            error: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("done"));
    }
}
