use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tracing::info;

use crate::db::repositories::documents::DocumentRepository;
use crate::db::repositories::jobs::JobRepository;
use crate::error::AppError;

use super::state::AppState;

#[derive(Serialize)]
pub struct UploadResponse {
    pub job_id: Option<String>,
    pub doc_id: Option<String>,
    pub status: String, // "pending" | "already_indexed" | "already_queued"
}

#[derive(Serialize)]
pub struct DocumentItem {
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
        .route("/api/documents/:id", patch(update_document))
        .route("/api/documents/:id/file", get(get_document_file))
}

async fn upload_document(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, AppError> {
    // Read the first field from the multipart upload
    let field = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart error: {e}")))?
        .ok_or_else(|| AppError::BadRequest("no file field in multipart".to_string()))?;

    let filename = field
        .file_name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "upload".to_string());

    let content_type = field
        .content_type()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let bytes = field
        .bytes()
        .await
        .map_err(|e| AppError::BadRequest(format!("failed to read upload bytes: {e}")))?;

    // Compute SHA256 checksum
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let checksum = hex::encode(hasher.finalize());

    let pool = state.pool.clone();
    let upload_dir = state.config.upload_dir.clone();
    let filename_clone = filename.clone();
    let content_type_clone = content_type.clone();
    let bytes_clone = bytes.clone();

    let response = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        let doc_repo = DocumentRepository::new(&conn);
        let job_repo = JobRepository::new(&conn);

        // Check if already indexed
        if let Some(doc) = doc_repo.get_by_checksum(&checksum).map_err(AppError::Db)? {
            return Ok::<UploadResponse, AppError>(UploadResponse {
                job_id: None,
                doc_id: Some(doc.id),
                status: "already_indexed".to_string(),
            });
        }

        // Check if already queued
        if let Some(job) = job_repo
            .get_by_checksum_active(&checksum)
            .map_err(AppError::Db)?
        {
            return Ok(UploadResponse {
                job_id: Some(job.id),
                doc_id: None,
                status: "already_queued".to_string(),
            });
        }

        // Save file to upload directory — prefix with checksum to avoid filename collisions
        std::fs::create_dir_all(&upload_dir)?;
        let unique_name = format!("{}-{}", &checksum[..16], &filename_clone);
        let dest_path: PathBuf = PathBuf::from(&upload_dir).join(&unique_name);
        std::fs::write(&dest_path, &bytes_clone)?;

        let source = dest_path.to_string_lossy().to_string();

        // Create ingestion job
        let job_id = job_repo.create(&source, &checksum).map_err(AppError::Db)?;

        info!(
            job_id = %job_id,
            filename = %filename_clone,
            mime = %content_type_clone,
            "Created ingestion job"
        );

        Ok(UploadResponse {
            job_id: Some(job_id),
            doc_id: None,
            status: "pending".to_string(),
        })
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(Json(response))
}

async fn list_documents(
    State(state): State<AppState>,
) -> Result<Json<Vec<DocumentItem>>, AppError> {
    let pool = state.pool.clone();

    let docs = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        let doc_repo = DocumentRepository::new(&conn);
        doc_repo.list_all().map_err(AppError::Db)
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    let items = docs
        .into_iter()
        .map(|d| DocumentItem {
            id: d.id,
            source: d.source,
            mime_type: d.mime_type,
            title: d.title,
            checksum: d.checksum,
            indexed_at: d.indexed_at,
        })
        .collect();

    Ok(Json(items))
}

async fn delete_document(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let pool = state.pool.clone();
    let tantivy = state.tantivy.clone();
    let vectors = state.vectors.clone();
    let doc_id_clone = doc_id.clone();

    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let doc_repo = DocumentRepository::new(&conn);

        // Check document exists
        let _doc = doc_repo
            .get_by_id(&doc_id_clone)
            .map_err(AppError::Db)?
            .ok_or_else(|| AppError::NotFound(format!("document {doc_id_clone}")))?;

        // Remove chunk vectors
        vectors
            .delete_chunks_for_doc(&conn, &doc_id_clone)
            .map_err(AppError::Db)?;

        // Remove from tantivy (then commit)
        tantivy
            .delete_by_doc_id(&doc_id_clone)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        tantivy
            .commit()
            .map_err(|e| AppError::Parse(e.to_string()))?;

        // Delete from DB (CASCADE removes chunks)
        doc_repo.delete(&doc_id_clone).map_err(AppError::Db)?;

        info!(doc_id = %doc_id_clone, "Deleted document");
        Ok(())
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(StatusCode::NO_CONTENT)
}

async fn get_document_file(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
) -> Result<Response<Body>, AppError> {
    let pool = state.pool.clone();
    let doc_id_clone = doc_id.clone();

    let doc = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        let doc_repo = DocumentRepository::new(&conn);
        doc_repo
            .get_by_id(&doc_id_clone)
            .map_err(AppError::Db)?
            .ok_or_else(|| AppError::NotFound(format!("document {doc_id_clone}")))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    let file_bytes = tokio::fs::read(&doc.source)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::NotFound(format!("file not found on disk: {}", doc.source))
            } else {
                AppError::Io(e)
            }
        })?;

    let mime = mime_guess::from_path(&doc.source)
        .first_or_octet_stream()
        .to_string();

    let filename = std::path::Path::new(&doc.source)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download")
        .to_string();

    // Strip checksum prefix "{16chars}-" if present
    let display_name = if filename.len() > 17 && filename.chars().nth(16) == Some('-') {
        filename[17..].to_string()
    } else {
        filename
    };

    // Sanitize filename: remove control chars and quote/backslash characters to prevent header injection
    let safe_name: String = display_name
        .chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\\')
        .collect();
    let safe_name = if safe_name.is_empty() {
        "download".to_string()
    } else {
        safe_name
    };

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{safe_name}\""),
        )
        .body(Body::from(file_bytes))
        .map_err(|e| AppError::Parse(format!("response build error: {e}")))?;

    Ok(response)
}

#[derive(serde::Deserialize)]
struct UpdateDocumentRequest {
    title: Option<String>,
}

#[derive(Serialize)]
struct UpdateDocumentResponse {
    id: String,
    title: Option<String>,
}

async fn update_document(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    Json(req): Json<UpdateDocumentRequest>,
) -> Result<Json<UpdateDocumentResponse>, AppError> {
    let pool = state.pool.clone();
    let doc_id_clone = doc_id.clone();
    let title = req.title.clone();

    let result = tokio::task::spawn_blocking(move || -> Result<UpdateDocumentResponse, AppError> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let doc_repo = DocumentRepository::new(&conn);

        let updated = doc_repo
            .update_title(&doc_id_clone, title.as_deref())
            .map_err(AppError::Db)?;

        if !updated {
            return Err(AppError::NotFound(format!("document {doc_id_clone}")));
        }

        Ok(UpdateDocumentResponse {
            id: doc_id_clone,
            title,
        })
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(Json(result))
}

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
