# AGENTS.md — Архитектурный контракт Memex

> Читай этот файл перед тем как предлагать любой код или архитектурное изменение.
> Здесь — принятые решения, границы модулей и явные запреты.

---

## Что такое Memex

Personal RAG система. Один пользователь. Индексирует личные документы разных форматов, отвечает на вопросы на естественном языке (RU + EN).

**Поддерживаемые форматы:** PDF, DOCX, MD, TXT, PPTX, XLSX/XLS, EPUB.

**Интерфейсы:** REST API (FastAPI) + Web UI (Jinja2 + HTMX) + MCP Server (для Claude Code и других AI-клиентов).

---

## Стек

| Компонент | Технология |
|-----------|-----------|
| Язык | Python 3.12, **весь код async** |
| API фреймворк | FastAPI |
| ORM | SQLAlchemy 2.0 async + asyncpg |
| Миграции | Alembic |
| Конфигурация | Pydantic Settings (`src/config.py`) |
| База данных | PostgreSQL 15 + pgvector |
| Полнотекстовый поиск | PostgreSQL tsvector (встроен, не Elasticsearch) |
| Фоновые задачи | asyncio task + PostgreSQL как очередь (`ingestion_jobs`) |
| Embedding | OpenAI text-embedding-3-small (API) |
| Reranker | cross-encoder/ms-marco-MiniLM-L-6-v2 (локально, sentence-transformers) |
| LLM | Claude / GPT-4o (конфигурируемо через `LLM_PROVIDER` env) |
| UI | FastAPI + Jinja2 + HTMX |
| Тесты | pytest + testcontainers-python |

**Не добавляй** в стек без явного запроса: Redis, Elasticsearch, Celery, отдельный message broker, Weaviate, Qdrant, Pinecone.

**Весь код async.** Синхронные операции (sentence-transformers) — через `asyncio.run_in_executor`.

---

## Структура проекта

```
src/
├── api/           ← REST API: JSON endpoints (/api/*), DI, request/response models
├── ui/            ← Web UI: Jinja2 шаблоны, HTMX роуты (/, /documents, /upload)
├── mcp/           ← MCP Server: обёртка над api/ инструментами
├── adapters/      ← Adapter Layer: AdapterRegistry + адаптеры по форматам
├── ingestion/     ← Ingestion Pipeline: Chunker, EmbeddingStage, IndexingStage
├── retrieval/     ← Retrieval Pipeline: Search, RRF, Expand, Reranker, ContextBuilder
├── llm/           ← LLM Provider abstraction: Protocol, ClaudeProvider, OpenAIProvider
├── models/        ← Shared data models: Document, Chunk, ParsedDocument, Section
└── db/            ← Database: connection, migrations, repository classes
```

**Правила:**
- Новый файл идёт в модуль который соответствует его pipeline-шагу
- Не клади бизнес-логику в `api/` или `ui/`
- `ui/` и `api/` — разные роуты, одни и те же сервисы из `ingestion/` и `retrieval/`
- `ui/` НЕ вызывает `api/` напрямую — оба вызывают сервисы

---

## Границы модулей и направление зависимостей

```
api/ → ingestion/          (вызывает при POST /api/documents)
api/ → retrieval/          (вызывает при POST /api/query)
ui/  → ingestion/          (загрузка файлов через форму)
ui/  → retrieval/          (поиск через форму)
mcp/ → api/                (тонкая обёртка, не дублирует логику)

adapters/  → models/       (знает о ParsedDocument, Section)
ingestion/ → adapters/     (использует AdapterRegistry)
ingestion/ → models/       (знает о Chunk, Document)
retrieval/ → models/       (знает о Chunk)
retrieval/ → db/           (читает chunks, documents)
retrieval/ → llm/          (ContextBuilder вызывает LLMProvider)

llm/       → models/       (знает о LLMResponse)

adapters/  НЕ знает об ingestion/
ingestion/ НЕ знает о retrieval/
retrieval/ НЕ знает об ingestion/
ui/        НЕ вызывает api/ напрямую
```

