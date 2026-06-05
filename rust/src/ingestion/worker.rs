use std::sync::Arc;

use tracing::{error, info, warn};

use crate::db::pool::DbPool;
use crate::db::repositories::jobs::JobRepository;
use crate::ingestion::pipeline::{file_checksum, IngestionPipeline};
use crate::search::tantivy_fts::TantivyStore;
use crate::search::vectors::VectorStore;

pub struct IngestionWorker {
    pub pool: Arc<DbPool>,
    pub pipeline: Arc<IngestionPipeline>,
    pub tantivy: Arc<TantivyStore>,
    pub vectors: Arc<VectorStore>,
}

impl IngestionWorker {
    /// Run the worker loop until a shutdown signal is received.
    ///
    /// The loop wakes every 500 ms to check for pending jobs.
    /// On shutdown the loop exits cleanly after the current job (if any) finishes.
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        loop {
            tokio::select! {
                // Shutdown signal received
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("IngestionWorker: shutdown signal received, stopping.");
                        break;
                    }
                }
                // Poll for work every 500 ms
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                    match self.process_one().await {
                        Ok(true) => {
                            // Processed a job; immediately try again in case there are more.
                        }
                        Ok(false) => {
                            // Nothing to do; will sleep again.
                        }
                        Err(e) => {
                            error!("IngestionWorker: error processing job: {e:#}");
                        }
                    }
                }
            }
        }
    }

    /// Claim and process one pending ingestion job.
    ///
    /// Returns `Ok(true)` if a job was processed, `Ok(false)` if there was nothing to do.
    pub(crate) async fn process_one(&self) -> anyhow::Result<bool> {
        let pool = Arc::clone(&self.pool);
        let pipeline = Arc::clone(&self.pipeline);
        let tantivy = Arc::clone(&self.tantivy);
        let vectors = Arc::clone(&self.vectors);

        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
            let conn = pool.get().map_err(|e| anyhow::anyhow!("DB pool error: {e}"))?;
            let job_repo = JobRepository::new(&conn);

            // Atomically claim the oldest pending job.
            let job = match job_repo.claim_next()? {
                Some(j) => j,
                None => return Ok(false),
            };

            info!("IngestionWorker: processing job {} source={}", job.id, job.source);

            // Compute checksum (may have changed since the job was enqueued, so
            // we recompute; the pre-computed one from the job is used as the
            // deduplication key in the documents table).
            let checksum = match file_checksum(&job.source) {
                Ok(c) => c,
                Err(e) => {
                    warn!("IngestionWorker: checksum failed for {}: {e}", job.source);
                    job_repo.mark_error(&job.id, &e.to_string())?;
                    return Ok(true);
                }
            };

            // Run the full pipeline.
            match pipeline.process(&conn, &tantivy, &vectors, &job.source, &checksum) {
                Ok(doc_id) => {
                    job_repo.mark_done(&job.id, &doc_id)?;
                    info!("IngestionWorker: job {} done, doc_id={doc_id}", job.id);
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    error!("IngestionWorker: job {} failed: {msg}", job.id);
                    job_repo.mark_error(&job.id, &msg)?;
                }
            }

            Ok(true)
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))?;

        result
    }
}
