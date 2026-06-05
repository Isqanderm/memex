use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug)]
pub struct Document {
    pub id: String,
    pub source: String,
    pub mime_type: String,
    pub title: Option<String>,
    pub checksum: String,
    pub metadata: String,
    pub indexed_at: String,
}

pub struct DocumentRepository<'a> {
    conn: &'a Connection,
}

impl<'a> DocumentRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn create(
        &self,
        source: &str,
        mime_type: &str,
        checksum: &str,
        title: Option<&str>,
        metadata_json: &str,
    ) -> rusqlite::Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO documents (id, source, mime_type, checksum, title, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, source, mime_type, checksum, title, metadata_json],
        )?;
        Ok(id)
    }

    pub fn get_by_checksum(&self, checksum: &str) -> rusqlite::Result<Option<Document>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source, mime_type, title, checksum, metadata, indexed_at \
             FROM documents WHERE checksum = ?1",
        )?;
        let mut rows = stmt.query(params![checksum])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Document {
                id: row.get(0)?,
                source: row.get(1)?,
                mime_type: row.get(2)?,
                title: row.get(3)?,
                checksum: row.get(4)?,
                metadata: row.get(5)?,
                indexed_at: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_by_id(&self, id: &str) -> rusqlite::Result<Option<Document>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source, mime_type, title, checksum, metadata, indexed_at \
             FROM documents WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Document {
                id: row.get(0)?,
                source: row.get(1)?,
                mime_type: row.get(2)?,
                title: row.get(3)?,
                checksum: row.get(4)?,
                metadata: row.get(5)?,
                indexed_at: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_all(&self) -> rusqlite::Result<Vec<Document>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source, mime_type, title, checksum, metadata, indexed_at \
             FROM documents ORDER BY indexed_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Document {
                id: row.get(0)?,
                source: row.get(1)?,
                mime_type: row.get(2)?,
                title: row.get(3)?,
                checksum: row.get(4)?,
                metadata: row.get(5)?,
                indexed_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Updates the title of a document. Returns true if a row was updated.
    pub fn update_title(&self, id: &str, title: Option<&str>) -> rusqlite::Result<bool> {
        let rows = self.conn.execute(
            "UPDATE documents SET title = ?1 WHERE id = ?2",
            rusqlite::params![title, id],
        )?;
        Ok(rows > 0)
    }

    /// Deletes a document by id. Returns true if a row was deleted.
    pub fn delete(&self, id: &str) -> rusqlite::Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM documents WHERE id = ?1", params![id])?;
        Ok(n > 0)
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
    fn create_and_get_by_checksum() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();
        let repo = DocumentRepository::new(&conn);

        let id = repo
            .create("file://test.txt", "text/plain", "abc123", Some("Test"), "{}")
            .unwrap();

        let doc = repo.get_by_checksum("abc123").unwrap().expect("should exist");
        assert_eq!(doc.id, id);
        assert_eq!(doc.source, "file://test.txt");
        assert_eq!(doc.title, Some("Test".to_string()));
    }

    #[test]
    fn duplicate_checksum_returns_error() {
        let (_dir, pool) = setup();
        let conn = pool.get().unwrap();
        let repo = DocumentRepository::new(&conn);

        repo.create("file://a.txt", "text/plain", "dup_checksum", None, "{}")
            .unwrap();

        let result =
            repo.create("file://b.txt", "text/plain", "dup_checksum", None, "{}");
        assert!(result.is_err(), "duplicate checksum should return an error");
    }
}
