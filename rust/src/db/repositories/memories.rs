use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub raw_input: String,
    pub source: String,
    pub is_active: bool,
    pub forget_after: Option<String>,
    pub relation: Option<String>,
    pub parent_id: Option<String>,
    pub category: Option<String>,
    pub project: Option<String>,
    pub created_at: String,
}

pub struct MemoryRepository<'a> {
    conn: &'a Connection,
}

impl<'a> MemoryRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn create(
        &self,
        content: &str,
        raw_input: &str,
        source: &str,
        forget_after: Option<&str>,
        relation: Option<&str>,
        parent_id: Option<&str>,
        category: Option<&str>,
        project: Option<&str>,
    ) -> rusqlite::Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO memories
                 (id, content, raw_input, source, forget_after, relation, parent_id, category, project)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                content,
                raw_input,
                source,
                forget_after,
                relation,
                parent_id,
                category,
                project
            ],
        )?;
        Ok(id)
    }

    pub fn deactivate(&self, id: &str) -> rusqlite::Result<bool> {
        let n = self.conn.execute(
            "UPDATE memories SET is_active = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(n > 0)
    }

    pub fn get_by_id(&self, id: &str) -> rusqlite::Result<Option<Memory>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, raw_input, source, is_active, forget_after,
                    relation, parent_id, category, project, created_at
             FROM memories WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_memory(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_all_active(&self) -> rusqlite::Result<Vec<Memory>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, raw_input, source, is_active, forget_after,
                    relation, parent_id, category, project, created_at
             FROM memories
             WHERE is_active = 1
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| Self::row_to_memory(row))?;
        rows.collect()
    }

    /// Deactivate all memories whose `forget_after` timestamp is in the past.
    /// Returns the number of affected rows.
    pub fn expire_stale(&self) -> rusqlite::Result<usize> {
        let n = self.conn.execute(
            "UPDATE memories
             SET is_active = 0
             WHERE is_active = 1
               AND forget_after IS NOT NULL
               AND forget_after < datetime('now')",
            [],
        )?;
        Ok(n)
    }

    fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
        let is_active_int: i64 = row.get(4)?;
        Ok(Memory {
            id: row.get(0)?,
            content: row.get(1)?,
            raw_input: row.get(2)?,
            source: row.get(3)?,
            is_active: is_active_int != 0,
            forget_after: row.get(5)?,
            relation: row.get(6)?,
            parent_id: row.get(7)?,
            category: row.get(8)?,
            project: row.get(9)?,
            created_at: row.get(10)?,
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
    fn create_and_get_active() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);

        let id = repo
            .create(
                "Remember this",
                "raw text",
                "chat",
                None,
                None,
                None,
                Some("general"),
                None,
            )
            .unwrap();

        let active = repo.get_all_active().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id);
        assert!(active[0].is_active);
    }

    #[test]
    fn deactivate_removes_from_active() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);

        let id = repo
            .create("Remember this", "raw", "chat", None, None, None, None, None)
            .unwrap();

        let ok = repo.deactivate(&id).unwrap();
        assert!(ok);

        let active = repo.get_all_active().unwrap();
        assert!(active.is_empty(), "no active memories after deactivation");

        let mem = repo.get_by_id(&id).unwrap().expect("should still exist");
        assert!(!mem.is_active);
    }
}
