# Task 2: Database Layer

**Goal:** SQLite пул соединений, SQL-схема (аналог Alembic миграций), базовые CRUD репозитории для всех пяти таблиц.

**Files:**
- Create: `rust/src/db/mod.rs`
- Create: `rust/src/db/pool.rs`
- Create: `rust/src/db/schema.sql`
- Create: `rust/src/db/repositories/documents.rs`
- Create: `rust/src/db/repositories/chunks.rs`
- Create: `rust/src/db/repositories/jobs.rs`
- Create: `rust/src/db/repositories/memories.rs`

---

### Task 2.1: SQLite схема

- [ ] **Шаг 1: Создать rust/src/db/schema.sql**

```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
PRAGMA synchronous=NORMAL;

CREATE TABLE IF NOT EXISTS documents (
    id          TEXT PRIMARY KEY,
    source      TEXT NOT NULL,
    mime_type   TEXT NOT NULL,
    title       TEXT,
    checksum    TEXT NOT NULL UNIQUE,
    metadata    TEXT NOT NULL DEFAULT '{}',
    indexed_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS chunks (
    id                TEXT PRIMARY KEY,
    doc_id            TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    parent_chunk_id   TEXT REFERENCES chunks(id) ON DELETE SET NULL,
    chunk_role        TEXT NOT NULL CHECK (chunk_role IN ('parent', 'leaf')),
    chunk_index       INTEGER NOT NULL,
    section_heading   TEXT,
    section_level     INTEGER,
    page_number       INTEGER,
    prev_chunk_id     TEXT REFERENCES chunks(id) ON DELETE SET NULL,
    next_chunk_id     TEXT REFERENCES chunks(id) ON DELETE SET NULL,
    language          TEXT NOT NULL DEFAULT 'en',
    content           TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chunks_doc_id   ON chunks(doc_id);
CREATE INDEX IF NOT EXISTS idx_chunks_parent   ON chunks(parent_chunk_id) WHERE parent_chunk_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_chunks_role     ON chunks(chunk_role);

CREATE TABLE IF NOT EXISTS ingestion_jobs (
    id          TEXT PRIMARY KEY,
    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending','processing','done','error')),
    source      TEXT NOT NULL,
    checksum    TEXT NOT NULL,
    doc_id      TEXT REFERENCES documents(id) ON DELETE SET NULL,
    error       TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_jobs_status ON ingestion_jobs(status, created_at)
    WHERE status IN ('pending', 'processing');
CREATE UNIQUE INDEX IF NOT EXISTS uq_jobs_checksum_active
    ON ingestion_jobs(checksum)
    WHERE status IN ('pending', 'processing');

CREATE TABLE IF NOT EXISTS memories (
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

CREATE INDEX IF NOT EXISTS idx_memories_active ON memories(is_active, created_at)
    WHERE is_active = 1;

CREATE TABLE IF NOT EXISTS memory_extraction_jobs (
    id               TEXT PRIMARY KEY,
    source_ref       TEXT NOT NULL,
    source           TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'pending',
    facts_extracted  INTEGER NOT NULL DEFAULT 0,
    error            TEXT,
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at       TEXT NOT NULL DEFAULT (datetime('now'))
);
```

---

### Task 2.2: Connection Pool

- [ ] **Шаг 1: Написать тест пула**

```rust
// rust/src/db/pool.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pool_opens_and_schema_created() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = DbPool::new(db_path.to_str().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        // Проверяем что таблицы созданы
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='documents'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn wal_mode_is_active() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = DbPool::new(db_path.to_str().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }
}
```

- [ ] **Шаг 2: Запустить тест — убедиться что FAIL**

```bash
cd rust && cargo test db::pool 2>&1 | tail -5
```

- [ ] **Шаг 3: Реализовать pool.rs**

