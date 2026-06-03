# ADR-0016: Local Embedding Model — intfloat/multilingual-e5-small

**Статус:** accepted  
**Дата:** 2026-06-02  
**Автор:** Александр Мельник  
**Supersedes:** ADR-0004

---

## Контекст

ADR-0004 выбрал `text-embedding-3-small` от OpenAI (API). После добавления memory layer и анализа production-данных появились основания для пересмотра:

- Latency embedding через API: **300–1400ms** per request (зависит от сети)
- Memory layer вызывает `embed()` при каждом `remember()` — latency критична
- OPENAI_API_KEY как hard requirement усложняет self-hosted deployment
- Стоимость: $0.020/1M tokens — незначительна, но добавляет operational dependency

## Решение

Перейти на **`intfloat/multilingual-e5-small`** — локальная sentence-transformers модель.

| Параметр | Старое (ADR-0004) | Новое |
|---|---|---|
| Модель | text-embedding-3-small | intfloat/multilingual-e5-small |
| Провайдер | OpenAI API | локально (sentence-transformers) |
| Размерность | 1536 | **384** |
| Latency (warm) | 300–1400ms | **~50ms** |
| Стоимость | $0.020/1M tokens | **$0** |
| API key | обязателен | не нужен |
| Размер модели | 0 MB (in cloud) | **117 MB** (в образе) |

## Почему multilingual-e5-small

- Поддерживает RU + EN нативно (обучена на 100+ языках)
- MTEB retrieval score ~62 — сравнимо с text-embedding-3-small (~62.3)
- e5-style prefixes: `"query: "` для запросов, `"passage: "` для документов — критично для качества retrieval
- 117MB — помещается в Docker image без проблем

## Технические последствия

1. **Миграция 0004**: resize `content_vector` с 1536 до 384 dims в таблицах `chunks` и `memories`
2. После применения миграции — все векторы обнуляются. Требуется `scripts/reindex.py`
3. `LocalEmbeddingClient` в `src/ingestion/embedding.py` — singleton, загружается при старте
4. `OPENAI_API_KEY` больше не нужен для embeddings — убран из `Settings` как required

## Риски

- **CPU latency**: 50ms warm, но модель запускается в thread executor — не блокирует event loop
- **Качество**: на специализированных доменах может уступать GPT-API моделям
- **Размер образа**: +117MB в Docker image

## Альтернативы, которые отвергнуты

- **BAAI/bge-m3** (1024 dims, 570MB) — лучше качество, но слишком тяжёлый
- **all-MiniLM-L6-v2** (22MB) — не поддерживает RU нормально
- **Оставить OpenAI** — latency неприемлема для memory layer
