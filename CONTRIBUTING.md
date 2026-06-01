# Contributing to Memex

Thank you for your interest in contributing! Here's everything you need to get started.

## Prerequisites

- Python 3.12+
- Docker (for PostgreSQL and integration tests)
- An OpenAI API key (for embeddings); optionally an Anthropic API key (for Claude LLM)

## Local setup

```bash
# Clone the repo
git clone https://github.com/Isqanderm/memex.git
cd memex

# Install with dev dependencies
pip install -e ".[dev]"

# Copy and fill in config
cp .env.example .env

# Start PostgreSQL
docker compose up -d postgres

# Apply migrations
alembic upgrade head

# Run the app
uvicorn src.main:app --reload
# → http://localhost:8000
```

## Running tests

```bash
# Unit tests — no Docker needed, run fast
pytest tests/unit/ -v

# Integration tests — require a running PostgreSQL container
docker compose up -d postgres
pytest tests/integration/ -v -m integration
```

## Project structure

```
src/
├── api/          REST API endpoints (/api/*)
├── ui/           Web UI routes + Jinja2 templates
├── mcp/          MCP server tools (add_document, query, find_related, recall_related)
├── adapters/     File format adapters (PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB)
├── ingestion/    Chunker, embedding stage, indexing stage
├── retrieval/    Hybrid search (semantic + BM25 + RRF), reranker
├── llm/          LLM provider abstraction (Claude / OpenAI)
└── db/           SQLAlchemy models, repositories, migrations (Alembic)
```

Architecture decisions are documented in [`docs/architecture/adr/`](docs/architecture/adr/).
The architectural contract (boundaries, rules, stack) is in [`docs/architecture/AGENTS.md`](docs/architecture/AGENTS.md).

## Submitting a pull request

1. Fork the repo and create a branch from `main`
2. Make your changes with tests where applicable
3. Ensure all tests pass: `pytest tests/unit/ -v`
4. Open a PR with a clear description of what and why

## Adding a new file format adapter

1. Create `src/adapters/your_format_adapter.py` implementing the `DocumentAdapter` protocol
2. Register it in `src/adapters/registry.py`
3. Add unit tests in `tests/unit/adapters/`

## Questions

Open a GitHub issue — happy to discuss before you start on a larger change.