```rust
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

pub type DbPool = Pool<SqliteConnectionManager>;
pub type DbConn = r2d2::PooledConnection<SqliteConnectionManager>;

const SCHEMA: &str = include_str!("schema.sql");

pub fn build_pool(database_path: &str) -> Result<DbPool, r2d2::Error> {
    let manager = SqliteConnectionManager::file(database_path)
        .with_init(|conn| init_connection(conn));
    Pool::builder().max_size(16).build(manager)
}

fn init_connection(conn: &Connection) -> rusqlite::Result<()> {
    // Загрузить sqlite-vec расширение
    unsafe { conn.load_extension_enable()? };
    sqlite_vec::load(conn)?;
    unsafe { conn.load_extension_disable()? };

    // WAL + foreign keys
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;

    // Применить схему (идемпотентно через IF NOT EXISTS)
    conn.execute_batch(SCHEMA)?;

    // Создать sqlite-vec виртуальные таблицы
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pool_opens_and_schema_created() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = build_pool(db_path.to_str().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='documents'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn wal_mode_is_active() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = build_pool(db_path.to_str().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }
}
```

- [ ] **Шаг 4: Запустить тесты**

```bash
cd rust && cargo test db::pool 2>&1
```

Ожидаем: оба теста зелёные.

---

### Task 2.3: Document Repository

- [ ] **Шаг 1: Написать тесты**

```rust
// rust/src/db/repositories/documents.rs — начать с тестов
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use tempfile::tempdir;

    fn make_pool() -> crate::db::pool::DbPool {
        let dir = tempdir().unwrap();
        build_pool(dir.path().join("test.db").to_str().unwrap()).unwrap()
    }

    #[test]
    fn create_and_get_by_checksum() {
        let pool = make_pool();
        let conn = pool.get().unwrap();
        let repo = DocumentRepository::new(&conn);

        let id = repo.create("test.pdf", "application/pdf", "abc123", None, "{}").unwrap();
        let doc = repo.get_by_checksum("abc123").unwrap().unwrap();

        assert_eq!(doc.id, id);
        assert_eq!(doc.mime_type, "application/pdf");
    }

    #[test]
    fn duplicate_checksum_returns_none_on_create() {
        let pool = make_pool();
        let conn = pool.get().unwrap();
        let repo = DocumentRepository::new(&conn);

        repo.create("a.pdf", "application/pdf", "dup123", None, "{}").unwrap();
        // Второй вызов с тем же checksum — должен вернуть ошибку UNIQUE constraint
        let result = repo.create("b.pdf", "application/pdf", "dup123", None, "{}");
        assert!(result.is_err());
    }
}
```

- [ ] **Шаг 2: Запустить — убедиться что FAIL**

```bash
cd rust && cargo test documents 2>&1 | tail -5
```

- [ ] **Шаг 3: Реализовать document repository**

```rust
use rusqlite::{Connection, params};
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
            "INSERT INTO documents (id, source, mime_type, checksum, title, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, source, mime_type, checksum, title, metadata_json],
        )?;
        Ok(id)
    }

    pub fn get_by_checksum(&self, checksum: &str) -> rusqlite::Result<Option<Document>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, source, mime_type, title, checksum, metadata, indexed_at
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
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, source, mime_type, title, checksum, metadata, indexed_at
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
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, source, mime_type, title, checksum, metadata, indexed_at
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

    pub fn delete(&self, id: &str) -> rusqlite::Result<bool> {
        let n = self.conn.execute("DELETE FROM documents WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }
}
```

- [ ] **Шаг 4: Запустить тесты**

```bash
cd rust && cargo test documents 2>&1
```

Ожидаем: оба теста зелёные.

---

### Task 2.4: Job Repository

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/db/repositories/jobs.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use tempfile::tempdir;

    #[test]
    fn claim_next_returns_pending_job() {
        let pool = build_pool(tempdir().unwrap().path().join("t.db").to_str().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        let repo = JobRepository::new(&conn);

        let id = repo.create("file.pdf", "abc").unwrap();
        let job = repo.claim_next().unwrap().unwrap();

        assert_eq!(job.id, id);
        assert_eq!(job.status, "processing");
    }

    #[test]
    fn mark_done_updates_status() {
        let pool = build_pool(tempdir().unwrap().path().join("t.db").to_str().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        let repo = JobRepository::new(&conn);

        let id = repo.create("f.pdf", "cks").unwrap();
        repo.claim_next().unwrap();
        repo.mark_done(&id, "doc-uuid-1").unwrap();

        let job = repo.get_by_id(&id).unwrap().unwrap();
        assert_eq!(job.status, "done");
        assert_eq!(job.doc_id, Some("doc-uuid-1".to_string()));
    }
}
```

- [ ] **Шаг 2: Реализовать jobs.rs**

```rust
use rusqlite::{Connection, params};
use uuid::Uuid;

#[derive(Debug)]
pub struct IngestionJob {
    pub id: String,
    pub status: String,
    pub source: String,
    pub checksum: String,
    pub doc_id: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
}

pub struct JobRepository<'a> {
    conn: &'a Connection,
}

