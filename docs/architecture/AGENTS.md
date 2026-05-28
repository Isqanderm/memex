# AGENTS.md

Этот файл читается Claude Code, Cursor, Copilot при старте.
Здесь — архитектурные ограничения, которые LLM **не должен нарушать**.

---

## Архитектурные принципы

- Модульный монолит: `schema/`, `domain/`, `infrastructure/`, `apps/`
- Схема БД не импортирует SQLAlchemy-модели — только чистые структуры
- Репозитории — только CRUD, без бизнес-логики
- Интеллект (retrieval, ranking, chunking) — в `domain/retrieval/`

## Технические ограничения

- FastAPI + async SQLAlchemy + pgvector
- Не добавлять новые зависимости без явного указания
- Все внешние интеграции — через Connector SDK-паттерн
- Тесты: testcontainers для интеграционных, PostgreSQL 16

## Явные НЕ-цели

- Не микросервисы на старте
- Не Kubernetes — Docker Compose достаточно
- Не real-time коллаборация

## Принятые решения (ADR)

См. `docs/architecture/adr/` — перед генерацией кода прочитай релевантный ADR.
