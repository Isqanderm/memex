# Rust Changelog

All notable changes to the Rust (SQLite) version of Memex.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)
Versioning: independent from Python version (see root CHANGELOG.md)

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

### Known differences from Python version

- `GET /api/memory/context` returns `static_summary`/`dynamic_summary` (Python: `static`/`dynamic`)
- `POST /api/search/chunks` returns array directly (Python: `{"chunks": [...]}`)
- `DELETE /api/memory/:id` returns 204 No Content (Python: `{"status": "deleted"}`)
- No `PATCH /api/documents/:id` endpoint
- No `GET /api/documents/:id/file` endpoint
