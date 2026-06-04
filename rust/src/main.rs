mod config;
mod error;

// Заглушки — заполняются в следующих задачах
mod db;
mod search;
mod ingestion;
mod llm;
mod memory;
mod api;

use axum::Router;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "memex=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::Config::from_env()
        .map_err(|e| anyhow::anyhow!("config error: {e}"))?;

    info!("Memex starting on {}:{}", config.host, config.port);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }));

    info!("Listening on {addr}");
    axum::serve(listener, app).await?;

    Ok(())
}
