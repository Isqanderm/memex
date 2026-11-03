//! Background worker that drains `memory_extraction_jobs` (queued by `observe`).

use std::sync::Arc;

use tracing::{error, info};

use crate::db::pool::DbPool;
use crate::db::repositories::memory_jobs::MemoryJobRepository;

use super::service::MemorySvc;

pub struct MemoryExtractionWorker {
    pub pool: Arc<DbPool>,
    pub svc: Arc<MemorySvc>,
}

impl MemoryExtractionWorker {
    /// Run the worker loop until a shutdown signal is received; polls every 500 ms.
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("MemoryExtractionWorker: shutdown signal received, stopping.");
                        break;
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                    if let Err(e) = self.process_one().await {
                        error!("MemoryExtractionWorker: error processing job: {e:#}");
                    }
                }
            }
        }
    }

    /// Claim and process one pending extraction job.
    ///
    /// Returns `Ok(true)` if a job was processed, `Ok(false)` if there was nothing to do.
    pub(crate) async fn process_one(&self) -> anyhow::Result<bool> {
        let pool = Arc::clone(&self.pool);
        let svc = Arc::clone(&self.svc);

        tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
            let conn = pool.get().map_err(|e| anyhow::anyhow!("DB pool error: {e}"))?;
            let jobs = MemoryJobRepository::new(&conn);
            let Some(job) = jobs.claim_next()? else {
                return Ok(false);
            };
            info!("MemoryExtractionWorker: processing job {} source_ref={}", job.id, job.source_ref);

            let result = tokio::runtime::Handle::current().block_on(svc.remember_from_source(
                &conn,
                &job.source_ref,
                &job.source,
                &job.scope,
            ));
            match result {
                Ok(r) => {
                    jobs.mark_done(&job.id, r.facts_extracted)?;
                    info!(
                        "MemoryExtractionWorker: job {} done, facts_extracted={}",
                        job.id, r.facts_extracted
                    );
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    error!("MemoryExtractionWorker: job {} failed: {msg}", job.id);
                    jobs.mark_error(&job.id, &msg)?;
                }
            }
            Ok(true)
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use crate::db::repositories::memories::MemoryRepository;
    use crate::db::repositories::memory_jobs::MemoryJobRepository;
    use crate::ingestion::embeddings::{EmbeddingClient, DEFAULT_MODEL};
    use crate::llm::test_support::ScriptedLlm;
    use crate::memory::extractor::FactExtractor;
    use crate::memory::service::MemorySvc;
    use crate::search::tantivy_fts::TantivyStore;
    use crate::search::vectors::VectorStore;

    fn model_cache_dir() -> std::path::PathBuf {
        match std::env::var("MODEL_CACHE_DIR") {
            Ok(dir) => dir.into(),
            Err(_) => std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                .join(".cache/fastembed"),
        }
    }

    /// Needs the embedding model (~120 MB download on first run).
    /// Run with: cargo test --lib -- --ignored memory::extraction_worker
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore]
    async fn worker_processes_job_and_marks_done() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let pool = Arc::new(build_pool(db.to_str().unwrap()).unwrap());
        let fts = Arc::new(
            TantivyStore::open(dir.path().join("memories").to_str().unwrap()).unwrap(),
        );
        let embed = Arc::new(EmbeddingClient::new(DEFAULT_MODEL, &model_cache_dir()).unwrap());
        let llm = Arc::new(ScriptedLlm::new(vec![
            r#"{"facts":[{"content":"User lives in Novi Sad","category":"preference"}]}"#,
        ]));
        let svc = Arc::new(MemorySvc::new(
            Arc::new(FactExtractor::new(llm.clone())),
            embed,
            Arc::new(VectorStore::new(384)),
            fts,
            0.82,
        ));

        let (job_id, created) = {
            let conn = pool.get().unwrap();
            svc.enqueue_observe(&conn, "User: I moved to Novi Sad.", "default")
                .unwrap()
        };
        assert!(created);

        let worker = MemoryExtractionWorker { pool: pool.clone(), svc: svc.clone() };
        assert!(worker.process_one().await.unwrap());
        assert!(!worker.process_one().await.unwrap(), "queue should be empty");

        let conn = pool.get().unwrap();
        let job = MemoryJobRepository::new(&conn).get_by_id(&job_id).unwrap().unwrap();
        assert_eq!(job.status, "done");
        assert_eq!(job.facts_extracted, 1);
        assert_eq!(MemoryRepository::new(&conn).get_all_active(None).unwrap().len(), 1);

        // The same conversation after the job finished maps to the same job.
        let (again, created) = svc
            .enqueue_observe(&conn, "User: I moved to Novi Sad.", "default")
            .unwrap();
        assert_eq!(again, job_id);
        assert!(!created);
    }
}
