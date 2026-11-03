# Rust Changelog

All notable changes to the Rust (SQLite) version of Memex.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)
Versioning: independent from Python version (see root CHANGELOG.md)

---

## [3.0.0-rc.6] — 2026-11-03

### Upgrading from rc.5

- The database schema moves to v2. The first start migrates rc.4/rc.5 databases automatically; back up `data/memex.db` before upgrading.
- `POST /api/memory/observe` and the MCP `observe` tool no longer return extracted facts; see Changed.
- Memory search reranks candidates with the cross-encoder; facts are indexed without the leading 'User' token — the first start after upgrading rebuilds the memory keyword index automatically (versioned by `data/tantivy/memories/INDEX_VERSION`). `MEMORY_RERANK=false` turns the rerank off.

### Added

- MCP tool `check_memory_job` and `GET /api/memory/jobs/{job_id}` (`{job_id, status, facts_extracted, error}`)

### Changed

- `observe` is asynchronous: the conversation is stored and queued in `memory_extraction_jobs`, a background worker extracts the facts. `POST /api/memory/observe` returns `202 Accepted` with `{"job_id", "status": "pending"}`; the MCP tool returns `Queued. job_id: <id>. Check with check_memory_job.` The same conversation sent twice in the same scope maps to the same job; blank conversations are rejected with 400. `remember` stays synchronous.
- `recall` searches memory facts without calling the LLM

---

## [3.0.0-rc.5] — 2026-11-03

### Upgrading from rc.4

- The model cache moved to `MODEL_CACHE_DIR` (default `data/models`) and the default reranker changed, so the first start downloads about 1.2 GB again. Set `RERANKER_MODEL=bge-reranker-base` to keep the previous ranking.
- The `memex-mcp` binary is gone; use `claude mcp add --transport http memex http://localhost:8000/mcp`.

### Added

- `LLM_BASE_URL` — any OpenAI-compatible endpoint (e.g. Ollama); `OPENAI_LLM_API_KEY` is optional when it is set
- JSON mode for fact extraction
- MCP over Streamable HTTP at `/mcp` in the main process (stateless, JSON responses, 16 MiB body limit, browser `Origin` rejected, `Host` allow-list via `MCP_ALLOWED_HOSTS`)
- MCP tools `index_text` and `delete_document`
- `memex --download-models` pre-downloads the ONNX models into `MODEL_CACHE_DIR` and exits
- `RERANKER_MODEL` option, including `bge-reranker-v2-m3`
- Docker image `ghcr.io/isqanderm/memex:rust-<version>` built by `docker.yml` on `rust/v*` tags

### Removed

- `memex-mcp` binary (and its release artifacts); use `claude mcp add --transport http memex http://localhost:8000/mcp`
- MCP tool `index_file` (use `index_text` or the upload API)

### Changed

- `docker-compose.rust.yml` binds the port to `127.0.0.1:8000` and uses a single `./data:/app/data` mount
- ONNX models are baked into the image at `/app/models` (image is ~1.8 GB)
- Default reranker is `jina-reranker-v2-base-multilingual`

---

## [3.0.0-rc.4] — 2026-06-05

### Fixed

- `MemoryExpiryWorker` now responds to shutdown immediately via `tokio::select!`; previously the worker waited up to `interval_secs` (up to 1 hour) before checking the shutdown signal

---

## [3.0.0-rc.3] — 2026-06-05

### Fixed

- **L2/cosine distance math** — `find_similar_memories` now uses correct formula `sqrt(2*(1-cos))` for unit-normalized vectors; previous `1.0-cos` was 8× stricter causing valid memory updates to be stored as new facts
- **Embedding dimension mismatch** — `insert_chunk`/`insert_memory` now return `Err` instead of panicking when embedding size doesn't match store dimensions
- **Filename display in context** — source file names with 5+ dashes no longer get truncated; uses correct 16-char checksum prefix stripping
- **Upload size limit** — `POST /api/documents` now enforces 100 MB body limit; previously unlimited upload could OOM the server
- **LLM request timeout** — OpenAI and Claude clients now timeout after 120s; previously a hung LLM call would block all worker threads
- **SQLite busy_timeout** — pool connections now set `PRAGMA busy_timeout=5000`; previously concurrent writes returned immediate `SQLITE_BUSY` (HTTP 500) instead of waiting

### CI

- `memex-mcp` binary now included in GitHub Release artifacts alongside `memex`

### Docs

- `docs/rust.md`: added `poppler-utils` installation note for PDF support in binary deployments


---

## [3.0.0-rc.2] — 2026-06-05

### Fixed

- `POST /api/search/chunks` now returns `{"chunks": [...]}` matching Python API (was: bare array)
- `DELETE /api/memory/:id` now returns `{"status": "deleted"}` matching Python API (was: 204 No Content)
- Rust `1.82` → `1.85` in Dockerfile (required for `edition2024` dependencies)
- ARM64 binary: switched from cross-compilation to native `ubuntu-24.04-arm` runner — eliminates OpenSSL toolchain issues
- Docker image tags lowercase (OCI requirement)

### Changed

- Removed `memex-migrate` binary and `tokio-postgres` dependency — PostgreSQL migration is out of scope for the Rust edition
- `reqwest` switched to `rustls-tls` backend (no system OpenSSL required)

---

## [3.0.0-rc.1] — 2026-06-05

### Added

- `GET /api/documents/:id/file` — serve original uploaded file
- `PATCH /api/documents/:id` — update document title
- `GET /api/memory/list` now returns `relation` field
- `GET /api/memory/context` now returns `static`/`dynamic` (aligned with Python API)
- EPUB document adapter (via `epub` crate)
- `memex-mcp` — native MCP server binary for Claude Code integration (9 tools: remember, recall, context, observe, memories, index_file, check_indexing, list_documents, forget)

### Fixed

- Upload filename collision: files now stored with checksum prefix to prevent overwrites
- EPUB removed from UI until adapter was implemented; now re-added
- `curl` added to Docker runtime image (required for healthcheck)
- MCP `index_file`: store plain path instead of `file://` URI

### Known intentional differences from Python version

- `POST /api/search/chunks` returns array directly (Python: `{"chunks": [...]}`)
- `DELETE /api/memory/:id` returns 204 No Content (Python: `{"status": "deleted"}`)

---

## [3.0.0] — 2026-06-04

### Added

- Initial Rust implementation: Axum + SQLite + ONNX embeddings
- Full API parity with Python version for core endpoints:
  `/api/documents`, `/api/jobs`, `/api/memory/*`, `/api/search/chunks`, `/api/query`
- Web UI via minijinja (Jinja2-compatible templates)
- MCP-compatible REST API
- Local embedding model via fastembed (multilingual-e5-small, 384 dims)
- BM25 full-text search via tantivy
- Cross-encoder reranker via ONNX
- Single binary deployment — no PostgreSQL, no Python required
- `GET /health` endpoint

### Architecture

- Single binary, ~25 MB stripped
- SQLite for storage (vs PostgreSQL in Python version)
- Idle RAM: ~80-120 MB (vs ~2.5 GB Python+PostgreSQL)
- Cold start: ~2-3 seconds including model loading

All API differences from Python were resolved in [3.0.0-rc.1](#300-rc1--2026-06-05).
