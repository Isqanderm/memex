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
    pub async fn run(&self, shutdown: tokio::sync::watch::Receiver<bool>) {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(self.interval_secs)).await;

            // Check for shutdown signal
            if *shutdown.borrow() {
                info!("MemoryExpiryWorker: shutdown signal received, stopping.");
                break;
            }

            // Run expire_stale via spawn_blocking to avoid blocking the async runtime
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

            // Check shutdown again after work
            if shutdown.has_changed().unwrap_or(false) && *shutdown.borrow() {
                info!("MemoryExpiryWorker: shutdown signal received after work, stopping.");
                break;
            }
        }
    }
}
