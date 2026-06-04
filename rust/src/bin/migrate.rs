//! Utility to migrate data from PostgreSQL to SQLite.
//!
//! Usage:
//!   DATABASE_URL=postgresql://... \
//!   SQLITE_PATH=data/memex.db \
//!   TANTIVY_PATH=data/tantivy \
//!   cargo run --bin memex-migrate
//!
//! Add --reembed flag to recompute all embeddings (required for vector search to work).

use tokio_postgres::{Client, NoTls};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let _ = dotenvy::dotenv();

    let pg_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL required (postgresql://user:pass@host/db)"))?;
    let sqlite_path = std::env::var("SQLITE_PATH").unwrap_or("data/memex.db".to_string());
    let tantivy_path = std::env::var("TANTIVY_PATH").unwrap_or("data/tantivy".to_string());
    let reembed = std::env::args().any(|a| a == "--reembed");

    // Connect to PostgreSQL
    info!("Connecting to PostgreSQL...");
    let (pg, connection) = tokio_postgres::connect(&pg_url, NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            warn!("PG connection error: {e}");
        }
    });

    // Open SQLite
    info!("Opening SQLite at {sqlite_path}...");
    let pool = memex::db::pool::build_pool(&sqlite_path)?;
    let conn = pool.get()?;

    // Migrate each table
    let n_docs = migrate_documents(&pg, &conn).await?;
    info!("Migrated {n_docs} documents");
    let n_chunks = migrate_chunks(&pg, &conn).await?;
    info!("Migrated {n_chunks} chunks");
    let n_jobs = migrate_jobs(&pg, &conn).await?;
    info!("Migrated {n_jobs} ingestion_jobs");
    let n_mems = migrate_memories(&pg, &conn).await?;
    info!("Migrated {n_mems} memories");

    // Rebuild tantivy FTS from migrated chunks
    info!("Rebuilding tantivy FTS index...");
    let tantivy = memex::search::TantivyStore::open(&tantivy_path)?;
    tantivy.clear()?;
    let n_fts = rebuild_tantivy(&conn, &tantivy)?;
    info!("FTS indexed {n_fts} chunks");

    // Optionally recompute embeddings
    if reembed {
        let embed_model = std::env::var("LOCAL_EMBEDDING_MODEL")
            .unwrap_or("intfloat/multilingual-e5-small".to_string());
        info!("Loading embedding model {embed_model}...");
        let embed = memex::ingestion::EmbeddingClient::new(&embed_model)?;
        let vectors = memex::search::VectorStore::new(embed.dimensions());
        let n_emb = recompute_embeddings(&conn, &embed, &vectors)?;
        info!("Embedded {n_emb} chunks");
    } else {
        warn!("Skipping embeddings (run with --reembed to enable vector search)");
    }

    info!(
        "Migration complete! documents={n_docs} chunks={n_chunks} jobs={n_jobs} memories={n_mems}"
    );
    Ok(())
}

async fn migrate_documents(
    pg: &Client,
    conn: &rusqlite::Connection,
) -> anyhow::Result<usize> {
    let rows = pg
        .query(
            "SELECT id::text, source, mime_type, title, checksum,
                    metadata::text, indexed_at::text
             FROM documents",
            &[],
        )
        .await?;

    for row in &rows {
        let id: String = row.get(0);
        let source: String = row.get(1);
        let mime_type: String = row.get(2);
        let title: Option<String> = row.get(3);
        let checksum: String = row.get(4);
        let metadata: Option<String> = row.get(5);
        let indexed_at: Option<String> = row.get(6);

        conn.execute(
            "INSERT OR IGNORE INTO documents
             (id, source, mime_type, title, checksum, metadata, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, source, mime_type, title, checksum, metadata, indexed_at],
        )?;
    }
    Ok(rows.len())
}