**Нарушение зависимостей — ошибка архитектуры.** Если нужна связь через границу — используй shared models или events.

---

## Ключевые архитектурные решения

Перед изменением любого из этих компонентов — прочитай соответствующий ADR в `docs/architecture/adr/`.

### Adapter Layer (ADR-0001, ADR-0013)
- Каждый адаптер — класс с `can_handle(source) → bool` и `parse(source) → ParsedDocument`
- `ParsedDocument` содержит `sections: list[Section]` — структуру документа, не сырой текст
- `AdapterRegistry` итерирует адаптеры по порядку регистрации, первый подходящий wins
- **Не возвращай** из адаптера сырую строку — только `ParsedDocument`

**Порядок регистрации в registry строго фиксирован** (менять только через ADR):
```
1. PdfAdapter        ← pypdf, сохраняет page_number — критично для citations
2. DocxAdapter       ← python-docx, сохраняет heading levels
3. MarkdownAdapter   ← regex по заголовкам
4. TextAdapter       ← plain text, один Section
5. MarkItDownAdapter ← Microsoft MarkItDown, fallback: PPTX, XLSX, XLS, EPUB
```

**Правило MarkItDownAdapter (ADR-0013):** используется только как fallback для форматов без нативного адаптера. PdfAdapter и DocxAdapter на нативных библиотеках сохраняют структуру лучше. `page_number` у PPTX/XLSX/EPUB всегда `None` — эти форматы не имеют стабильной пагинации.

### Chunking — Small-to-Big (ADR-0002)
- Два уровня: **L2** (~512 токенов, `chunk_role='parent'`) и **L1** (~128 токенов, `chunk_role='leaf'`)
- L1 хранит `parent_chunk_id → L2`
- `content_vector` (embedding) — **только у L1**
- `tsv` (tsvector) — у обоих уровней
- **Не индексируй** L2 в pgvector — поиск идёт по L1, expand к L2 при retrieval
- Overlap: L2 с overlap 64 токена, L1 без overlap

### Retrieval Pipeline (ADR-0003)
Порядок шагов строго фиксирован:
```
QueryProcessor → SemanticSearch + BM25Search (параллельно)
→ RRF Merger (k=60) → Expand to L2 → Reranker → ContextBuilder → LLM
```
- **Не меняй** порядок шагов без ADR
- Reranker запускается **после** Expand (видит L2, не L1)
- RRF k=60 — не менять без измерений recall

### Embedding (ADR-0004)
- Модель: `text-embedding-3-small`, размерность 1536
- Батчинг при индексации обязателен (до 2048 текстов за раз)
- При недоступности API — возвращать ошибку, не падать тихо

### UI — FastAPI + Jinja2 + HTMX (ADR-0007)
- `src/ui/` — отдельный модуль с Jinja2 шаблонами
- Роуты UI: `GET /`, `GET /documents`, `POST /upload`
- Роуты API: `GET /api/documents`, `POST /api/documents`, `POST /api/query`
- HTMX делает частичные обновления — результаты поиска без reload страницы
- **Не дублируй** бизнес-логику между `ui/` и `api/` — оба вызывают одни сервисы

### LLM Provider Abstraction (ADR-0008)
- `src/llm/protocol.py` — `LLMProvider` Protocol + `LLMResponse` dataclass
- `ContextBuilder` зависит **только** от `LLMProvider` протокола, не от конкретного SDK
- Провайдер выбирается через env: `LLM_PROVIDER=claude|openai`
- **Не импортируй** `anthropic` или `openai` SDK вне `src/llm/`
- Для тестов — `MockProvider` без API вызовов

### Reranker (ADR-0005)
- Модель грузится **один раз** при старте сервиса, не при каждом запросе
- Запускается на L2 чанках (не L1)
- Возвращает top-3-5 чанков

