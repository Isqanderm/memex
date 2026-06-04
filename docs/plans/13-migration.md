# Task 13: Migration Tool (PostgreSQL → SQLite)

**Goal:** Утилита `memex-migrate` для переноса существующих данных из PostgreSQL в SQLite + перестройка tantivy индекса из перенесённых чанков.

**Files:**
- Create: `rust/src/bin/migrate.rs`

---

### Task 13.1: Migration Binary

- [ ] **Шаг 1: Добавить зависимости в Cargo.toml**

```toml
# В rust/Cargo.toml добавить:
[dependencies]
tokio-postgres = { version = "0.7", features = ["with-uuid-1", "with-serde_json-1"] }

[[bin]]
name = "memex-migrate"
path = "src/bin/migrate.rs"
```

- [ ] **Шаг 2: Реализовать migrate.rs**

```rust
//! Утилита переноса данных PostgreSQL → SQLite.
//!
//! Запуск:
//!   DATABASE_URL=postgresql://... \
//!   SQLITE_PATH=data/memex.db \
//!   TANTIVY_PATH=data/tantivy \
//!   cargo run --bin memex-migrate
//!
//! Переносит: documents, chunks (без векторов), ingestion_jobs, memories.
//! После переноса — перестраивает tantivy FTS индекс и эмбеддинги (если указан флаг --reembed).

use std::collections::HashMap;
use std::time::Instant;
use tokio_postgres::{Client, NoTls};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let _ = dotenvy::dotenv();

    let pg_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL required"))?;
    let sqlite_path = std::env::var("SQLITE_PATH").unwrap_or("data/memex.db".to_string());
    let tantivy_path = std::env::var("TANTIVY_PATH").unwrap_or("data/tantivy".to_string());
    let reembed = std::env::args().any(|a| a == "--reembed");

    info!("Connecting to PostgreSQL...");
    let (pg, connection) = tokio_postgres::connect(&pg_url, NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            warn!("PG connection error: {e}");
        }
    });

    info!("Opening SQLite at {sqlite_path}...");
    let pool = memex::db::pool::build_pool(&sqlite_path)?;
    let conn = pool.get()?;

    info!("Migrating documents...");
    let n_docs = migrate_documents(&pg, &conn).await?;
    info!("Migrated {n_docs} documents");

    info!("Migrating chunks...");
    let n_chunks = migrate_chunks(&pg, &conn).await?;
    info!("Migrated {n_chunks} chunks");

    info!("Migrating ingestion_jobs...");
    let n_jobs = migrate_jobs(&pg, &conn).await?;
    info!("Migrated {n_jobs} jobs");

    info!("Migrating memories...");
    let n_mems = migrate_memories(&pg, &conn).await?;
    info!("Migrated {n_mems} memories");

    // Перестройка tantivy FTS индекса
    info!("Rebuilding tantivy FTS index...");
    let tantivy = memex::search::TantivyStore::open(&tantivy_path)?;
    tantivy.clear()?;
    let n_indexed = rebuild_tantivy_index(&conn, &tantivy)?;
    info!("Indexed {n_indexed} chunks in tantivy");

    // Пересчёт эмбеддингов (если --reembed)
    if reembed {
        info!("Recomputing embeddings (--reembed)...");
        let embed_model = std::env::var("LOCAL_EMBEDDING_MODEL")
            .unwrap_or("intfloat/multilingual-e5-small".to_string());
        let embed = memex::ingestion::embeddings::EmbeddingClient::new(&embed_model)?;
        let vectors = memex::search::VectorStore::new(embed.dimensions());
        let n_embedded = recompute_embeddings(&conn, &embed, &vectors)?;
        info!("Recomputed {n_embedded} chunk embeddings");
    } else {
        info!("Skipping embeddings recompute (run with --reembed to include)");
        warn!("Vector search will not work until embeddings are computed!");
        warn!("Run: cargo run --bin memex-migrate -- --reembed");
    }

    info!("Migration complete!");
    info!("  documents:  {n_docs}");
    info!("  chunks:     {n_chunks}");
    info!("  jobs:       {n_jobs}");
    info!("  memories:   {n_mems}");

    Ok(())
}

async fn migrate_documents(pg: &Client, conn: &rusqlite::Connection) -> anyhow::Result<usize> {
    let rows = pg.query(
        "SELECT id::text, source, mime_type, title, checksum, metadata::text,
                indexed_at::text
         FROM documents",
        &[],
    ).await?;

    let mut n = 0;
    for row in &rows {
        let id: String = row.get(0);
        let source: String = row.get(1);
        let mime_type: String = row.get(2);
        let title: Option<String> = row.get(3);
        let checksum: String = row.get(4);
        let metadata: String = row.get(5);
        let indexed_at: String = row.get(6);

        conn.execute(
            "INSERT OR IGNORE INTO documents (id, source, mime_type, title, checksum, metadata, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, source, mime_type, title, checksum, metadata, indexed_at],
        )?;
        n += 1;
    }
    Ok(n)
}

async fn migrate_chunks(pg: &Client, conn: &rusqlite::Connection) -> anyhow::Result<usize> {
    let rows = pg.query(
        "SELECT id::text, doc_id::text, parent_chunk_id::text,
                chunk_role, chunk_index, section_heading, section_level,
                page_number, language, content
         FROM chunks",
        &[],
    ).await?;

    let mut n = 0;
    for row in &rows {
        let id: String = row.get(0);
        let doc_id: String = row.get(1);
        let parent_chunk_id: Option<String> = row.get(2);
        let chunk_role: String = row.get(3);
        let chunk_index: i32 = row.get(4);
        let section_heading: Option<String> = row.get(5);
        let section_level: Option<i32> = row.get(6);
        let page_number: Option<i32> = row.get(7);
        let language: String = row.get(8);
        let content: String = row.get(9);

        conn.execute(
            "INSERT OR IGNORE INTO chunks
                (id, doc_id, parent_chunk_id, chunk_role, chunk_index,
                 section_heading, section_level, page_number, language, content)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id, doc_id, parent_chunk_id, chunk_role, chunk_index as i64,
                section_heading, section_level.map(|l| l as i64), page_number.map(|p| p as i64),
                language, content,
            ],
        )?;
        n += 1;
    }
    Ok(n)
}

async fn migrate_jobs(pg: &Client, conn: &rusqlite::Connection) -> anyhow::Result<usize> {
    let rows = pg.query(
        "SELECT id::text, status, source, checksum, doc_id::text, error,
                created_at::text, updated_at::text
         FROM ingestion_jobs",
        &[],
    ).await?;

    let mut n = 0;
    for row in &rows {
        let id: String = row.get(0);
        let status: String = row.get(1);
        let source: String = row.get(2);
        let checksum: String = row.get(3);
        let doc_id: Option<String> = row.get(4);
        let error: Option<String> = row.get(5);
        let created_at: String = row.get(6);
        let updated_at: String = row.get(7);

        conn.execute(
            "INSERT OR IGNORE INTO ingestion_jobs
                (id, status, source, checksum, doc_id, error, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![id, status, source, checksum, doc_id, error, created_at, updated_at],
        )?;
        n += 1;
    }
    Ok(n)
}

async fn migrate_memories(pg: &Client, conn: &rusqlite::Connection) -> anyhow::Result<usize> {
    let rows = pg.query(
        "SELECT id::text, content, raw_input, source, is_active,
                forget_after::text, relation, parent_id::text, category, project,
                created_at::text
         FROM memories",
        &[],
    ).await?;

    let mut n = 0;
    for row in &rows {
        let id: String = row.get(0);
        let content: String = row.get(1);
        let raw_input: String = row.get(2);
        let source: String = row.get(3);
        let is_active: bool = row.get(4);
        let forget_after: Option<String> = row.get(5);
        let relation: Option<String> = row.get(6);
        let parent_id: Option<String> = row.get(7);
        let category: Option<String> = row.get(8);
        let project: Option<String> = row.get(9);
        let created_at: String = row.get(10);

        conn.execute(
            "INSERT OR IGNORE INTO memories
                (id, content, raw_input, source, is_active, forget_after,
                 relation, parent_id, category, project, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                id, content, raw_input, source, is_active as i64, forget_after,
                relation, parent_id, category, project, created_at,
            ],
        )?;
        n += 1;
    }
    Ok(n)
}

fn rebuild_tantivy_index(
    conn: &rusqlite::Connection,
    tantivy: &memex::search::TantivyStore,
) -> anyhow::Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT id, doc_id, language, content FROM chunks WHERE chunk_role = 'leaf'",
    )?;

    let rows: Vec<(String, String, String, String)> = stmt.query_map([], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })?.filter_map(|r| r.ok()).collect();

    for (id, doc_id, lang, content) in &rows {
        tantivy.add_chunk(id, doc_id, lang, content)?;
    }
    tantivy.commit()?;

    Ok(rows.len())
}

fn recompute_embeddings(
    conn: &rusqlite::Connection,
    embed: &memex::ingestion::embeddings::EmbeddingClient,
    vectors: &memex::search::VectorStore,
) -> anyhow::Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT id, content FROM chunks WHERE chunk_role = 'leaf'",
    )?;

    let chunks: Vec<(String, String)> = stmt.query_map([], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?.filter_map(|r| r.ok()).collect();

    let batch_size = 64;
    let mut n = 0;

    for batch in chunks.chunks(batch_size) {
        let texts: Vec<&str> = batch.iter().map(|(_, c)| c.as_str()).collect();
        let embeddings = embed.embed_passages(&texts)?;
        for ((id, _), emb) in batch.iter().zip(embeddings.iter()) {
            vectors.insert_chunk(conn, id, emb)?;
        }
        n += batch.len();
        if n % 1000 == 0 {
            info!("  {n}/{} chunks embedded", chunks.len());
        }
    }

    Ok(n)
}
```

