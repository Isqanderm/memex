//! MCP server over Streamable HTTP, mounted at `/mcp` in the main Axum app.
//!
//! Built from the shared [`AppState`], so it reuses the server's DB pool, indexes and
//! loaded models instead of opening its own.

pub mod router;

use std::collections::HashSet;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::api::documents::{delete_document_by_id, enqueue_upload};
use crate::api::state::AppState;
use crate::db::repositories::documents::DocumentRepository;
use crate::db::repositories::jobs::JobRepository;
use crate::db::repositories::memories::MemoryRepository;
use crate::error::AppError;

// ── Tool parameter types ──────────────────────────────────────────────────────

/// Parameters for the `remember` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RememberParams {
    /// Text to remember (facts will be extracted automatically).
    pub content: String,
    /// Memory source label (default: "explicit").
    pub source: Option<String>,
}

/// Parameters for the `recall` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RecallParams {
    /// Query text.
    pub query: String,
    /// Optional memory category filter.
    pub category: Option<String>,
}

/// Parameters for the `observe` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ObserveParams {
    /// Full conversation history as text.
    pub conversation: String,
}

/// Parameters for the `memories` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MemoriesParams {
    /// Optional category filter.
    pub category: Option<String>,
}

/// Parameters for the `index_text` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct IndexTextParams {
    /// Document text (Markdown or plain text).
    pub content: String,
    /// Document title; also used as the stored file name.
    pub title: String,
}

/// Parameters for the `check_indexing` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CheckIndexingParams {
    /// Job ID returned by `index_text`.
    pub job_id: String,
}

/// Parameters for the `forget` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ForgetParams {
    /// Memory ID to deactivate.
    pub memory_id: String,
}

/// Parameters for the `delete_document` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeleteDocumentParams {
    /// Document ID to delete.
    pub doc_id: String,
}

// ── Result helpers ────────────────────────────────────────────────────────────

fn ok_text(s: impl Into<String>) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![Content::text(s.into())]))
}

fn domain_error(s: impl Into<String>) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::error(vec![Content::text(s.into())]))
}

fn internal(e: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

/// Turns a document title into a safe single-segment file name (no path separators,
/// no control characters, no leading dots).
fn safe_file_stem(title: &str) -> String {
    let cleaned: String = title
        .trim()
        .chars()
        .map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "untitled".to_string()
    } else {
        cleaned
    }
}

// ── Server struct ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct MemexMcp {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

impl MemexMcp {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    /// Runs `f` on a pooled connection inside `spawn_blocking`.
    /// Async service calls inside `f` go through `Handle::current().block_on`.
    async fn with_conn<T, F>(&self, f: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, AppError> + Send + 'static,
    {
        let pool = self.state.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(AppError::Pool)?;
            f(&conn)
        })
        .await
        .map_err(|e| AppError::Llm(format!("task join error: {e}")))?
    }
}

// ── Tool implementations ──────────────────────────────────────────────────────

#[tool_router]
impl MemexMcp {
    #[tool(description = "Save text as a memory — extracts atomic facts, resolves conflicts with existing memories.")]
    pub async fn remember(
        &self,
        Parameters(p): Parameters<RememberParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let memory_svc = self.state.memory_svc.clone();
        let source = p.source.unwrap_or_else(|| "explicit".to_string());
        let r = self
            .with_conn(move |conn| {
                tokio::runtime::Handle::current()
                    .block_on(memory_svc.remember(conn, &p.content, &source))
                    .map_err(|e| AppError::Llm(e.to_string()))
            })
            .await
            .map_err(internal)?;
        ok_text(format!(
            "Remembered. Facts extracted: {}, memories updated: {}",
            r.facts_extracted, r.memories_updated
        ))
    }

