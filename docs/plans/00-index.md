# Memex Rust Migration — Индекс плана

> **Для агентных исполнителей:** ОБЯЗАТЕЛЬНЫЙ SUB-SKILL: `superpowers:subagent-driven-development` (рекомендуется) или `superpowers:executing-plans`.

**Цель:** Полная замена Python-приложения на Rust с переходом с PostgreSQL+pgvector на SQLite+sqlite-vec+tantivy. Нулевая регрессия функциональности.

**Принципы:** TDD · DRY · YAGNI · Частые коммиты · Без placeholder-ов

---

## Архитектура результата

```
rust/                  ← новый Rust-проект внутри того же репо
  Cargo.toml
  src/
    main.rs            ← точка входа, axum сервер
    config.rs          ← Settings (аналог pydantic-settings)
    error.rs           ← AppError (thiserror)
    db/                ← SQLite: пул, схема, репозитории
    search/            ← Поиск: векторный, BM25, RRF, reranker, контекст
    ingestion/         ← Ingestion: адаптеры, чанкер, эмбеддинги, воркер
    llm/               ← LLM клиент: Claude + OpenAI (reqwest)
    memory/            ← Память: извлечение фактов, сервис, профиль
    api/               ← HTTP обработчики (axum)

mcp_server.py          ← НЕ ТРОГАТЬ: уже HTTP-клиент к приложению
templates/             ← НЕ ТРОГАТЬ: копируются в rust/templates/
static/                ← НЕ ТРОГАТЬ: копируются в rust/static/
```

## Стек

| Слой | Python (было) | Rust (будет) |
|---|---|---|
| HTTP | FastAPI + uvicorn | `axum 0.7` |
| БД | SQLAlchemy + asyncpg + PostgreSQL | `rusqlite 0.31` + `r2d2_sqlite` + SQLite WAL |
| Векторный поиск | pgvector | `sqlite-vec 0.1` (HNSW) |
| Полнотекстовый поиск | PostgreSQL tsvector | `tantivy 0.22` (многоязычный BM25) |
| Эмбеддинги | sentence-transformers (PyTorch) | `fastembed 4` (ONNX) |
| Ранжировщик | CrossEncoder (PyTorch) | `fastembed` (bge-reranker-base ONNX) |
| LLM клиент | anthropic/openai SDK | `reqwest` (plain HTTP) |
| Шаблоны | Jinja2 | `minijinja 2` |
| Определение языка | langdetect | `whichlang 0.1` |
| PDF парсинг | pypdf | subprocess `pdftotext` |
| DOCX парсинг | python-docx | `zip` + `quick-xml` |
| XLSX парсинг | markitdown | `calamine 0.25` |
| PPTX парсинг | markitdown | `zip` + `quick-xml` |
| Markdown | str.split | `pulldown-cmark 0.12` |

## Хранилища данных

```
data/
  memex.db        ← SQLite WAL: все таблицы (docs, chunks, jobs, memories)
  tantivy/        ← tantivy FTS индекс (BM25, многоязычный)
```
sqlite-vec хранит векторы в виртуальных таблицах внутри memex.db.

**Backup = `cp -r data/ backup/`**

Если tantivy-директория потеряна → rebuild из `chunks.content` в SQLite.

---

## Файлы плана

| Файл | Что строим |
|---|---|
| [01-scaffold.md](01-scaffold.md) | Cargo.toml, main.rs, config.rs, error.rs |
| [02-database.md](02-database.md) | SQLite пул, схема, SQL миграции, базовые репозитории |
| [03-vector-search.md](03-vector-search.md) | sqlite-vec: VectorStore, CRUD, HNSW поиск |
| [04-fts-tantivy.md](04-fts-tantivy.md) | tantivy: multilingual BM25, индексация, поиск |
| [05-embeddings.md](05-embeddings.md) | fastembed: эмбеддинги (e5-small) + reranker (bge) |
| [06-document-adapters.md](06-document-adapters.md) | PDF, DOCX, XLSX, PPTX, Markdown, Text адаптеры |
| [07-ingestion.md](07-ingestion.md) | Chunker, Language, EmbeddingStage, IndexingStage, Pipeline, Worker |
| [08-retrieval.md](08-retrieval.md) | SemanticSearch, RRF, L2 expand, RetrievalService |
| [09-llm.md](09-llm.md) | LlmProvider, Claude, OpenAI (reqwest + streaming) |
| [10-memory.md](10-memory.md) | MemoryRepo, FactExtractor, MemoryService, ProfileService |
| [11-api.md](11-api.md) | Все axum маршруты и обработчики |
| [12-ui.md](12-ui.md) | UI обработчики, minijinja, SSE стриминг |
| [13-migration.md](13-migration.md) | Миграция данных PostgreSQL → SQLite |
| [14-deployment.md](14-deployment.md) | Dockerfile для ARM/RPi, cross-compilation |

---

## Порядок выполнения

```
01 → 02 → 03 → 04   (фундамент: scaffold + хранилища)
         ↓
        05           (модели: embeddings)
         ↓
     06 → 07         (ingestion pipeline)
         ↓
     08 → 09 → 10   (retrieval + LLM + memory)
         ↓
     11 → 12        (HTTP API + UI)
         ↓
     13 → 14        (migration + deployment)
```

Каждый этап даёт рабочий, тестируемый артефакт.