impl<'a> JobRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self { Self { conn } }

    pub fn create(&self, source: &str, checksum: &str) -> rusqlite::Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO ingestion_jobs (id, source, checksum) VALUES (?1, ?2, ?3)",
            params![id, source, checksum],
        )?;
        Ok(id)
    }

    /// Атомарно берёт следующий pending-джоб и переводит в processing.
    pub fn claim_next(&self) -> rusqlite::Result<Option<IngestionJob>> {
        let id: Option<String> = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT id FROM ingestion_jobs WHERE status = 'pending'
                 ORDER BY created_at LIMIT 1",
            )?;
            let mut rows = stmt.query([])?;
            rows.next()?.map(|r| r.get(0)).transpose()?
        };

        let id = match id {
            Some(id) => id,
            None => return Ok(None),
        };

        self.conn.execute(
            "UPDATE ingestion_jobs SET status='processing', updated_at=datetime('now') WHERE id=?1",
            params![id],
        )?;

        self.get_by_id(&id)
    }

    pub fn get_by_id(&self, id: &str) -> rusqlite::Result<Option<IngestionJob>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, status, source, checksum, doc_id, error, created_at
             FROM ingestion_jobs WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(IngestionJob {
                id: row.get(0)?,
                status: row.get(1)?,
                source: row.get(2)?,
                checksum: row.get(3)?,
                doc_id: row.get(4)?,
                error: row.get(5)?,
                created_at: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_by_checksum_active(&self, checksum: &str) -> rusqlite::Result<Option<IngestionJob>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, status, source, checksum, doc_id, error, created_at
             FROM ingestion_jobs
             WHERE checksum = ?1 AND status IN ('pending', 'processing')
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![checksum])?;
        if let Some(row) = rows.next()? {
            Ok(Some(IngestionJob {
                id: row.get(0)?,
                status: row.get(1)?,
                source: row.get(2)?,
                checksum: row.get(3)?,
                doc_id: row.get(4)?,
                error: row.get(5)?,
                created_at: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn mark_done(&self, id: &str, doc_id: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE ingestion_jobs SET status='done', doc_id=?2, updated_at=datetime('now') WHERE id=?1",
            params![id, doc_id],
        )?;
        Ok(())
    }

    pub fn mark_error(&self, id: &str, error: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE ingestion_jobs SET status='error', error=?2, updated_at=datetime('now') WHERE id=?1",
            params![id, error],
        )?;
        Ok(())
    }

    pub fn list_active(&self) -> rusqlite::Result<Vec<IngestionJob>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, status, source, checksum, doc_id, error, created_at
             FROM ingestion_jobs WHERE status IN ('pending','processing','error')
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| Ok(IngestionJob {
            id: r.get(0)?,
            status: r.get(1)?,
            source: r.get(2)?,
            checksum: r.get(3)?,
            doc_id: r.get(4)?,
            error: r.get(5)?,
            created_at: r.get(6)?,
        }))?;
        rows.collect()
    }
}
```

- [ ] **Шаг 3: Запустить тесты**

```bash
cd rust && cargo test jobs 2>&1
```

Ожидаем: 2 теста зелёных.

---

### Task 2.5: Memory Repository

- [ ] **Шаг 1: Написать тест**

```rust
// rust/src/db/repositories/memories.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::build_pool;
    use tempfile::tempdir;

    #[test]
    fn create_and_get_active() {
        let pool = build_pool(tempdir().unwrap().path().join("t.db").to_str().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);

        let id = repo.create("User likes Rust", "I like Rust", "explicit", None, None, None, None).unwrap();
        let mems = repo.get_all_active(None).unwrap();

        assert_eq!(mems.len(), 1);
        assert_eq!(mems[0].id, id);
        assert_eq!(mems[0].content, "User likes Rust");
    }

    #[test]
    fn deactivate_removes_from_active() {
        let pool = build_pool(tempdir().unwrap().path().join("t.db").to_str().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        let repo = MemoryRepository::new(&conn);

        let id = repo.create("User likes Go", "I also like Go", "explicit", None, None, None, None).unwrap();
        repo.deactivate(&id).unwrap();

        let mems = repo.get_all_active(None).unwrap();
        assert!(mems.is_empty());
    }
}
```

- [ ] **Шаг 2: Реализовать memories.rs**

```rust
use rusqlite::{Connection, params};
use uuid::Uuid;

