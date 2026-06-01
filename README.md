# Memex

Personal RAG system — indexes your documents and answers questions about them in natural language (EN + RU).

## Features

- **Supported formats:** PDF, DOCX, MD, TXT, PPTX, XLSX/XLS, EPUB
- **Hybrid search:** semantic search (pgvector) + full-text (BM25) + RRF fusion
- **Smart chunking:** Small-to-Big — retrieval over small chunks, LLM receives full parent context
- **Local reranker:** cross-encoder, no extra API calls
- **Multilingual:** EN + RU in a single corpus
- **Async indexing:** uploads return immediately, indexing runs in the background via a PG queue
- **Configurable LLM:** Claude or GPT-4o via env variable
- **Interfaces:** Web UI + REST API + MCP server for Claude Code

## Quick start

```bash
# 1. Start PostgreSQL
docker compose up -d postgres

# 2. Apply migrations
alembic upgrade head

# 3. Configure
cp .env.example .env
# Fill in OPENAI_API_KEY and/or ANTHROPIC_API_KEY

# 4. Run
uvicorn src.main:app --reload
# → http://localhost:8000
```

## Or with Docker Compose (full stack)

```bash
cp .env.example .env   # fill in API keys
docker compose up
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

Available tools: `add_document`, `query`, `find_related`, `recall_related`.

## Architecture

```
Ingestion:  Source → Adapter → ParsedDocument → Chunker (L2+L1) → Embed → PostgreSQL
Retrieval:  Query → Semantic + BM25 → RRF → Expand L2 → Reranker → LLM → Answer
```

Details: [`docs/architecture/AGENTS.md`](docs/architecture/AGENTS.md) — architectural contract for developers and LLM tools.

ADRs: [`docs/architecture/adr/`](docs/architecture/adr/) — 15 accepted architecture decision records.

## Development

```bash
pip install -e ".[dev]"

# Unit tests (no Docker required)
pytest tests/unit/ -v

# Integration tests (requires Docker)
pytest tests/integration/ -v -m integration
```

## Stack

Python 3.12 · FastAPI · SQLAlchemy 2.0 async · PostgreSQL 15 + pgvector · Alembic · OpenAI Embeddings · sentence-transformers · Anthropic / OpenAI · Jinja2 + HTMX · MCP

## License

MIT — see [LICENSE](LICENSE).
