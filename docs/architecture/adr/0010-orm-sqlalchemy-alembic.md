# ADR-0010: ORM и Миграции — SQLAlchemy 2.0 async + Alembic

**Статус:** accepted
**Дата:** 2026-05-29
**Автор:** Александр Мельник

---

## Контекст

Нужно выбрать как работать с PostgreSQL из Python: сырой SQL, query builder или ORM. И как управлять схемой БД — ручные скрипты или инструмент миграций.

## Рассматриваемые варианты

### Вариант 1: asyncpg + сырой SQL

Прямые SQL запросы через asyncpg, без ORM.

**Плюсы:** полный контроль над SQL, нет overhead ORM, максимальная производительность.
**Минусы:** нет управления миграциями из коробки, много boilerplate для CRUD.
**Риски:** ручные миграции легко рассинхронизируются со схемой.

### Вариант 2: SQLAlchemy 2.0 async + Alembic

ORM с async поддержкой через asyncpg под капотом. Alembic — стандартный инструмент миграций для SQLAlchemy.

**Плюсы:**
- Async из коробки (`async_sessionmaker`, `AsyncSession`)
- Alembic автогенерирует миграции из моделей
- Репозитории получаются чистые
- Широкая экосистема, хорошая документация
**Минусы:**
- Overhead ORM на сложных запросах (pgvector search, BM25) — решается через `text()` для raw SQL
- Более сложная настройка чем asyncpg напрямую

### Вариант 3: Tortoise ORM / SQLModel

Альтернативные async ORM.

**Плюсы:** меньше boilerplate чем SQLAlchemy.
**Минусы:** меньшая экосистема, хуже поддержка pgvector, меньше документации по Alembic интеграции.

## Решение

Выбрал **Вариант 2: SQLAlchemy 2.0 async + Alembic**.

SQLAlchemy — де-факто стандарт для FastAPI проектов. Async поддержка через `AsyncSession` и `asyncpg` покрывает требование всего кода быть async. Для сложных запросов (pgvector `<=>`, tsvector `@@`) используем `text()` с сырым SQL внутри ORM сессии — лучшее из двух миров.

**Паттерн репозиториев:** каждая сущность (`Document`, `Chunk`, `IngestionJob`) имеет свой репозиторий в `src/db/repositories/`. Бизнес-логика не знает об `AsyncSession` напрямую.

**Pgvector:** через `pgvector-sqlalchemy` расширение — добавляет `Vector` тип к SQLAlchemy моделям.

## Последствия

**Придётся:**
- `src/db/models.py` — SQLAlchemy модели (`Document`, `Chunk`, `IngestionJob`)
- `src/db/repositories/` — репозитории с async методами
- `src/db/session.py` — `async_sessionmaker`, dependency для FastAPI
- `alembic/` — директория миграций в корне проекта
- Зависимости: `sqlalchemy[asyncio]`, `asyncpg`, `alembic`, `pgvector-sqlalchemy`

**Правила:**
- pgvector и BM25 запросы — через `session.execute(text(...))` внутри репозитория
- `AsyncSession` никогда не пробрасывается выше репозитория
- Каждая миграция — атомарное изменение схемы
