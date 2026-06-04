use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

pub type DbPool = Pool<SqliteConnectionManager>;
pub type DbConn = r2d2::PooledConnection<SqliteConnectionManager>;

const SCHEMA: &str = include_str!("schema.sql");

/// Register sqlite-vec as an auto-extension so every connection gets it.
/// Must be called once before any connection is opened.
///
/// We use the same approach as the sqlite-vec crate's own test suite:
/// transmute the C entry-point to the type expected by sqlite3_auto_extension.
fn register_sqlite_vec() {
    // sqlite3_vec_init is the C entry point compiled into libsqlite_vec0.a
    // (linked via the `sqlite-vec` crate's `#[link(name = "sqlite_vec0")]`).
    use sqlite_vec::sqlite3_vec_init;

    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite3_vec_init as *const (),
        )));
    }
}

fn init_connection(conn: &mut Connection) -> rusqlite::Result<()> {
    // WAL + foreign keys
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;",
    )?;

    // Apply schema (idempotent via IF NOT EXISTS)
    conn.execute_batch(SCHEMA)?;

    // Create sqlite-vec virtual tables for vectors
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunk_vectors USING vec0(
            chunk_id TEXT,
            embedding float[384]
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS memory_vectors USING vec0(
            memory_id TEXT,
            embedding float[384]
         );",
    )?;

    Ok(())
}

pub fn build_pool(database_path: &str) -> Result<DbPool, r2d2::Error> {
    // Register sqlite-vec once before opening any connections.
    register_sqlite_vec();

    let manager = SqliteConnectionManager::file(database_path)
        .with_init(init_connection);
    Pool::builder().max_size(16).build(manager)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pool_opens_and_schema_created() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = build_pool(db_path.to_str().unwrap()).expect("pool build failed");
        let conn = pool.get().expect("get conn");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='documents'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "documents table should exist");
    }

    #[test]
    fn wal_mode_is_active() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_wal.db");
        let pool = build_pool(db_path.to_str().unwrap()).expect("pool build failed");
        let conn = pool.get().expect("get conn");

        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal", "journal_mode should be WAL");
    }
}