### Мультиязычность (ADR-0006)
- Language detection — на уровне каждого **чанка**, не документа
- `chunk.language` → маппинг в PostgreSQL конфиг: `ru→russian`, `en→english`, `*→simple`
- `tsv` — не GENERATED ALWAYS, вычисляется в коде при INSERT с правильным языковым конфигом
- При поиске: определять язык запроса → использовать тот же конфиг для `plainto_tsquery`

---

## Схема данных — инварианты

```sql
-- Эти инварианты должны соблюдаться всегда:

-- L1 чанк всегда имеет parent
chunks WHERE chunk_role = 'leaf'   → parent_chunk_id IS NOT NULL

-- L2 чанк никогда не имеет parent
chunks WHERE chunk_role = 'parent' → parent_chunk_id IS NULL

-- embedding только у L1
chunks WHERE chunk_role = 'leaf'   → content_vector IS NOT NULL
chunks WHERE chunk_role = 'parent' → content_vector IS NULL

-- оба уровня индексированы для BM25
chunks → tsv IS NOT NULL
```

**Не нарушай эти инварианты** в миграциях и в коде.

---

## Async — правила

- Все endpoint функции — `async def`
- Все репозитории — `async def` с `AsyncSession`
- Все внешние HTTP вызовы — через `httpx.AsyncClient`, не `requests`
- `sentence-transformers` (reranker) — синхронная библиотека, всегда оборачивать: `await asyncio.get_event_loop().run_in_executor(None, reranker.rerank, ...)`
- `asyncio.sleep` вместо `time.sleep`

## Фоновая индексация (ADR-0009)

- `POST /upload` → сохранить файл → `INSERT ingestion_jobs` → вернуть `202 {job_id}`
- `IngestionWorker` — asyncio task, запускается через FastAPI `lifespan`
- `SELECT FOR UPDATE SKIP LOCKED` — атомарный захват задачи
- `GET /api/jobs/{job_id}` — проверка статуса
- Файлы сейчас — локальный диск (`settings.upload_dir`). Путь к S3: менять только upload логику.

## Конфигурация (ADR-0011)

- Единый `src/config.py` с `Settings(BaseSettings)`
- Никаких `os.environ.get()` вне `config.py`
- `.env` для локальной разработки, `.env.example` в репо
- `Settings` передаётся через FastAPI DI (`Depends(get_settings)`)

## Тестирование (ADR-0012)

- Интеграционные тесты — с реальным PostgreSQL через `testcontainers`
- **Никаких моков репозиториев** в интеграционных тестах
- `MockLLMProvider` и `MockEmbeddingClient` — чтобы не тратить API токены
- Маркеры: `@pytest.mark.unit`, `@pytest.mark.integration`, `@pytest.mark.e2e`
- E2E тесты пропускаются без `OPENAI_API_KEY` в env

## Non-Goals (MVP)

Не строится в текущей итерации. **Не добавляй без явного запроса:**

- Аутентификация и авторизация
- Мультиарендность (несколько пользователей)
- Веб-интерфейс (только API + MCP)
- Автоматическая переиндексация по расписанию
- Поддержка URL / веб-скрапинг (только локальные файлы)
- Streaming ответов LLM
- Query decomposition (multi-hop вопросы)
- Contextual Enrichment (LLM-prefix на чанках при индексации)
- Docker / Kubernetes оркестрация

---

## Идемпотентность индексации

При повторной загрузке документа:
1. Проверяем `documents.checksum`
2. Если не изменился — пропускаем (возвращаем существующий `doc_id`)
3. Если изменился — удаляем все старые чанки документа, индексируем заново

**Не создавай** дубликаты чанков для одного документа.

---

## Ссылки на архитектурные артефакты

- ADR: `docs/architecture/adr/` — 13 принятых решений с контекстом и альтернативами
- C4 Level 1: `docs/architecture/c4/01-system-context.md`
- C4 Level 2: `docs/architecture/c4/02-containers.md`
- Анализ паттернов chunking: `docs/superpowers/specs/2026-05-28-rag-chunking-patterns.md`
- Анализ adapter/RAG архитектуры: `docs/superpowers/specs/2026-05-28-rag-architecture-analysis.md`
