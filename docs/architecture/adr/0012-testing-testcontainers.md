# ADR-0012: Стратегия тестирования — testcontainers + pytest

**Статус:** accepted
**Дата:** 2026-05-29
**Автор:** Александр Мельник

---

## Контекст

Нужна стратегия тестирования для системы с PostgreSQL + pgvector в критическом пути. Выбор: мокать БД или тестировать с реальным PostgreSQL.

## Рассматриваемые варианты

### Вариант 1: Моки БД

Репозитории заменяются на in-memory заглушки в тестах.

**Плюсы:** быстрые тесты, нет зависимости от Docker.
**Минусы:** pgvector запросы, tsvector, `FOR UPDATE SKIP LOCKED` — невозможно замокать корректно. Тесты проходят, в проде падает.
**Риски:** ложное ощущение покрытия.

### Вариант 2: testcontainers-python + pytest

Реальный PostgreSQL в Docker-контейнере запускается на время тестов через `testcontainers-python`. Каждый тестовый запуск — чистая БД.

**Плюсы:**
- Тесты проверяют реальные SQL запросы, pgvector, tsvector
- pgvector extension устанавливается в контейнере автоматически
- Изоляция: каждый тест в транзакции, откат после теста
**Минусы:**
- Требует Docker
- Первый запуск медленнее (pull образа)
**Риски:**
- CI должен поддерживать Docker (стандарт для GitHub Actions)

## Решение

Выбрал **Вариант 2: testcontainers**.

Система завязана на специфику PostgreSQL (pgvector, tsvector, SKIP LOCKED). Моки не дадут уверенности. Testcontainers — стандартный подход для таких случаев.

**Уровни тестирования:**

```
Unit тесты (без БД):
  - Chunker логика (нарезка, overlap)
  - RRF Merger (математика слияния)
  - ContextBuilder (сборка промпта)
  - LLM MockProvider
  → pytest, без Docker

Интеграционные тесты (с реальным PostgreSQL):
  - AdapterRegistry + парсинг реальных файлов
  - Ingestion Pipeline (end-to-end: файл → чанки в БД)
  - SemanticSearch + BM25Search (реальные запросы)
  - Retrieval Pipeline (end-to-end: запрос → ответ без LLM)
  → pytest + testcontainers

E2E тесты (с реальными API — только в CI):
  - Embedding через OpenAI
  - LLM ответ
  → помечены @pytest.mark.e2e, пропускаются без API ключей
```

**Фикстуры:**
```python
@pytest.fixture(scope="session")
async def pg_container():
    with PostgresContainer("pgvector/pgvector:pg15") as pg:
        yield pg  # один контейнер на все тесты сессии

@pytest.fixture(autouse=True)
async def db_transaction(db_session):
    async with db_session.begin():
        yield
        await db_session.rollback()  # откат после каждого теста
```

## Последствия

**Придётся:**
- `tests/conftest.py` — фикстуры для контейнера и сессии
- `MockLLMProvider` и `MockEmbeddingClient` в `tests/mocks/`
- Docker обязателен для запуска интеграционных тестов
- `pytest.ini` или `pyproject.toml` с маркерами: `unit`, `integration`, `e2e`

**Правила:**
- Никаких моков репозиториев в интеграционных тестах
- `MockProvider` для LLM — чтобы не тратить API токены на тесты
- `MockEmbeddingClient` — возвращает случайные векторы правильной размерности
