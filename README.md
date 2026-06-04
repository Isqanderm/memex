# Memex

**Self-hosted RAG for your documents.** Upload PDFs, DOCX, Markdown and more — ask questions in natural language, get answers with source references.

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Python](https://img.shields.io/badge/python-3.12-blue.svg)](https://www.python.org/downloads/)
[![Docker](https://img.shields.io/badge/docker-compose-2496ED?logo=docker&logoColor=white)](docker-compose.prod.yml)
[![FastAPI](https://img.shields.io/badge/FastAPI-0.115-009688?logo=fastapi&logoColor=white)](https://fastapi.tiangolo.com)
[![pgvector](https://img.shields.io/badge/pgvector-PostgreSQL-336791?logo=postgresql&logoColor=white)](https://github.com/pgvector/pgvector)
[![MCP](https://img.shields.io/badge/MCP-Claude_Code-orange)](https://docs.anthropic.com/en/docs/claude-code/mcp)

---

## Why Memex?

Most document Q&A tools are SaaS — your documents leave your machine. Memex runs entirely on your infrastructure.

| Feature | Memex | privateGPT | Quivr | Danswer |
|---|:---:|:---:|:---:|:---:|
| Self-hosted | ✅ | ✅ | ✅ | ✅ |
| One-command Docker install | ✅ | ❌ | ❌ | ❌ |
| Hybrid search (semantic + BM25) | ✅ | ❌ | ❌ | ✅ |
| Local reranker (no extra API) | ✅ | ❌ | ❌ | ❌ |
| MCP server for Claude Code | ✅ | ❌ | ❌ | ❌ |
| Persistent memory layer | ✅ | ❌ | ❌ | ❌ |
| Small-to-Big chunking | ✅ | ❌ | ❌ | ❌ |
| REST API | ✅ | ✅ | ✅ | ✅ |
| Web UI | ✅ | ✅ | ✅ | ✅ |

---

## Features

- **Supported formats:** PDF, DOCX, MD, TXT, PPTX, XLSX/XLS, EPUB
- **Hybrid search:** semantic (pgvector) + full-text (BM25) + RRF fusion
- **Smart chunking:** Small-to-Big — retrieval over small chunks, LLM receives full parent context
- **Local reranker:** cross-encoder, no extra API calls
- **Persistent memory:** extracts atomic facts from conversations, resolves conflicts, categorises by type (research / reminder / insight / decision / preference)
- **Multilingual:** EN + RU in a single corpus
- **Async indexing:** uploads return immediately, indexing runs in background via PG queue
- **Configurable LLM:** Claude or GPT-4o via env variable
- **Three interfaces:** Web UI + REST API + MCP server for Claude Code

## ⚠️ Upgrading from 1.x

Version 2.0 changes the embedding model (OpenAI → local `multilingual-e5-small`, 384 dims). **Existing vectors are incompatible** — migration 0004 NULLs them automatically. After upgrading you must re-index:

```bash
docker compose -f docker-compose.prod.yml pull && docker compose -f docker-compose.prod.yml up -d
docker exec memex alembic upgrade head
docker exec memex uv run python scripts/reindex.py   # restores search
```

Remove from `.env`: `OPENAI_API_KEY`, `EMBEDDING_MODEL`, `EMBEDDING_DIMENSIONS` (no longer used for embeddings).

See [CHANGELOG.md](CHANGELOG.md) for full breaking changes.

---

## Quick start

```bash
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.prod.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/.env.example
cp .env.example .env  # fill in POSTGRES_PASSWORD and LLM credentials
docker compose -f docker-compose.prod.yml up -d
# → http://localhost:8000
```

## Usage

### Web UI

Open `http://localhost:8000` — upload documents and ask questions.

### REST API

```bash
# Upload a document
curl -F "file=@report.pdf" http://localhost:8000/api/documents

# Check indexing status
curl http://localhost:8000/api/jobs/{job_id}

# Ask a question
curl -X POST http://localhost:8000/api/query \
  -H "Content-Type: application/json" \
  -d '{"query": "what is this document about?"}'
```

### MCP (Claude Code)

Add to `.claude/settings.json`:

```json
{
  "mcpServers": {
    "memex": {
      "command": "python3",
      "args": ["mcp_server.py"],
      "cwd": "/path/to/memex"
    }
  }
}
```

**Memory tools:** `remember` · `recall` · `context` · `observe` · `memories` · `forget`

**Document tools:** `index_file` · `check_indexing` · `list_memories`

## Hermes Integration

Use Memex as persistent memory for a Hermes agent.

```bash
OPENAI_LLM_API_KEY=sk-... bash <(curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-hermes.sh)
```

Auto-detects your Hermes container and network, starts Memex, installs the MCP bridge and skill, patches `config.yaml`, and restarts Hermes. Manual setup: [`docs/hermes.md`](docs/hermes.md).

## Architecture

```
Ingestion:  Source → Adapter → ParsedDocument → Chunker (L2+L1) → Embed → PostgreSQL
Retrieval:  Query → Semantic + BM25 → RRF → Expand L2 → Reranker → LLM → Answer
```

Details: [`docs/architecture/AGENTS.md`](docs/architecture/AGENTS.md) — architectural contract for developers and LLM tools.

ADRs: [`docs/architecture/adr/`](docs/architecture/adr/) — 15 accepted architecture decision records.

## Development

```bash
uv sync --extra dev

# Lint
uv run ruff check src/ tests/

# Type check
uv run mypy src/

# Unit tests (no Docker required)
uv run pytest tests/unit/ -v

# Integration tests (requires Docker)
uv run pytest tests/integration/ -v -m integration
```

## Stack

Python 3.12 · FastAPI · SQLAlchemy 2.0 async · PostgreSQL 15 + pgvector · Alembic · sentence-transformers (local embeddings) · Anthropic / OpenAI · Jinja2 + HTMX · MCP

## License

MIT — see [LICENSE](LICENSE).
