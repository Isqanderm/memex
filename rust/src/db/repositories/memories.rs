use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub source: String,
    pub is_active: bool,
    pub forget_after: Option<String>,
    pub relation: Option<String>,
    pub parent_id: Option<String>,
    pub category: Option<String>,
    pub project: Option<String>,
    pub created_at: String,
    pub scope: String,
    pub source_id: Option<String>,
    pub updated_at: String,
    pub last_accessed_at: Option<String>,
    pub access_count: i64,
}

pub struct NewMemory<'a> {
    pub content: &'a str,
    pub source: &'a str,
    pub source_id: Option<&'a str>,
    pub scope: &'a str,
    pub forget_after: Option<&'a str>,
    pub relation: Option<&'a str>,
    pub parent_id: Option<&'a str>,
    pub category: Option<&'a str>,
    pub project: Option<&'a str>,
}

const COLS: &str = "id, content, source, is_active, forget_after, relation, parent_id,
                    category, project, created_at, scope, source_id,
                    COALESCE(updated_at, created_at), last_accessed_at, access_count";

pub struct MemoryRepository<'a> {
    conn: &'a Connection,
}

impl<'a> MemoryRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn create(&self, m: &NewMemory) -> rusqlite::Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO memories
                 (id, content, source, source_id, scope, forget_after, relation, parent_id,
                  category, project, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'))",
            params![
                id,
                m.content,
                m.source,
                m.source_id,
                m.scope,
                m.forget_after,
                m.relation,
                m.parent_id,
                m.category,
                m.project
            ],
        )?;
        Ok(id)
    }

    pub fn deactivate(&self, id: &str) -> rusqlite::Result<bool> {
        let n = self.conn.execute(
            "UPDATE memories SET is_active = 0, updated_at = datetime('now') WHERE id = ?1",
            params![id],
        )?;
        Ok(n > 0)
    }

    pub fn get_by_id(&self, id: &str) -> rusqlite::Result<Option<Memory>> {
        let mut stmt = self.conn.prepare(
            &format!("SELECT {COLS} FROM memories WHERE id = ?1"),
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_memory(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_all_active(&self, scope: Option<&str>) -> rusqlite::Result<Vec<Memory>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM memories
             WHERE is_active = 1 AND (?1 IS NULL OR scope = ?1)
             ORDER BY created_at DESC"
        ))?;
        let rows = stmt.query_map(params![scope], Self::row_to_memory)?;
        rows.collect()
    }

    pub fn touch_accessed(&self, ids: &[String]) -> rusqlite::Result<()> {
        let mut stmt = self.conn.prepare(
            "UPDATE memories
             SET access_count = access_count + 1, last_accessed_at = datetime('now')
             WHERE id = ?1",
        )?;
        for id in ids {
            stmt.execute(params![id])?;
        }
        Ok(())
    }

    /// `"{active_count}:{max_updated_at}"`; changes whenever the active set changes.
    pub fn fingerprint(&self) -> rusqlite::Result<String> {
        let (count, max): (i64, Option<String>) = self.conn.query_row(
            "SELECT COUNT(*), MAX(COALESCE(updated_at, created_at)) FROM memories WHERE is_active = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(format!("{count}:{}", max.unwrap_or_default()))
    }

    /// Deactivate all memories whose `forget_after` timestamp is in the past.
    /// Returns the number of affected rows.
    pub fn expire_stale(&self) -> rusqlite::Result<usize> {
        let n = self.conn.execute(
            "UPDATE memories
             SET is_active = 0, updated_at = datetime('now')
             WHERE is_active = 1
               AND forget_after IS NOT NULL
               AND forget_after < datetime('now')",
            [],
        )?;
        Ok(n)
    }

    fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
        let is_active_int: i64 = row.get(3)?;
        Ok(Memory {
            id: row.get(0)?,
            content: row.get(1)?,
            source: row.get(2)?,
            is_active: is_active_int != 0,
            forget_after: row.get(4)?,
            relation: row.get(5)?,
            parent_id: row.get(6)?,
            category: row.get(7)?,
            project: row.get(8)?,
            created_at: row.get(9)?,
            scope: row.get(10)?,
            source_id: row.get(11)?,
            updated_at: row.get(12)?,
            last_accessed_at: row.get(13)?,
            access_count: row.get(14)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;

    fn nm<'a>(content: &'a str, scope: &'a str) -> NewMemory<'a> {
        NewMemory {
            content,
            source: "explicit",
            source_id: None,
            scope,
            forget_after: None,
            relation: None,
            parent_id: None,
            category: None,
            project: None,
        }
    }

    #[test]
    fn create_and_list_by_scope() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);
        repo.create(&nm("fact a", "a")).unwrap();
        repo.create(&nm("fact b", "b")).unwrap();
        assert_eq!(repo.get_all_active(Some("a")).unwrap().len(), 1);
        assert_eq!(repo.get_all_active(None).unwrap().len(), 2);
    }

    #[test]
    fn touch_accessed_increments_and_sets_timestamp() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);
        let id = repo.create(&nm("fact", "default")).unwrap();
        assert_eq!(repo.get_by_id(&id).unwrap().unwrap().access_count, 0);
        repo.touch_accessed(std::slice::from_ref(&id)).unwrap();
        repo.touch_accessed(std::slice::from_ref(&id)).unwrap();
        let m = repo.get_by_id(&id).unwrap().unwrap();
        assert_eq!(m.access_count, 2);
        assert!(m.last_accessed_at.is_some());
    }

    #[test]
    fn deactivate_bumps_updated_at_and_fingerprint_changes() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);
        assert_eq!(repo.fingerprint().unwrap(), "0:");
        let id = repo.create(&nm("fact", "default")).unwrap();
        repo.create(&nm("other", "default")).unwrap();
        let fp1 = repo.fingerprint().unwrap();
        assert!(repo.deactivate(&id).unwrap());
        let fp2 = repo.fingerprint().unwrap();
        assert_ne!(fp1, fp2);
        assert!(fp2.starts_with("1:"));
    }

    #[test]
    fn deactivate_removes_from_active() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);
        let id = repo.create(&nm("fact", "default")).unwrap();
        assert!(repo.deactivate(&id).unwrap());
        assert!(repo.get_all_active(None).unwrap().is_empty());
        assert!(!repo.get_by_id(&id).unwrap().unwrap().is_active);
    }

    #[test]
    fn expire_stale_deactivates_past_memories() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);
        let mut past = nm("Past fact", "default");
        past.forget_after = Some("2020-01-01T00:00:00");
        let id = repo.create(&past).unwrap();
        let mut fut = nm("Future fact", "default");
        fut.forget_after = Some("2099-01-01T00:00:00");
        let id_future = repo.create(&fut).unwrap();

        assert_eq!(repo.expire_stale().unwrap(), 1);
        assert!(!repo.get_by_id(&id).unwrap().unwrap().is_active);
        assert!(repo.get_by_id(&id_future).unwrap().unwrap().is_active);
    }
}