#[derive(Debug, Clone)]
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
    pub fn new(conn: &'a Connection) -> Self { Self { conn } }

    pub fn create(
        &self,
        content: &str,
        raw_input: &str,
        source: &str,
        parent_id: Option<&str>,
        relation: Option<&str>,
        forget_after: Option<&str>,
        category: Option<&str>,
    ) -> rusqlite::Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO memories (id, content, raw_input, source, parent_id, relation, forget_after, category)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, content, raw_input, source, parent_id, relation, forget_after, category],
        )?;
        Ok(id)
    }

    pub fn deactivate(&self, id: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE memories SET is_active = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn get_by_id(&self, id: &str) -> rusqlite::Result<Option<Memory>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, content, raw_input, source, is_active, forget_after,
                    relation, parent_id, category, project, created_at
             FROM memories WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_memory(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_all_active(&self, category: Option<&str>) -> rusqlite::Result<Vec<Memory>> {
        if let Some(cat) = category {
            let mut stmt = self.conn.prepare_cached(
                "SELECT id, content, raw_input, source, is_active, forget_after,
                        relation, parent_id, category, project, created_at
                 FROM memories WHERE is_active = 1 AND category = ?1
                 ORDER BY created_at DESC",
            )?;
            stmt.query_map(params![cat], |r| row_to_memory(r))?.collect()
        } else {
            let mut stmt = self.conn.prepare_cached(
                "SELECT id, content, raw_input, source, is_active, forget_after,
                        relation, parent_id, category, project, created_at
                 FROM memories WHERE is_active = 1
                 ORDER BY created_at DESC",
            )?;
            stmt.query_map([], |r| row_to_memory(r))?.collect()
        }
    }

    /// Деактивирует memories у которых forget_after < now().
    pub fn expire_stale(&self) -> rusqlite::Result<usize> {
        let n = self.conn.execute(
            "UPDATE memories SET is_active = 0
             WHERE is_active = 1
               AND forget_after IS NOT NULL
               AND forget_after < datetime('now')",
            [],
        )?;
        Ok(n)
    }
}

fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: row.get(0)?,
        content: row.get(1)?,
        raw_input: row.get(2)?,
        source: row.get(3)?,
        is_active: row.get::<_, i64>(4)? != 0,
        forget_after: row.get(5)?,
        relation: row.get(6)?,
        parent_id: row.get(7)?,
        category: row.get(8)?,
        project: row.get(9)?,
        created_at: row.get(10)?,
    })
}
```

- [ ] **Шаг 3: Запустить все тесты**

```bash
cd rust && cargo test 2>&1
```

Ожидаем: все тесты зелёные.

- [ ] **Шаг 4: Обновить db/mod.rs**

```rust
// rust/src/db/mod.rs
pub mod pool;
pub mod repositories;

pub use pool::{DbPool, DbConn, build_pool};
```

```rust
// rust/src/db/repositories/mod.rs
pub mod documents;
pub mod chunks;
pub mod jobs;
pub mod memories;
```

> **Примечание:** `chunks.rs` — реализуется в Task 7 (вместе с IndexingStage), когда станет ясна точная структура с embedded sqlite-vec.

- [ ] **Шаг 5: Коммит**

```bash
git add rust/src/db/
git commit -m "feat(rust): SQLite schema, connection pool, document/job/memory repositories"
```
