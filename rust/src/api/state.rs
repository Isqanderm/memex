use std::sync::Arc;

use crate::config::Config;
use crate::db::pool::DbPool;
use crate::ingestion::embeddings::EmbeddingClient;
use crate::memory::profile::ProfileService;
use crate::memory::service::MemorySvc;
use crate::search::service::RetrievalService;
use crate::search::tantivy_fts::TantivyStore;
use crate::search::vectors::VectorStore;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<DbPool>,
    pub config: Arc<Config>,
    pub tantivy: Arc<TantivyStore>,
    pub vectors: Arc<VectorStore>,
    pub embed: Arc<EmbeddingClient>,
    pub retrieval: Arc<RetrievalService>,
    pub memory_svc: Arc<MemorySvc>,
    pub profile_svc: Arc<ProfileService>,
}
