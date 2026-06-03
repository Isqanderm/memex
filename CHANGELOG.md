# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.1.0] — 2026-06-03

### Added

- **Memory categories** — `category` (research/reminder/decision/preference/insight) and `project` fields on memory facts, extracted automatically by LLM during `remember()`. No manual tagging required.
- **Category filtering** — `recall(query, category="research")` MCP tool, `GET /api/memory/list?category=...`, `POST /api/query` with `memory_category` field. Invalid category values rejected at API boundary with 422.
- **Rich retrieval context** — memories display as `[memory | decision | Memex | 2026-06-02]` instead of `[memory]`, giving LLM temporal and categorical context for better reasoning.
- **ContextBuilder v2** — explicit memory vs document hierarchy, today's date injection, explicit "I don't know" instruction. A/B benchmark: +23% keyword accuracy, zero regressions.
- **A/B benchmark** — `tests/research/rq_prompt_ab_test.py`, 11 cases across 6 categories.
- **ADR-0016** — documents the local embedding decision.

### Upgrading from 2.0.0

```bash
docker compose -f docker-compose.prod.yml pull
docker compose -f docker-compose.prod.yml up -d
docker exec memex alembic upgrade head   # applies migration 0006
```

Migration 0006 adds nullable `category` and `project` columns — no data loss, no re-indexing required.

---

## [2.0.0] — 2026-06-02

### ⚠️ Breaking Changes

- **Migration 0004 — vector resize (1536 → 384).** After `alembic upgrade head`, all existing chunk and memory vectors are NULLed. Search returns no results until re-indexing. Run `uv run python scripts/reindex.py` (server must be running) to restore search.
- **`remember` lost `title` and `tags` parameters.** The tool now extracts atomic facts via LLM automatically. Clients that passed `title` or `tags` must remove these fields — they are silently ignored if passed to the new endpoint.
- **Embeddings are now local-only.** `OPENAI_API_KEY` is no longer used for embeddings. `EMBEDDING_MODEL` env var is obsolete. Embeddings run via `intfloat/multilingual-e5-small` (bundled in Docker image).

### Upgrading from 1.0.0

```bash
# 1. Pull and restart containers
docker compose -f docker-compose.prod.yml pull
docker compose -f docker-compose.prod.yml up -d

# 2. Apply migrations (nulls existing vectors)
docker exec memex alembic upgrade head

# 3. Re-embed all documents and memories (requires running server)
docker exec memex uv run python scripts/reindex.py

# 4. Remove from .env (no longer needed):
#    OPENAI_API_KEY
#    EMBEDDING_MODEL
#    EMBEDDING_DIMENSIONS
```

### Added

- **Memory layer** — per-user evolving facts extracted from text, conversations, and documents via LLM. New tools: `context`, `observe`, `memories`.
- **Fact evolution** — LLM resolves `updates / extends / derives` relations between facts. Old facts are deactivated when superseded (e.g. "moved to Berlin" supersedes "lives in Moscow").
- **User profile** — `context()` returns a structured `static` + `dynamic` profile for system prompt injection at session start.
- **Memory injection** — `recall()` and Web UI search automatically prepend relevant memory facts before document chunks.
- **Local embeddings** — `intfloat/multilingual-e5-small` (117 MB, 384 dims, multilingual). No API key, no cost, 27× faster than OpenAI API (~50ms vs ~1400ms).
- **Auto-expiry** — time-bound facts (`forget_after`) are deactivated hourly.
- **Document memory extraction** — uploaded documents are automatically parsed for personal facts after indexing.
- **REST API** — `/api/memory/remember`, `/api/memory/observe`, `/api/memory/list`, `/api/memory/context`, `DELETE /api/memory/{id}`.
- **Dev profiling** — `MEMEX_PROFILE=1` enables per-step timing logs (embed / semantic / bm25 / rerank / memory / llm).
- **`scripts/reindex.py`** — re-embed all chunks and memories after switching embedding model.

### Fixed

- **Reranker cold start** — model warmed up at server startup (12 s → 0.7 s first request).
- **Chunk FK constraint** — `parent_chunk_id`, `prev_chunk_id`, `next_chunk_id` now have `ON DELETE SET NULL` (migration 0005). Document deletion no longer fails with `ForeignKeyViolationError`.
- **pgvector cast in raw SQL** — `:param::vector` pattern replaced with f-string interpolation, consistent with `SemanticSearch`.

### Changed

- Hermes skill updated to v2.0.0 — session protocol, new tools, updated `remember` description.
- Hermes MCP bridge updated — new tools, `remember` calls `/api/memory/remember`, `forget` tries memory first.
- `docker-compose.prod.yml` pinned to `v2.0.0` image tag (was `:latest`).

---

## [1.0.0] — 2026-06-01

### Added

- **Core RAG system** — hybrid search (pgvector semantic + BM25 full-text + RRF fusion), Small-to-Big chunking, local cross-encoder reranker
- **Supported formats** — PDF, DOCX, MD, TXT, PPTX, XLSX/XLS, EPUB via MarkItDown adapter
- **Async indexing** — uploads return immediately, background indexing via PostgreSQL queue
- **Multilingual** — EN + RU in a single corpus
- **LLM streaming** — Server-Sent Events for real-time responses (ADR-0014)
- **Configurable LLM** — Claude or GPT-4o selectable via `LLM_PROVIDER` env variable
- **Web UI** — dark sidebar layout, chat-style search, drag-and-drop upload with live progress, document list with HTMX polling
- **REST API** — `/api/documents`, `/api/query`, `/api/search/chunks`, `/api/jobs`
- **MCP server** — `remember`, `recall`, `index_file`, `check_indexing`, `list_memories`, `forget` tools for Claude Code and Hermes
- **MCP memory tools** — `find_related`, `recall_related` for document linking by group (ADR-0015)
- **Hermes integration** — one-command installer (`install-hermes.sh`), standalone MCP bridge, Hermes skill file, Docker Compose overlay
- **Docker** — production-ready image on GHCR, CPU-optimised (no CUDA), reranker model pre-downloaded at build time
- **CI/CD** — GitHub Actions: unit tests on push, Docker image build and push on tag
- **Architecture docs** — 15 ADRs, C4 diagrams, AGENTS.md contract

### Fixed

- SET NULL on `ingestion_jobs.doc_id` when a document is deleted
- Starlette 1.2 `TemplateResponse` API compatibility
- CPU-only torch in Docker (avoids 870 MB CUDA download)
- Session management and concurrent DB operations
- Deduplicate source tags by `doc_id` in search results

## [0.1.0] — 2026-05-31

Initial public release.
