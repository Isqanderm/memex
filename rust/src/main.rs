#![allow(dead_code, unused_imports)]

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
use search::memory_search::MemorySearch;
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

    std::fs::create_dir_all(&config.upload_dir)?;
    std::fs::create_dir_all(
        std::path::Path::new(&config.database_path)
            .parent()
            .unwrap_or(std::path::Path::new(".")),
    )?;

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

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

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
