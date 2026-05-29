# Memex

Personal RAG система — индексирует личные документы и отвечает на вопросы по ним на естественном языке (RU + EN).

## Возможности

- **Поддерживаемые форматы:** PDF, DOCX, MD, TXT, PPTX, XLSX/XLS, EPUB
- **Hybrid Search:** семантический поиск (pgvector) + полнотекстовый (BM25) + RRF слияние
- **Smart chunking:** Small-to-Big — поиск по маленьким чанкам, LLM получает полный контекст
- **Reranker:** локальный cross-encoder без API вызовов
- **Мультиязычность:** RU + EN в одном корпусе
- **Async indexing:** загрузка файлов не блокирует — индексация в фоне через PG очередь
- **LLM на выбор:** Claude или GPT-4o через env переменную
- **Интерфейсы:** Web UI + REST API + MCP сервер для Claude Code

## Быстрый старт

```bash
# 1. Запустить PostgreSQL
docker compose up -d postgres

# 2. Применить миграции
alembic upgrade head

# 3. Скопировать конфиг
cp .env.example .env
# Заполнить OPENAI_API_KEY, ANTHROPIC_API_KEY

# 4. Запустить
uvicorn src.main:app --reload
# → http://localhost:8000
```

## Или через Docker Compose

```bash
cp .env.example .env  # заполнить ключи
docker compose up
```

## Использование

### Web UI

Открыть `http://localhost:8000` — форма загрузки и поиска.

### REST API

```bash
# Загрузить документ
curl -F "file=@report.pdf" http://localhost:8000/api/documents

# Проверить статус индексации
curl http://localhost:8000/api/jobs/{job_id}

# Задать вопрос
curl -X POST http://localhost:8000/api/query \
  -H "Content-Type: application/json" \
  -d '{"query": "о чём этот документ?"}'
```

### MCP (Claude Code)

Добавить в `.claude/settings.json`:

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

Инструменты: `add_document`, `query`.

## Архитектура

```
Ingestion:  Source → Adapter → ParsedDocument → Chunker (L2+L1) → Embed → PostgreSQL
Retrieval:  Query → Semantic + BM25 → RRF → Expand L2 → Reranker → LLM → Answer
```

Подробно: [`docs/architecture/AGENTS.md`](docs/architecture/AGENTS.md) — архитектурный контракт для разработчиков и LLM-инструментов.

ADR: [`docs/architecture/adr/`](docs/architecture/adr/) — 13 принятых архитектурных решений.

## Разработка

```bash
pip install -e ".[dev]"

# Unit тесты (без Docker)
pytest tests/unit/ -v

# Интеграционные тесты (нужен Docker)
pytest tests/integration/ -v -m integration
```

## Стек

Python 3.12 · FastAPI · SQLAlchemy 2.0 async · PostgreSQL 15 + pgvector · Alembic · OpenAI Embeddings · sentence-transformers · Anthropic / OpenAI · Jinja2 + HTMX · MCP
