use axum::extract::{Form, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::Router;
use async_stream::stream;
use minijinja::Environment;
use serde::Serialize;
use std::sync::OnceLock;

use crate::api::state::AppState;
use crate::db::repositories::documents::DocumentRepository;
use crate::db::repositories::jobs::JobRepository;
use crate::error::AppError;

static TEMPLATES: OnceLock<Environment<'static>> = OnceLock::new();

fn get_env() -> &'static Environment<'static> {
    TEMPLATES.get_or_init(|| {
        let templates_dir = std::env::var("TEMPLATES_DIR")
            .unwrap_or_else(|_| "templates".to_string());
        let mut env = Environment::new();
        env.set_loader(minijinja::path_loader(templates_dir));
        env
    })
}

fn render(template_name: &str, ctx: impl Serialize) -> Result<Html<String>, AppError> {
    let env = get_env();
    let tmpl = env
        .get_template(template_name)
        .map_err(|e| AppError::Parse(format!("template {template_name}: {e}")))?;
    let html = tmpl
        .render(minijinja::Value::from_serialize(&ctx))
        .map_err(|e| AppError::Parse(format!("render {template_name}: {e}")))?;
    Ok(Html(html))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/documents", get(documents_page))
        .route("/upload", get(upload_page))
        .route("/jobs-fragment", get(jobs_fragment))
        .route("/search", post(search_html))
        .route("/search/stream", post(search_stream))
}

async fn index(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let pool = state.pool.clone();

    let doc_count = tokio::task::spawn_blocking(move || -> Result<i64, AppError> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .map_err(AppError::Db)?;
        Ok(count)
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    #[derive(Serialize)]
    struct Ctx {
        active_page: &'static str,
        doc_count: i64,
    }

    render("index.html", Ctx { active_page: "search", doc_count })
}

#[derive(Serialize)]
struct DocumentItem {
    id: String,
    source: String,
    mime_type: String,
    title: Option<String>,
    checksum: String,
    indexed_at: String,
}

#[derive(Serialize)]
struct JobItem {
    id: String,
    status: String,
    source: String,
    error: Option<String>,
}

async fn documents_page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let pool = state.pool.clone();

    let (docs, jobs) = tokio::task::spawn_blocking(move || -> Result<(Vec<DocumentItem>, Vec<JobItem>), AppError> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let doc_repo = DocumentRepository::new(&conn);
        let job_repo = JobRepository::new(&conn);

        let docs = doc_repo
            .list_all()
            .map_err(AppError::Db)?
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

        let jobs = job_repo
            .list_active()
            .map_err(AppError::Db)?
            .into_iter()
            .map(|j| JobItem {
                id: j.id,
                status: j.status,
                source: j.source,
                error: j.error,
            })
            .collect();

        Ok((docs, jobs))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    #[derive(Serialize)]
    struct Ctx {
        active_page: &'static str,
        documents: Vec<DocumentItem>,
        jobs: Vec<JobItem>,
    }

    render(
        "documents.html",
        Ctx {
            active_page: "documents",
            documents: docs,
            jobs,
        },
    )
}

async fn upload_page() -> Result<Html<String>, AppError> {
    #[derive(Serialize)]
    struct Ctx {
        active_page: &'static str,
    }
    render("upload.html", Ctx { active_page: "upload" })
}

async fn jobs_fragment(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let pool = state.pool.clone();

    let jobs = tokio::task::spawn_blocking(move || -> Result<Vec<JobItem>, AppError> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let job_repo = JobRepository::new(&conn);
        let jobs = job_repo
            .list_active()
            .map_err(AppError::Db)?
            .into_iter()
            .map(|j| JobItem {
                id: j.id,
                status: j.status,
                source: j.source,
                error: j.error,
            })
            .collect();
        Ok(jobs)
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    #[derive(Serialize)]
    struct Ctx {
        jobs: Vec<JobItem>,
    }

    render("_jobs_fragment.html", Ctx { jobs })
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

    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        retrieval
            .query(&conn, &query, None)
            .map_err(|e| AppError::Llm(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    #[derive(Serialize)]
    struct Ctx {
        query: String,
        answer: String,
        sources: Vec<serde_json::Value>,
    }

    render(
        "_results.html",
        Ctx {
            query: form.query,
            answer: result.answer,
            sources: result.sources,
        },
    )
}

async fn search_stream(
    State(state): State<AppState>,
    Form(form): Form<SearchForm>,
) -> impl IntoResponse {
    let pool = state.pool.clone();
    let retrieval = state.retrieval.clone();
    let query = form.query;

    let s = stream! {
        let query_clone = query.clone();
        let result = tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| anyhow::anyhow!("{e}"))?;
            retrieval
                .query(&conn, &query_clone, None)
                .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .await;

        match result {
            Ok(Ok(r)) => {
                yield Ok::<Event, std::convert::Infallible>(
                    Event::default()
                        .event("token")
                        .data(serde_json::to_string(&r.answer).unwrap_or_default()),
                );
                yield Ok(
                    Event::default()
                        .event("sources")
                        .data(serde_json::to_string(&r.sources).unwrap_or_default()),
                );
                yield Ok(Event::default().event("done").data("{}"));
            }
            Ok(Err(e)) => {
                yield Ok(
                    Event::default()
                        .event("error")
                        .data(
                            serde_json::json!({"message": e.to_string()}).to_string()
                        ),
                );
            }
            Err(e) => {
                yield Ok(
                    Event::default()
                        .event("error")
                        .data(
                            serde_json::json!({"message": e.to_string()}).to_string()
                        ),
                );
            }
        }
    };

    Sse::new(s).keep_alive(KeepAlive::default())
}
