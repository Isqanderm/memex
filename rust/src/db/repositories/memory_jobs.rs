//! Queue of background fact-extraction jobs (`memory_extraction_jobs`).
//!
//! `source_ref` is the id of the `memory_sources` row whose text the job extracts from.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

#[derive(Debug)]
pub struct MemoryJob {
    pub id: String,
    pub source_ref: String,
    pub source: String,
    pub scope: String,
    pub status: String,
    pub facts_extracted: i64,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str =
    "id, source_ref, source, scope, status, facts_extracted, error, created_at, updated_at";

pub struct MemoryJobRepository<'a> {
    conn: &'a Connection,
}

impl<'a> MemoryJobRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn create(&self, source_ref: &str, source: &str, scope: &str) -> rusqlite::Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO memory_extraction_jobs (id, source_ref, source, scope)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, source_ref, source, scope],
        )?;
        Ok(id)
    }

    /// Atomically claim the oldest pending job, transitioning it to 'processing'.
    pub fn claim_next(&self) -> rusqlite::Result<Option<MemoryJob>> {
        self.conn
            .query_row(
                &format!(
                    "UPDATE memory_extraction_jobs
                     SET status = 'processing', updated_at = datetime('now')
                     WHERE id = (
                         SELECT id FROM memory_extraction_jobs
                         WHERE status = 'pending'
                         ORDER BY created_at ASC, rowid ASC
                         LIMIT 1
                     )
                     RETURNING {COLUMNS}"
                ),
                [],
                Self::row_to_job,
            )
            .optional()
    }

    pub fn get_by_id(&self, id: &str) -> rusqlite::Result<Option<MemoryJob>> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM memory_extraction_jobs WHERE id = ?1"),
                params![id],
                Self::row_to_job,
            )
            .optional()
    }

    /// The newest pending or processing job for a source, if any.
    pub fn get_active_by_source_ref(&self, source_ref: &str) -> rusqlite::Result<Option<MemoryJob>> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM memory_extraction_jobs
                     WHERE source_ref = ?1 AND status IN ('pending', 'processing')
                     ORDER BY created_at DESC, rowid DESC LIMIT 1"
                ),
                params![source_ref],
                Self::row_to_job,
            )
            .optional()
    }

    /// The newest job for a source in any status, if any.
    pub fn get_latest_by_source_ref(&self, source_ref: &str) -> rusqlite::Result<Option<MemoryJob>> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM memory_extraction_jobs
                     WHERE source_ref = ?1
                     ORDER BY created_at DESC, rowid DESC LIMIT 1"
                ),
                params![source_ref],
                Self::row_to_job,
            )
            .optional()
    }

    pub fn mark_done(&self, id: &str, facts_extracted: usize) -> rusqlite::Result<bool> {
        let n = self.conn.execute(
            "UPDATE memory_extraction_jobs
             SET status = 'done', facts_extracted = ?2, error = NULL, updated_at = datetime('now')
             WHERE id = ?1",
            params![id, facts_extracted as i64],
        )?;
        Ok(n > 0)
    }

    pub fn mark_error(&self, id: &str, msg: &str) -> rusqlite::Result<bool> {
        let n = self.conn.execute(
            "UPDATE memory_extraction_jobs
             SET status = 'error', error = ?2, updated_at = datetime('now')
             WHERE id = ?1",
            params![id, msg],
        )?;
        Ok(n > 0)
    }

    fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryJob> {
        Ok(MemoryJob {
            id: row.get(0)?,
            source_ref: row.get(1)?,
            source: row.get(2)?,
            scope: row.get(3)?,
            status: row.get(4)?,
            facts_extracted: row.get(5)?,
            error: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;

    #[test]
    fn claim_mark_done_and_lookup_by_source() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryJobRepository::new(&conn);

        let id = repo.create("src-1", "conversation", "work").unwrap();
        assert_eq!(repo.get_active_by_source_ref("src-1").unwrap().unwrap().id, id);

        let job = repo.claim_next().unwrap().expect("pending job");
        assert_eq!(job.id, id);
        assert_eq!(job.status, "processing");
        assert_eq!(job.scope, "work");
        assert!(repo.claim_next().unwrap().is_none());

        assert!(repo.mark_done(&id, 3).unwrap());
        let job = repo.get_by_id(&id).unwrap().unwrap();
        assert_eq!(job.status, "done");
        assert_eq!(job.facts_extracted, 3);
        assert!(repo.get_active_by_source_ref("src-1").unwrap().is_none());
        assert_eq!(repo.get_latest_by_source_ref("src-1").unwrap().unwrap().id, id);
        assert!(repo.get_latest_by_source_ref("other").unwrap().is_none());
    }

    #[test]
    fn mark_error_records_message() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryJobRepository::new(&conn);
        let id = repo.create("src-1", "conversation", "default").unwrap();
        assert!(repo.mark_error(&id, "boom").unwrap());
        let job = repo.get_by_id(&id).unwrap().unwrap();
        assert_eq!(job.status, "error");
        assert_eq!(job.error.as_deref(), Some("boom"));
    }
}
