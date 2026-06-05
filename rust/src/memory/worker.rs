use std::sync::Arc;

use tracing::{error, info};

use crate::db::pool::DbPool;

use super::service::MemorySvc;

pub struct MemoryExpiryWorker {
    pub pool: Arc<DbPool>,
    pub svc: Arc<MemorySvc>,
    pub interval_secs: u64,
}

impl MemoryExpiryWorker {
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        loop {
            // Wake up either when the interval fires OR when shutdown is signalled —
            // whichever comes first. Without select!, a 1-hour sleep would ignore SIGTERM.
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(self.interval_secs)) => {}
                _ = shutdown.changed() => {
                    info!("MemoryExpiryWorker: shutdown signal received, stopping.");
                    break;
                }
            }

            if *shutdown.borrow() {
                info!("MemoryExpiryWorker: shutdown signal received, stopping.");
                break;
            }

            let pool = Arc::clone(&self.pool);
            let svc = Arc::clone(&self.svc);

            let result = tokio::task::spawn_blocking(move || {
                let conn = pool.get().map_err(|e| anyhow::anyhow!("pool get: {e}"))?;
                svc.expire_stale(&conn)
            })
            .await;

            match result {
                Ok(Ok(n)) => {
                    if n > 0 {
                        info!("MemoryExpiryWorker: expired {n} stale memories.");
                    }
                }
                Ok(Err(e)) => {
                    error!("MemoryExpiryWorker: expire_stale failed: {e}");
                }
                Err(e) => {
                    error!("MemoryExpiryWorker: spawn_blocking panicked: {e}");
                }
            }
        }
    }
}
