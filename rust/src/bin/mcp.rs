/// Memex MCP server binary — exposes 9 tools over the MCP stdio transport.
///
/// All log output goes to stderr; stdout is reserved for the MCP JSON-RPC protocol.
use std::sync::Arc;

use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use rmcp::handler::server::wrapper::Parameters;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use memex::config::Config;
use memex::db::pool::{DbPool, build_pool};
use memex::ingestion::adapters::build_default_registry;
use memex::ingestion::chunker::SmallToBigChunker;
use memex::ingestion::embeddings::EmbeddingClient;
use memex::ingestion::language::LanguageDetector;
use memex::ingestion::pipeline::IngestionPipeline;
use memex::ingestion::worker::IngestionWorker;
use memex::llm::create_llm_provider;
use memex::memory::extractor::FactExtractor;
use memex::memory::profile::ProfileService;
use memex::memory::service::MemorySvc;
use memex::memory::worker::MemoryExpiryWorker;
use memex::search::context::ContextBuilder;
use memex::search::memory_search::MemorySearch;
use memex::search::reranker::Reranker;
use memex::search::service::RetrievalService;
use memex::search::{TantivyStore, VectorStore};

// ── Tool parameter types ──────────────────────────────────────────────────────

/// Parameters for the `remember` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RememberParams {
    /// Text to remember (facts will be extracted automatically).
    pub content: String,
    /// Memory source label (default: "explicit").
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "explicit".to_string()
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

/// Parameters for the `index_file` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct IndexFileParams {
    /// Absolute path to the file on disk.
    pub path: String,
}

/// Parameters for the `check_indexing` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CheckIndexingParams {
    /// Job ID returned by `index_file`.
    pub job_id: String,
}

/// Parameters for the `forget` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ForgetParams {
    /// Memory ID to deactivate.
    pub memory_id: String,
}

// ── Server struct ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct MemexMcpServer {
    pool: Arc<DbPool>,
    memory_svc: Arc<MemorySvc>,
    retrieval_svc: Arc<RetrievalService>,
    profile_svc: Arc<ProfileService>,
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Self>,
}

// ── Tool implementations ──────────────────────────────────────────────────────

