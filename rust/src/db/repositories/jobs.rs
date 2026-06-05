use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Processing,
    Done,
    Error,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Pending => "pending",
            JobStatus::Processing => "processing",
            JobStatus::Done => "done",
            JobStatus::Error => "error",
        }
    }
}

impl TryFrom<String> for JobStatus {
    type Error = rusqlite::Error;

    fn try_from(s: String) -> rusqlite::Result<Self> {
        match s.as_str() {
            "pending" => Ok(JobStatus::Pending),
            "processing" => Ok(JobStatus::Processing),
            "done" => Ok(JobStatus::Done),
            "error" => Ok(JobStatus::Error),
            other => Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown job status: {other}"),
                )),
            )),
        }
    }
}

#[derive(Debug)]
pub struct IngestionJob {
    pub id: String,
    pub status: String,
    pub source: String,
    pub checksum: String,
    pub doc_id: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct JobRepository<'a> {
    conn: &'a Connection,
}

impl<'a> JobRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn create(&self, source: &str, checksum: &str) -> rusqlite::Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO ingestion_jobs (id, source, checksum) VALUES (?1, ?2, ?3)",
            params![id, source, checksum],
        )?;
        Ok(id)
    }

    /// Atomically claim the oldest pending job, transitioning it to 'processing'.
    /// Returns None when no pending jobs exist.
    /// Uses RETURNING clause for atomic single-statement operation (SQLite 3.35+).
    pub fn claim_next(&self) -> rusqlite::Result<Option<IngestionJob>> {
        let mut stmt = self.conn.prepare(
            "UPDATE ingestion_jobs
             SET status = 'processing', updated_at = datetime('now')
             WHERE id = (
                 SELECT id FROM ingestion_jobs
                 WHERE status = 'pending'
                 ORDER BY created_at ASC
                 LIMIT 1
             )
             RETURNING id, status, source, checksum, doc_id, error, created_at, updated_at",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_job(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_by_id(&self, id: &str) -> rusqlite::Result<Option<IngestionJob>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, status, source, checksum, doc_id, error, created_at, updated_at
             FROM ingestion_jobs WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_job(row)?))
        } else {
            Ok(None)
        }
    }

    /// Returns an active (pending or processing) job matching the checksum,
    /// or None if no such job exists.
    pub fn get_by_checksum_active(
        &self,
        checksum: &str,
    ) -> rusqlite::Result<Option<IngestionJob>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, status, source, checksum, doc_id, error, created_at, updated_at
             FROM ingestion_jobs
             WHERE checksum = ?1 AND status IN ('pending', 'processing')
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![checksum])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_job(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn mark_done(&self, id: &str, doc_id: &str) -> rusqlite::Result<bool> {
        let n = self.conn.execute(
            "UPDATE ingestion_jobs
             SET status = 'done', doc_id = ?2, updated_at = datetime('now')
             WHERE id = ?1",
            params![id, doc_id],
        )?;
        Ok(n > 0)
    }

    pub fn mark_error(&self, id: &str, error_msg: &str) -> rusqlite::Result<bool> {
        let n = self.conn.execute(
            "UPDATE ingestion_jobs
             SET status = 'error', error = ?2, updated_at = datetime('now')
             WHERE id = ?1",
            params![id, error_msg],
        )?;
        Ok(n > 0)
    }

    pub fn list_active(&self) -> rusqlite::Result<Vec<IngestionJob>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, status, source, checksum, doc_id, error, created_at, updated_at
             FROM ingestion_jobs
             WHERE status IN ('pending', 'processing')
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], Self::row_to_job)?;
        rows.collect()
    }

    fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<IngestionJob> {
        Ok(IngestionJob {
            id: row.get(0)?,
            status: row.get(1)?,
            source: row.get(2)?,
            checksum: row.get(3)?,
            doc_id: row.get(4)?,
            error: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, crate::db::pool::DbPool) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = build_pool(path.to_str().unwrap()).unwrap();
        (dir, pool)
    }

    #[test]
    fn claim_next_returns_pending_job() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();
        let repo = JobRepository::new(&conn);

        let id = repo.create("file://doc.txt", "checksum_abc").unwrap();
        let job = repo.claim_next().unwrap().expect("should claim a job");

        assert_eq!(job.id, id);
        assert_eq!(job.status, "processing");
    }

    #[test]
    fn mark_done_updates_status() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();

        // Create a real document first so the foreign key on doc_id is satisfied.
        let doc_id = {
            use crate::db::repositories::documents::DocumentRepository;
            let doc_repo = DocumentRepository::new(&conn);
            doc_repo
                .create("file://doc.txt", "text/plain", "chk_xyz", None, "{}")
                .unwrap()
        };

        let repo = JobRepository::new(&conn);
        let job_id = repo.create("file://doc.txt", "checksum_xyz").unwrap();
        // Claim it to move it to processing
        repo.claim_next().unwrap();

        let ok = repo.mark_done(&job_id, &doc_id).unwrap();
        assert!(ok);

        let job = repo.get_by_id(&job_id).unwrap().expect("should exist");
        assert_eq!(job.status, "done");
        assert_eq!(job.doc_id, Some(doc_id));
    }
}