    #[tool(description = "Search memories and documents using RAG pipeline. Returns an LLM-synthesized answer with sources.")]
    pub async fn recall(
        &self,
        Parameters(p): Parameters<RecallParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let retrieval = self.state.retrieval.clone();
        let result = self
            .with_conn(move |conn| {
                retrieval
                    .query(conn, &p.query, p.category.as_deref())
                    .map_err(|e| AppError::Llm(e.to_string()))
            })
            .await
            .map_err(internal)?;

        let mut out = result.answer;
        let mut seen = HashSet::new();
        let mut refs = Vec::new();
        for s in &result.sources {
            let doc_id = s.get("doc_id").and_then(|v| v.as_str()).unwrap_or("");
            if !seen.insert(doc_id.to_string()) {
                continue;
            }
            let title = s
                .get("title")
                .or_else(|| s.get("filename"))
                .and_then(|v| v.as_str())
                .unwrap_or("source");
            refs.push(format!("  - {title}"));
        }
        if !refs.is_empty() {
            out.push_str("\n\nSources:\n");
            out.push_str(&refs.join("\n"));
        }
        ok_text(out)
    }

    #[tool(description = "Deactivate (soft-delete) a memory by its ID.")]
    pub async fn forget(
        &self,
        Parameters(p): Parameters<ForgetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let memory_svc = self.state.memory_svc.clone();
        let id = p.memory_id.clone();
        let found = self
            .with_conn(move |conn| {
                tokio::runtime::Handle::current()
                    .block_on(memory_svc.forget(conn, &id))
                    .map_err(|e| AppError::Llm(e.to_string()))
            })
            .await
            .map_err(internal)?;
        if found {
            ok_text(format!("Memory {} deactivated", p.memory_id))
        } else {
            domain_error(format!("Memory {} not found", p.memory_id))
        }
    }

    #[tool(description = "Get user profile as static (stable facts) and dynamic (recent activity). Call as the FIRST tool at the start of every session.")]
    pub async fn context(&self) -> Result<CallToolResult, ErrorData> {
        let memories = self
            .with_conn(|conn| {
                MemoryRepository::new(conn)
                    .get_all_active()
                    .map_err(AppError::Db)
            })
            .await
            .map_err(internal)?;

        if memories.is_empty() {
            return ok_text("No memories yet.");
        }

        let profile = self
            .state
            .profile_svc
            .build_profile(&memories)
            .await
            .map_err(internal)?;

        let mut lines = Vec::new();
        if !profile.static_summary.is_empty() {
            lines.push(format!("User profile: {}", profile.static_summary));
        }
        if !profile.dynamic_summary.is_empty() {
            lines.push(format!("Recent context: {}", profile.dynamic_summary));
        }
        lines.push(format!("(Total memories: {})", profile.raw_count));
        if lines.len() == 1 {
            // Only the count line — no actual summaries
            lines.insert(0, "No memories yet.".to_string());
        }
        ok_text(lines.join("\n"))
    }

    #[tool(description = "Extract facts from a conversation and save to memory. Call as the LAST tool at the end of every session, passing the full conversation.")]
    pub async fn observe(
        &self,
        Parameters(p): Parameters<ObserveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let memory_svc = self.state.memory_svc.clone();
        let r = self
            .with_conn(move |conn| {
                tokio::runtime::Handle::current()
                    .block_on(memory_svc.observe(conn, &p.conversation))
                    .map_err(|e| AppError::Llm(e.to_string()))
            })
            .await
            .map_err(internal)?;
        ok_text(format!(
            "Session observed. Facts extracted: {}, memories updated: {}",
            r.facts_extracted, r.memories_updated
        ))
    }

    #[tool(description = "List all active memories with their content, source, and timestamps.")]
    pub async fn memories(
        &self,
        Parameters(p): Parameters<MemoriesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let all = self
            .with_conn(|conn| {
                MemoryRepository::new(conn)
                    .get_all_active()
                    .map_err(AppError::Db)
            })
            .await
            .map_err(internal)?;

        let filtered: Vec<_> = match &p.category {
            Some(cat) => all
                .into_iter()
                .filter(|m| m.category.as_deref() == Some(cat.as_str()))
                .collect(),
            None => all,
        };
        if filtered.is_empty() {
            return ok_text("No active memories.");
        }

        let lines: Vec<String> = filtered
            .iter()
            .map(|m| {
                let rel = m
                    .relation
                    .as_deref()
                    .map(|r| format!(" [{r}]"))
                    .unwrap_or_default();
                let cat = m
                    .category
                    .as_deref()
                    .map(|c| format!(" | {c}"))
                    .unwrap_or_default();
                let proj = m
                    .project
                    .as_deref()
                    .map(|p| format!(" | {p}"))
                    .unwrap_or_default();
                let date = &m.created_at[..m.created_at.len().min(10)];
                format!(
                    "- {}{rel}\n  id: {}  |  {}{cat}{proj}  |  {date}",
                    m.content, m.id, m.source
                )
            })
            .collect();
        ok_text(format!(
            "Active memories: {}\n\n{}",
            filtered.len(),
            lines.join("\n\n")
        ))
    }