#[tool_router]
impl MemexMcpServer {
    /// Save text as a memory — extracts atomic facts, resolves conflicts with existing memories.
    #[tool(description = "Save text as a memory — extracts atomic facts, resolves conflicts with existing memories.")]
    pub async fn remember(&self, Parameters(p): Parameters<RememberParams>) -> String {
        let pool = Arc::clone(&self.pool);
        let memory_svc = Arc::clone(&self.memory_svc);
        let content = p.content.clone();
        let source = p.source.clone();

        // DB connection acquisition must happen in spawn_blocking because rusqlite is not Send
        let conn = match pool.get() {
            Ok(c) => c,
            Err(e) => return format!("Error getting DB connection: {e}"),
        };

        // Use block_in_place to run synchronous DB work without moving conn across threads
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                memory_svc.remember(&conn, &content, &source).await
            })
        }) {
            Ok(r) => format!(
                "Remembered. Facts extracted: {}, memories updated: {}",
                r.facts_extracted, r.memories_updated
            ),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// Search memories and documents using RAG pipeline, returning an LLM-synthesized answer.
    #[tool(description = "Search memories and documents using RAG pipeline. Returns an LLM-synthesized answer with sources.")]
    pub async fn recall(&self, Parameters(p): Parameters<RecallParams>) -> String {
        let pool = Arc::clone(&self.pool);
        let retrieval = Arc::clone(&self.retrieval_svc);
        let query = p.query.clone();
        let category = p.category.clone();

        match tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| anyhow::anyhow!("DB: {e}"))?;
            retrieval.query(&conn, &query, category.as_deref())
        })
        .await
        {
            Ok(Ok(result)) => {
                let mut out = result.answer;
                if !result.sources.is_empty() {
                    let mut seen = std::collections::HashSet::new();
                    let mut refs = Vec::new();
                    for s in &result.sources {
                        let doc_id = s
                            .get("doc_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if seen.contains(&doc_id) {
                            continue;
                        }
                        seen.insert(doc_id);
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
                }
                out
            }
            Ok(Err(e)) => format!("Error: {e}"),
            Err(e) => format!("Task error: {e}"),
        }
    }

    /// Get user profile as static (stable facts) and dynamic (recent activity).
    /// Call as the FIRST tool at the start of every session.
    #[tool(description = "Get user profile as static (stable facts) and dynamic (recent activity). Call as the FIRST tool at the start of every session.")]
    pub async fn context(&self) -> String {
        let pool = Arc::clone(&self.pool);
        let profile_svc = Arc::clone(&self.profile_svc);

        let memories = match tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| anyhow::anyhow!("DB: {e}"))?;
            let repo = memex::db::repositories::memories::MemoryRepository::new(&conn);
            repo.get_all_active()
                .map_err(|e| anyhow::anyhow!("DB query: {e}"))
        })
        .await
        {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => return format!("Error loading memories: {e}"),
            Err(e) => return format!("Task error: {e}"),
        };

        if memories.is_empty() {
            return "No memories yet.".to_string();
        }

        match profile_svc.build_profile(&memories).await {
            Ok(profile) => {
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
                lines.join("\n")
            }
            Err(e) => format!("Error building profile: {e}"),
        }
    }

    /// Extract facts from a conversation and save to memory.
    /// Call as the LAST tool at the end of every session, passing the full conversation.
    #[tool(description = "Extract facts from a conversation and save to memory. Call as the LAST tool at the end of every session, passing the full conversation.")]
    pub async fn observe(&self, Parameters(p): Parameters<ObserveParams>) -> String {
        let pool = Arc::clone(&self.pool);
        let memory_svc = Arc::clone(&self.memory_svc);
        let conversation = p.conversation.clone();

        let conn = match pool.get() {
            Ok(c) => c,
            Err(e) => return format!("Error getting DB connection: {e}"),
        };

        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { memory_svc.observe(&conn, &conversation).await })
        }) {
            Ok(r) => format!(
                "Session observed. Facts extracted: {}, memories updated: {}",
                r.facts_extracted, r.memories_updated
            ),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// List all active memories with their content, source, and timestamps.
    #[tool(description = "List all active memories with their content, source, and timestamps.")]
    pub async fn memories(&self, Parameters(p): Parameters<MemoriesParams>) -> String {
        let pool = Arc::clone(&self.pool);
        let category = p.category.clone();

        match tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| anyhow::anyhow!("DB: {e}"))?;
            let repo = memex::db::repositories::memories::MemoryRepository::new(&conn);
            repo.get_all_active()
                .map_err(|e| anyhow::anyhow!("DB query: {e}"))
        })
        .await
        {
            Ok(Ok(all)) => {
                let filtered: Vec<_> = if let Some(cat) = &category {
                    all.into_iter()
                        .filter(|m| m.category.as_deref() == Some(cat.as_str()))
                        .collect()
                } else {
                    all
                };
                if filtered.is_empty() {
                    return "No active memories.".to_string();
                }
                let mut lines = Vec::new();
                for m in &filtered {
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
                    lines.push(format!(
                        "- {}{rel}\n  id: {}  |  {}{cat}{proj}  |  {date}",
                        m.content, m.id, m.source
                    ));
                }
                format!("Active memories: {}\n\n{}", filtered.len(), lines.join("\n\n"))
            }
            Ok(Err(e)) => format!("Error: {e}"),
            Err(e) => format!("Task error: {e}"),
        }
    }

    /// Index a file from disk (PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB).
    /// Indexing is asynchronous — use check_indexing(job_id) to poll for completion.
    #[tool(description = "Index a file from disk (PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB). Indexing is asynchronous — use check_indexing(job_id) to poll for completion.")]
    pub async fn index_file(&self, Parameters(p): Parameters<IndexFileParams>) -> String {
        use sha2::{Digest, Sha256};

        let path_str = p.path.clone();
        let path = std::path::PathBuf::from(&path_str);
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Read file contents to compute checksum
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => return format!("File not found or unreadable: {e}"),
        };

        // Compute SHA-256 checksum
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let checksum = hex::encode(hasher.finalize());

        let pool = Arc::clone(&self.pool);

        match tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| anyhow::anyhow!("DB: {e}"))?;
            let doc_repo = memex::db::repositories::documents::DocumentRepository::new(&conn);
            let job_repo = memex::db::repositories::jobs::JobRepository::new(&conn);

            // Check if already indexed
            if let Ok(Some(doc)) = doc_repo.get_by_checksum(&checksum) {
                return Ok(format!("Already indexed (doc_id: {})", doc.id));
            }

            // Check if a job is already active for this checksum
            if let Ok(Some(job)) = job_repo.get_by_checksum_active(&checksum) {
                return Ok(format!(
                    "Already queued (job_id: {}). Use check_indexing('{}') to poll status.",
                    job.id, job.id
                ));
            }

            // Create new ingestion job — store plain path (not URI) so the worker can use it directly
            let source = path_str.to_string();
            job_repo
                .create(&source, &checksum)
                .map(|job_id| format!(
                    "File accepted: {filename}\njob_id: {job_id}\nUse check_indexing('{job_id}') to poll for completion."
                ))
                .map_err(|e| anyhow::anyhow!("Error creating job: {e}"))
        })
        .await
        {
            Ok(Ok(msg)) => msg,
            Ok(Err(e)) => format!("Error: {e}"),
            Err(e) => format!("Task error: {e}"),
        }
    }

    /// Check the indexing status of a file. Returns pending/processing/done/error.
    #[tool(description = "Check the indexing status of a file. Returns pending/processing/done/error.")]
    pub async fn check_indexing(&self, Parameters(p): Parameters<CheckIndexingParams>) -> String {
        let pool = Arc::clone(&self.pool);
        let job_id = p.job_id.clone();

        match tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| anyhow::anyhow!("DB: {e}"))?;
            let repo = memex::db::repositories::jobs::JobRepository::new(&conn);
            repo.get_by_id(&job_id)
                .map_err(|e| anyhow::anyhow!("DB error: {e}"))
        })
        .await
        {
            Ok(Ok(Some(job))) => match job.status.as_str() {
                "done" => format!("Done. doc_id: {}", job.doc_id.as_deref().unwrap_or("?")),
                "error" => format!(
                    "Error: {}",
                    job.error.as_deref().unwrap_or("unknown error")
                ),
                status => format!("{status} — not ready yet, check again later."),
            },
            Ok(Ok(None)) => format!("Job not found: {}", p.job_id),
            Ok(Err(e)) => format!("Error: {e}"),
            Err(e) => format!("Task error: {e}"),
        }
    }

    /// List all indexed documents: id, title, mime type, date.
    #[tool(description = "List all indexed documents: id, title, mime type, date.")]
    pub async fn list_documents(&self) -> String {
        let pool = Arc::clone(&self.pool);

        match tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| anyhow::anyhow!("DB: {e}"))?;
            let repo = memex::db::repositories::documents::DocumentRepository::new(&conn);
            repo.list_all()
                .map_err(|e| anyhow::anyhow!("DB error: {e}"))
        })
        .await
        {
            Ok(Ok(docs)) if docs.is_empty() => "No documents indexed.".to_string(),
            Ok(Ok(docs)) => {
                let lines: Vec<String> = docs
                    .iter()
                    .map(|d| {
                        let title = d.title.as_deref().unwrap_or("—");
                        let date = &d.indexed_at[..d.indexed_at.len().min(10)];
                        format!("- {title}\n  id: {}  |  {}  |  {date}", d.id, d.mime_type)
                    })
                    .collect();
                format!("Documents: {}\n\n{}", docs.len(), lines.join("\n\n"))
            }
            Ok(Err(e)) => format!("Error: {e}"),
            Err(e) => format!("Task error: {e}"),
        }
    }

    /// Deactivate (soft-delete) a memory by its ID.
    #[tool(description = "Deactivate (soft-delete) a memory by its ID.")]
    pub async fn forget(&self, Parameters(p): Parameters<ForgetParams>) -> String {
        let pool = Arc::clone(&self.pool);
        let memory_svc = Arc::clone(&self.memory_svc);
        let memory_id = p.memory_id.clone();

        let conn = match pool.get() {
            Ok(c) => c,
            Err(e) => return format!("Error getting DB connection: {e}"),
        };

        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { memory_svc.forget(&conn, &memory_id).await })
        }) {
            Ok(true) => format!("Memory deleted (id: {memory_id})"),
            Ok(false) => format!("Memory not found: {memory_id}"),
            Err(e) => format!("Error: {e}"),
        }
    }
}

