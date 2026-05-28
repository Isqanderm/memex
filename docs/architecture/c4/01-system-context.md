# C4 Model — Уровень 1: System Context

Кто взаимодействует с системой и через какие интерфейсы.

```mermaid
graph TB
    User["👤 Пользователь
    ──────────────
    Александр
    Загружает документы,
    задаёт вопросы на
    естественном языке"]

    Memex["🗂 Memex
    ──────────────
    Personal RAG System
    Индексирует личные
    документы, отвечает
    на вопросы по ним"]

    OpenAI_Embed["🔌 OpenAI Embeddings API
    ──────────────
    text-embedding-3-small
    Преобразует текст
    в векторы"]

    LLM["🤖 LLM API
    ──────────────
    Claude / GPT-4o
    Генерирует ответ
    на основе найденных
    чанков"]

    Claude_MCP["🔧 Claude Code / MCP Client
    ──────────────
    AI-ассистент с доступом
    к Memex как MCP-инструменту"]

    User -->|"POST /documents
    (загрузка файлов)"| Memex
    User -->|"POST /query
    (вопросы на RU/EN)"| Memex
    Claude_MCP -->|"MCP: add_document
    MCP: query"| Memex
    Memex -->|"embed(text) →
    vector[1536]"| OpenAI_Embed
    Memex -->|"prompt + context →
    answer"| LLM
```

## Внешние системы

| Система | Роль | Протокол |
|---------|------|----------|
| OpenAI Embeddings API | Преобразование текста в векторы при индексации и поиске | HTTPS / REST |
| LLM API (Claude/GPT-4o) | Генерация ответа по найденным чанкам | HTTPS / REST |
| Claude Code / MCP Client | AI-ассистент, использующий Memex как инструмент | MCP (stdin/stdout) |

## Пользователи

**Один пользователь — владелец системы.** Личный инструмент, нет мультиарендности, нет аутентификации в MVP.

## Ключевые потоки

1. **Индексация:** пользователь / MCP → `POST /documents` → Ingestion Pipeline → OpenAI Embed → PostgreSQL
2. **Поиск:** пользователь / MCP → `POST /query` → Retrieval Pipeline → OpenAI Embed → PostgreSQL → Reranker → LLM → ответ