    #[tool(description = "Index a text document (Markdown or plain text) given its content and title. Indexing is asynchronous — use check_indexing(job_id) to poll for completion.")]
    pub async fn index_text(
        &self,
        Parameters(p): Parameters<IndexTextParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let upload_dir = self.state.config.upload_dir.clone();
        let filename = format!("{}.md", safe_file_stem(&p.title));
        let resp = self
            .with_conn(move |conn| {
                enqueue_upload(conn, &upload_dir, &filename, p.content.as_bytes())
            })
            .await
            .map_err(internal)?;

        match (resp.status.as_str(), resp.job_id, resp.doc_id) {
            ("already_indexed", _, Some(doc_id)) => {
                ok_text(format!("Already indexed. doc_id: {doc_id}"))
            }
            (_, Some(job_id), _) => ok_text(format!("Queued. job_id: {job_id}")),
            (status, _, _) => Err(internal(format!("unexpected upload status: {status}"))),
        }
    }

    #[tool(description = "Check the indexing status of a document. Returns pending/processing/done/error.")]
    pub async fn check_indexing(
        &self,
        Parameters(p): Parameters<CheckIndexingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = p.job_id.clone();
        let job = self
            .with_conn(move |conn| JobRepository::new(conn).get_by_id(&id).map_err(AppError::Db))
            .await
            .map_err(internal)?;

        match job {
            None => domain_error(format!("Job not found: {}", p.job_id)),
            Some(job) => match job.status.as_str() {
                "done" => ok_text(format!(
                    "Done. doc_id: {}",
                    job.doc_id.as_deref().unwrap_or("?")
                )),
                "error" => ok_text(format!(
                    "Error: {}",
                    job.error.as_deref().unwrap_or("unknown error")
                )),
                status => ok_text(format!("{status} — not ready yet, check again later.")),
            },
        }
    }

    #[tool(description = "List all indexed documents: id, title, mime type, date.")]
    pub async fn list_documents(&self) -> Result<CallToolResult, ErrorData> {
        let docs = self
            .with_conn(|conn| DocumentRepository::new(conn).list_all().map_err(AppError::Db))
            .await
            .map_err(internal)?;

        if docs.is_empty() {
            return ok_text("No documents indexed.");
        }
        let lines: Vec<String> = docs
            .iter()
            .map(|d| {
                let title = d.title.as_deref().unwrap_or("—");
                let date = &d.indexed_at[..d.indexed_at.len().min(10)];
                format!("- {title}\n  id: {}  |  {}  |  {date}", d.id, d.mime_type)
            })
            .collect();
        ok_text(format!("Documents: {}\n\n{}", docs.len(), lines.join("\n\n")))
    }

    #[tool(description = "Delete an indexed document by its ID, removing it from the search indexes.")]
    pub async fn delete_document(
        &self,
        Parameters(p): Parameters<DeleteDocumentParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let tantivy = self.state.tantivy.clone();
        let vectors = self.state.vectors.clone();
        let id = p.doc_id.clone();
        let res = self
            .with_conn(move |conn| delete_document_by_id(conn, &tantivy, &vectors, &id))
            .await;
        match res {
            Ok(()) => ok_text(format!("Document {} deleted", p.doc_id)),
            Err(AppError::NotFound(_)) => domain_error(format!("Document {} not found", p.doc_id)),
            Err(e) => Err(internal(e)),
        }
    }
}

#[tool_handler(
    name = "memex",
    instructions = "Memex: persistent personal memory and document search. Call context first in a session, observe last."
)]
impl ServerHandler for MemexMcp {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_file_stem_strips_path_components() {
        assert_eq!(safe_file_stem("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_file_stem("notes\\x"), "notes_x");
        assert_eq!(safe_file_stem("  "), "untitled");
        assert_eq!(safe_file_stem("..."), "untitled");
        assert_eq!(safe_file_stem("My notes"), "My notes");
    }
}