- [ ] **Шаг 3: Проверить компиляцию**

```bash
cd rust && cargo build --bin memex-migrate 2>&1 | tail -5
```

- [ ] **Шаг 4: Тест на пустой БД (dry run)**

```bash
cd rust && DATABASE_URL="postgresql://memex:memex@localhost:5432/memex" \
  SQLITE_PATH="/tmp/test_migrate.db" \
  TANTIVY_PATH="/tmp/test_tantivy" \
  cargo run --bin memex-migrate 2>&1
```

Ожидаем: запуск без паники, лог о количестве перенесённых записей.

- [ ] **Шаг 5: Полный прогон с реальными данными + --reembed**

```bash
cd rust && DATABASE_URL="postgresql://memex:memex@localhost:5432/memex" \
  SQLITE_PATH="data/memex.db" \
  TANTIVY_PATH="data/tantivy" \
  LLM_PROVIDER=claude \
  LLM_MODEL=claude-haiku-4-5-20251001 \
  LOCAL_EMBEDDING_MODEL=intfloat/multilingual-e5-small \
  cargo run --bin memex-migrate -- --reembed 2>&1
```

Проверить результат:
```bash
sqlite3 data/memex.db "SELECT count(*) FROM documents; SELECT count(*) FROM chunks; SELECT count(*) FROM memories;"
```

- [ ] **Шаг 6: Коммит**

```bash
git add rust/src/bin/migrate.rs
git commit -m "feat(rust): migration tool pg→sqlite with tantivy rebuild and re-embedding"
```
