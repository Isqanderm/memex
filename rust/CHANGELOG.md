# Rust Changelog

All notable changes to the Rust (SQLite) version of Memex.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)
Versioning: independent from Python version (see root CHANGELOG.md)

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
