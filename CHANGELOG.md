# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
