//! Schema migrations driven by `PRAGMA user_version`.
//!
//! `schema.sql` describes the fresh (latest) shape with `IF NOT EXISTS`; `migrate`
//! upgrades databases created by older releases and stamps fresh ones.

use rusqlite::Connection;
use sha2::{Digest, Sha256};

pub const LATEST_VERSION: u32 = 1;

fn user_version(conn: &Connection) -> rusqlite::Result<u32> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |r| r.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Apply all pending migrations; returns the resulting schema version.
/// Cheap and idempotent: a database already at `LATEST_VERSION` is untouched.
pub fn migrate(conn: &Connection) -> rusqlite::Result<u32> {
    let current = user_version(conn)?;
    if current >= LATEST_VERSION {
        return Ok(current);
    }
    for v in current + 1..=LATEST_VERSION {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let step = match v {
            1 => migrate_to_1(conn),
            _ => Ok(()),
        }
        .and_then(|()| conn.execute_batch(&format!("PRAGMA user_version = {v}")));
        match step {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
    }
    user_version(conn)
}

/// v1: memory sources, scope, access stats, profile cache.
fn migrate_to_1(conn: &Connection) -> rusqlite::Result<()> {
    // Tables that may be missing on pre-v1 databases (no-ops when schema.sql made them).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_sources (
             id         TEXT PRIMARY KEY,
             kind       TEXT NOT NULL CHECK (kind IN ('explicit','conversation','document')),
             checksum   TEXT NOT NULL UNIQUE,
             text       TEXT NOT NULL,
             created_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE TABLE IF NOT EXISTS profile_cache (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             fingerprint TEXT NOT NULL,
             static_summary TEXT NOT NULL,
             dynamic_summary TEXT NOT NULL,
             raw_count INTEGER NOT NULL,
             built_at TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )?;

    // Fresh databases already have the v1 shape: nothing to alter.
    if column_exists(conn, "memories", "raw_input")? {
        conn.execute_batch(
            "ALTER TABLE memories ADD COLUMN source_id TEXT REFERENCES memory_sources(id);
             ALTER TABLE memories ADD COLUMN scope TEXT NOT NULL DEFAULT 'default';
             ALTER TABLE memories ADD COLUMN updated_at TEXT;
             UPDATE memories SET updated_at = created_at;
             ALTER TABLE memories ADD COLUMN last_accessed_at TEXT;
             ALTER TABLE memories ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0;",
        )?;

        // One source row per distinct raw_input.
        let rows: Vec<(String, String)> = {
            let mut stmt =
                conn.prepare("SELECT raw_input, MIN(source) FROM memories GROUP BY raw_input")?;
            let mapped = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            mapped.collect::<rusqlite::Result<_>>()?
        };
        for (raw, source) in rows {
            let checksum = hex::encode(Sha256::digest(raw.as_bytes()));
            // memories.source is free-form in rc.4; map unknown values to 'explicit'.
            let kind = match source.as_str() {
                "explicit" | "conversation" | "document" => source.as_str(),
                _ => "explicit",
            };
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO memory_sources (id, kind, checksum, text) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, kind, checksum, raw],
            )?;
            conn.execute(
                "UPDATE memories SET source_id = ?1 WHERE raw_input = ?2",
                rusqlite::params![id, raw],
            )?;
        }
        conn.execute_batch("ALTER TABLE memories DROP COLUMN raw_input;")?;
    }

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope, is_active);",
    )?;

    if !column_exists(conn, "memory_extraction_jobs", "scope")? {
        conn.execute_batch(
            "ALTER TABLE memory_extraction_jobs ADD COLUMN scope TEXT NOT NULL DEFAULT 'default';",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RC4_SCHEMA: &str = "
        CREATE TABLE memories (
            id           TEXT PRIMARY KEY,
            content      TEXT NOT NULL,
            raw_input    TEXT NOT NULL,
            source       TEXT NOT NULL,
            is_active    INTEGER NOT NULL DEFAULT 1,
            forget_after TEXT,
            relation     TEXT,
            parent_id    TEXT REFERENCES memories(id),
            category     TEXT,
            project      TEXT,
            created_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX idx_memories_active ON memories(is_active, created_at)
            WHERE is_active = 1;
        CREATE TABLE memory_extraction_jobs (
            id               TEXT PRIMARY KEY,
            source_ref       TEXT NOT NULL,
            source           TEXT NOT NULL,
            status           TEXT NOT NULL DEFAULT 'pending',
            facts_extracted  INTEGER NOT NULL DEFAULT 0,
            error            TEXT,
            created_at       TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at       TEXT NOT NULL DEFAULT (datetime('now'))
        );";

    fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
        column_exists(conn, table, column).unwrap()
    }

    #[test]
    fn fresh_db_ends_at_latest_version_without_raw_input() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("schema.sql")).unwrap();
        assert_eq!(migrate(&conn).unwrap(), LATEST_VERSION);
        assert!(!has_column(&conn, "memories", "raw_input"));
        assert!(has_column(&conn, "memories", "scope"));
        assert!(has_column(&conn, "memory_extraction_jobs", "scope"));
    }

    #[test]
    fn rc4_db_is_migrated_and_sources_backfilled() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(RC4_SCHEMA).unwrap();
        conn.execute("INSERT INTO memories (id, content, raw_input, source) VALUES ('m1','User uses Rust','I use Rust','explicit')", []).unwrap();
        conn.execute("INSERT INTO memories (id, content, raw_input, source) VALUES ('m2','User lives in Belgrade','I use Rust','explicit')", []).unwrap();
        assert_eq!(migrate(&conn).unwrap(), LATEST_VERSION);
        let n: i64 = conn
            .query_row("SELECT count(*) FROM memory_sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let txt: String = conn
            .query_row(
                "SELECT s.text FROM memories m JOIN memory_sources s ON s.id = m.source_id WHERE m.id='m2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(txt, "I use Rust");
        assert!(!has_column(&conn, "memories", "raw_input"));
        assert!(has_column(&conn, "memory_extraction_jobs", "scope"));
        let upd: Option<String> = conn
            .query_row("SELECT updated_at FROM memories WHERE id='m1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(upd.is_some());
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("schema.sql")).unwrap();
        migrate(&conn).unwrap();
        assert_eq!(migrate(&conn).unwrap(), LATEST_VERSION);
    }
}
