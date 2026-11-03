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
    ///
    /// Jobs left in `processing` by a previous run (restart mid-extraction) are first
    /// returned to `pending`; this is safe because there is a single extraction worker.
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        match self.reset_stale().await {
            Ok(0) => {}
            Ok(n) => info!("MemoryExtractionWorker: re-queued {n} interrupted job(s)."),
            Err(e) => error!("MemoryExtractionWorker: failed to re-queue interrupted jobs: {e:#}"),
        }
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

    async fn reset_stale(&self) -> anyhow::Result<usize> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
            let conn = pool.get()?;
            Ok(MemoryJobRepository::new(&conn).reset_stale_processing()?)
        })
        .await?
    }

    /// Claim and process one pending extraction job.
    ///
    /// Returns `Ok(true)` if a job was processed, `Ok(false)` if there was nothing to do.
    /// A claimed job always gets a final status write attempt: extraction errors and
    /// panics become `error`, and a failed `mark_done` falls back to `mark_error`.
    pub(crate) async fn process_one(&self) -> anyhow::Result<bool> {
        let pool = Arc::clone(&self.pool);
        let job = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let conn = pool.get()?;
            Ok(MemoryJobRepository::new(&conn).claim_next()?)
        })
        .await??;
        let Some(job) = job else {
            return Ok(false);
        };
        info!("MemoryExtractionWorker: processing job {} source_ref={}", job.id, job.source_ref);

        let pool = Arc::clone(&self.pool);
        let svc = Arc::clone(&self.svc);
        let (source_ref, source, scope) = (job.source_ref, job.source, job.scope);
        let extracted = tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
            let conn = pool.get()?;
            let r = tokio::runtime::Handle::current()
                .block_on(svc.remember_from_source(&conn, &source_ref, &source, &scope))?;
            Ok(r.facts_extracted)
        })
        .await;
        let outcome = match extracted {
            Ok(Ok(n)) => Ok(n),
            Ok(Err(e)) => Err(format!("{e:#}")),
            Err(e) => Err(format!("extraction panicked: {e}")),
        };

        let pool = Arc::clone(&self.pool);
        let job_id = job.id;
        tokio::task::spawn_blocking(move || finish(&pool, &job_id, outcome)).await?;
        Ok(true)
    }
}

/// Write the final status of a claimed job; failures are logged, never propagated.
fn finish(pool: &DbPool, job_id: &str, outcome: Result<usize, String>) {
    let conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            error!("MemoryExtractionWorker: job {job_id}: no DB connection for the status write: {e}");
            return;
        }
    };
    let jobs = MemoryJobRepository::new(&conn);
    let msg = match outcome {
        Ok(n) => match jobs.mark_done(job_id, n) {
            Ok(_) => {
                info!("MemoryExtractionWorker: job {job_id} done, facts_extracted={n}");
                return;
            }
            Err(e) => {
                error!("MemoryExtractionWorker: job {job_id}: mark_done failed: {e}");
                format!("mark_done failed: {e}")
            }
        },
        Err(msg) => {
            error!("MemoryExtractionWorker: job {job_id} failed: {msg}");
            msg
        }
    };
    if let Err(e) = jobs.mark_error(job_id, &msg) {
        error!("MemoryExtractionWorker: job {job_id}: mark_error failed: {e}");
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
