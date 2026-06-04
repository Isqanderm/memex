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
