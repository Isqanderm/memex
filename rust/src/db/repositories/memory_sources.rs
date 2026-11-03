use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug)]
pub struct MemorySource {
    pub id: String,
    pub kind: String,
    pub checksum: String,
    pub text: String,
    pub created_at: String,
}

pub struct MemorySourceRepository<'a> {
    conn: &'a Connection,
}

/// Hex sha256 of the text (same hashing as the v2 migration backfill).
fn checksum_of(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

impl<'a> MemorySourceRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Returns `(id, created)`; an existing source with the same text checksum is reused.
    pub fn get_or_create(&self, kind: &str, text: &str) -> rusqlite::Result<(String, bool)> {
        let checksum = checksum_of(text);
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM memory_sources WHERE checksum = ?1",
                params![checksum],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok((id, false));
        }
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO memory_sources (id, kind, checksum, text) VALUES (?1, ?2, ?3, ?4)",
            params![id, kind, checksum, text],
        )?;
        Ok((id, true))
    }

    pub fn get_by_id(&self, id: &str) -> rusqlite::Result<Option<MemorySource>> {
        self.conn
            .query_row(
                "SELECT id, kind, checksum, text, created_at FROM memory_sources WHERE id = ?1",
                params![id],
                |r| {
                    Ok(MemorySource {
                        id: r.get(0)?,
                        kind: r.get(1)?,
                        checksum: r.get(2)?,
                        text: r.get(3)?,
                        created_at: r.get(4)?,
                    })
                },
            )
            .optional()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;

    #[test]
    fn source_get_or_create_dedups_by_text() {
        let pool = build_pool(":memory:").unwrap();
        let conn = pool.get().unwrap();
        let repo = MemorySourceRepository::new(&conn);
        let (id1, c1) = repo.get_or_create("conversation", "x").unwrap();
        let (id2, c2) = repo.get_or_create("conversation", "x").unwrap();
        assert!(c1);
        assert!(!c2);
        assert_eq!(id1, id2);
        let s = repo.get_by_id(&id1).unwrap().unwrap();
        assert_eq!(s.text, "x");
        assert_eq!(s.kind, "conversation");
    }
}