async fn migrate_chunks(
    pg: &Client,
    conn: &rusqlite::Connection,
) -> anyhow::Result<usize> {
    let rows = pg
        .query(
            "SELECT id::text, doc_id::text, parent_chunk_id::text,
                    chunk_role, chunk_index, section_heading, section_level,
                    page_number, language, content
             FROM chunks",
            &[],
        )
        .await?;

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
                id,
                doc_id,
                parent_chunk_id,
                chunk_role,
                chunk_index,
                section_heading,
                section_level,
                page_number,
                language,
                content
            ],
        )?;
    }
    Ok(rows.len())
}

async fn migrate_jobs(
    pg: &Client,
    conn: &rusqlite::Connection,
) -> anyhow::Result<usize> {
    let rows = pg
        .query(
            "SELECT id::text, status, source, checksum, doc_id::text, error,
                    created_at::text, updated_at::text
             FROM ingestion_jobs",
            &[],
        )
        .await?;

    for row in &rows {
        let id: String = row.get(0);
        let status: String = row.get(1);
        let source: String = row.get(2);
        let checksum: Option<String> = row.get(3);
        let doc_id: Option<String> = row.get(4);
        let error: Option<String> = row.get(5);
        let created_at: Option<String> = row.get(6);
        let updated_at: Option<String> = row.get(7);

        conn.execute(
            "INSERT OR IGNORE INTO ingestion_jobs
             (id, status, source, checksum, doc_id, error, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                id, status, source, checksum, doc_id, error, created_at, updated_at
            ],
        )?;
    }
    Ok(rows.len())
}

async fn migrate_memories(
    pg: &Client,
    conn: &rusqlite::Connection,
) -> anyhow::Result<usize> {
    let rows = pg
        .query(
            "SELECT id::text, content, raw_input, source, is_active,
                    forget_after::text, relation, parent_id::text, category, project,
                    created_at::text
             FROM memories",
            &[],
        )
        .await?;

    for row in &rows {
        let id: String = row.get(0);
        let content: String = row.get(1);
        let raw_input: Option<String> = row.get(2);
        let source: Option<String> = row.get(3);
        let is_active: bool = row.get(4);
        let forget_after: Option<String> = row.get(5);
        let relation: Option<String> = row.get(6);
        let parent_id: Option<String> = row.get(7);
        let category: Option<String> = row.get(8);
        let project: Option<String> = row.get(9);
        let created_at: Option<String> = row.get(10);

        conn.execute(
            "INSERT OR IGNORE INTO memories
             (id, content, raw_input, source, is_active,
              forget_after, relation, parent_id, category, project, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                id,
                content,
                raw_input,
                source,
                is_active,
                forget_after,
                relation,
                parent_id,
                category,
                project,
                created_at
            ],
        )?;
    }
    Ok(rows.len())
}

fn rebuild_tantivy(
    conn: &rusqlite::Connection,
    tantivy: &memex::search::TantivyStore,
) -> anyhow::Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT id, doc_id, language, content FROM chunks WHERE chunk_role = 'leaf'",
    )?;
    let rows: Vec<(String, String, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .filter_map(|r| r.ok())
        .collect();

    for (id, doc_id, lang, content) in &rows {
        tantivy.add_chunk(id, doc_id, lang, content)?;
    }
    tantivy.commit()?;
    Ok(rows.len())
}

fn recompute_embeddings(
    conn: &rusqlite::Connection,
    embed: &memex::ingestion::EmbeddingClient,
    vectors: &memex::search::VectorStore,
) -> anyhow::Result<usize> {
    let mut stmt =
        conn.prepare("SELECT id, content FROM chunks WHERE chunk_role = 'leaf'")?;
    let chunks: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let batch_size = 64;
    let mut n = 0;
    let total = chunks.len();
    for batch in chunks.chunks(batch_size) {
        let texts: Vec<&str> = batch.iter().map(|(_, c)| c.as_str()).collect();
        let embeddings = embed.embed_passages(&texts)?;
        for ((id, _), emb) in batch.iter().zip(embeddings.iter()) {
            vectors.insert_chunk(conn, id, emb)?;
        }
        n += batch.len();
        if n % 1000 == 0 {
            info!("  {n}/{total} chunks embedded");
        }
    }
    Ok(n)
}
