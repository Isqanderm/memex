# Architecture Decision Records

Как использовать:
1. Скопируй `0000-template.md` → `0001-краткое-название.md`
2. Заполни секции
3. Это живой документ — дополни «Последствия» через неделю

Нумерация последовательная. Не удаляй старые решения — они объясняют контекст будущим читателям (и LLM).

## Статусы решений

- `proposed` — ты предложил, ещё думаешь
- `accepted` — принял, так и делаем
- `superseded` — заменено более новым ADR (укажи каким)
- `deprecated` — решение больше не релевантно

## Принятые решения

| # | Решение | Статус |
|---|---------|--------|
| [0001](0001-adapter-layer-protocol-registry.md) | Adapter Layer — Protocol-based Registry | accepted |
| [0002](0002-chunking-strategy-small-to-big.md) | Chunking Strategy — Small-to-Big (L1=128, L2=512 tok) | accepted |
| [0003](0003-retrieval-hybrid-search.md) | Retrieval — Hybrid Search (Semantic + BM25 + RRF) | accepted |
| [0004](0004-embedding-model-openai.md) | Embedding Model — OpenAI text-embedding-3-small | accepted |
| [0005](0005-reranker-local-cross-encoder.md) | Reranker — Локальный cross-encoder ms-marco | accepted |
| [0006](0006-multilingual-language-detection.md) | Мультиязычность — Language Detection per Chunk | accepted |
| [0007](0007-ui-fastapi-jinja2-htmx.md) | UI — FastAPI + Jinja2 + HTMX | accepted |
| [0008](0008-llm-provider-abstraction.md) | LLM Provider Abstraction — Custom Protocol | accepted |
| [0009](0009-async-ingestion-pg-queue.md) | Async Ingestion — PostgreSQL как очередь задач | accepted |
| [0010](0010-orm-sqlalchemy-alembic.md) | ORM и Миграции — SQLAlchemy 2.0 async + Alembic | accepted |
| [0011](0011-configuration-pydantic-settings.md) | Конфигурация — Pydantic Settings | accepted |
| [0012](0012-testing-testcontainers.md) | Тестирование — testcontainers + pytest | accepted |