#[tool_handler(
    name = "memex-mcp",
    version = "3.0.0",
    instructions = "Memex persistent memory MCP server. Call context() first at session start, observe() last at session end."
)]
impl ServerHandler for MemexMcpServer {}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logs go to stderr so they don't interfere with MCP JSON on stdout
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "memex_mcp=info,memex=info,warn".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    info!("Starting memex-mcp");

    let config = Config::from_env().map_err(|e| anyhow::anyhow!("config error: {e}"))?;

    std::fs::create_dir_all(
        std::path::Path::new(&config.database_path)
            .parent()
            .unwrap_or(std::path::Path::new(".")),
    )?;

    let pool = Arc::new(build_pool(&config.database_path)?);
    let tantivy = Arc::new(TantivyStore::open(&config.tantivy_path)?);
    let vectors = Arc::new(VectorStore::new(config.embedding_dimensions));

    info!(
        "Loading embedding model {}...",
        config.local_embedding_model
    );
    let embed = Arc::new(EmbeddingClient::new(&config.local_embedding_model)?);
    info!("Embedding model loaded ({} dims)", embed.dimensions());

    info!("Loading reranker model...");
    let reranker = Arc::new(Reranker::new()?);
    info!("Reranker loaded");

    let llm = create_llm_provider(&config)?;
    let extractor = Arc::new(FactExtractor::new(llm.clone()));
    let profile_svc = Arc::new(ProfileService::new(llm.clone()));
    let memory_svc = Arc::new(MemorySvc::new(
        extractor.clone(),
        embed.clone(),
        vectors.clone(),
    ));

    let retrieval_svc = Arc::new(RetrievalService {
        embed: embed.clone(),
        tantivy: tantivy.clone(),
        vectors: vectors.clone(),
        reranker,
        llm: llm.clone(),
        context_builder: ContextBuilder,
        memory_search: MemorySearch::new(),
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

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let ingestion_worker = Arc::new(IngestionWorker {
        pool: pool.clone(),
        pipeline: pipeline.clone(),
        tantivy: tantivy.clone(),
        vectors: vectors.clone(),
    });
    let expiry_worker = Arc::new(MemoryExpiryWorker {
        pool: pool.clone(),
        svc: memory_svc.clone(),
        interval_secs: 3600,
    });

    let w1 = ingestion_worker.clone();
    let w2 = expiry_worker.clone();
    let rx1 = shutdown_rx.clone();
    let rx2 = shutdown_rx.clone();

    tokio::spawn(async move { w1.run(rx1).await });
    tokio::spawn(async move { w2.run(rx2).await });

    let server = MemexMcpServer {
        pool,
        memory_svc,
        retrieval_svc,
        profile_svc,
        tool_router: MemexMcpServer::tool_router(),
    };

    info!("memex-mcp ready, serving on stdio");

    let (stdin, stdout) = rmcp::transport::io::stdio();
    server.serve((stdin, stdout)).await?.waiting().await?;

    let _ = shutdown_tx.send(true);
    Ok(())
}
