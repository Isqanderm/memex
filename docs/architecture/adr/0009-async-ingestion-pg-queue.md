# ADR-0009: Асинхронная индексация — PostgreSQL как очередь задач

**Статус:** accepted
**Дата:** 2026-05-29
**Автор:** Александр Мельник

---

## Контекст

Индексация документа — тяжёлая операция: парсинг файла, нарезка на чанки, батч-запросы к OpenAI Embeddings API, вставка сотен строк в PostgreSQL. Для PDF в 50 страниц — 10-30 секунд. Блокировать HTTP-ответ на это время — плохой UX.

Нужно: пользователь загружает файл → немедленно получает ответ → индексация происходит в фоне → можно узнать статус.

В будущем: файлы переедут в S3, но это вне текущего скоупа.

## Рассматриваемые варианты

### Вариант 1: Синхронная индексация

`POST /upload` блокирует до конца индексации, возвращает результат.

**Плюсы:** просто, нет дополнительных компонентов.
**Минусы:** таймауты HTTP, плохой UX при больших файлах, нельзя загрузить несколько файлов параллельно.
**Риски:** при потере соединения — непонятно завершилась ли индексация.

### Вариант 2: Celery + Redis

Фоновые задачи через Celery, брокер сообщений Redis.

**Плюсы:** промышленное решение, retry из коробки, мониторинг через Flower.
**Минусы:** два новых компонента в стеке (Redis + Celery worker), избыточно для личного инструмента.
**Риски:** операционная сложность несоразмерна задаче.

### Вариант 3: PostgreSQL как очередь задач (рекомендуется)

Таблица `ingestion_jobs` в PostgreSQL хранит очередь. Background worker (asyncio task) поллит очередь и обрабатывает задачи. `SELECT ... FOR UPDATE SKIP LOCKED` — атомарный захват задачи.

**Плюсы:**
- Нет новых зависимостей — PostgreSQL уже есть
- Транзакционность: файл сохранён и задача создана атомарно
- Статус задачи виден через тот же PostgreSQL
- `SKIP LOCKED` — безопасный захват без дублирования
**Минусы:**
- Поллинг вместо push (задержка до 1-2 сек между проверками)
- Не масштабируется на тысячи задач в секунду (но личный проект)
**Риски:**
- Worker падает в середине задачи → задача зависает в `processing`. Решение: timeout + reset зависших задач при старте.

## Решение

Выбрал **Вариант 3: PostgreSQL как очередь**.

Нет смысла тащить Redis + Celery в личный проект. PostgreSQL уже есть, `SELECT FOR UPDATE SKIP LOCKED` даёт надёжный захват задачи. Задержка в 1-2 секунды до начала обработки — приемлема.

**Схема задачи:**
```sql
CREATE TABLE ingestion_jobs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    status      TEXT NOT NULL DEFAULT 'pending',
                -- pending | processing | done | error
    source      TEXT NOT NULL,      -- путь к файлу
    checksum    TEXT NOT NULL,      -- SHA-256 файла (для дедупликации)
    doc_id      UUID REFERENCES documents(id),
    error       TEXT,               -- сообщение об ошибке
    created_at  TIMESTAMPTZ DEFAULT now(),
    updated_at  TIMESTAMPTZ DEFAULT now()
);

-- Индекс для воркера
CREATE INDEX ON ingestion_jobs(status, created_at)
    WHERE status = 'pending';

-- Уникальный индекс для защиты от дублей в очереди
CREATE UNIQUE INDEX ON ingestion_jobs(checksum)
    WHERE status IN ('pending', 'processing');

-- documents тоже должен иметь UNIQUE на checksum
ALTER TABLE documents ADD COLUMN checksum TEXT UNIQUE NOT NULL;
```

**Поток загрузки с дедупликацией:**
```
POST /upload (file)
  │
  ├── 1. Сохранить во временный файл
  ├── 2. checksum = sha256(file_bytes)
  │
  ├── 3. SELECT id FROM documents WHERE checksum = $checksum
  │      → нашли: удалить temp файл
  │               вернуть 200 { doc_id, status: "already_indexed" }
  │
  ├── 4. SELECT id FROM ingestion_jobs
  │      WHERE checksum = $checksum AND status IN ('pending', 'processing')
  │      → нашли: удалить temp файл
  │               вернуть 202 { job_id, status: "already_queued" }
  │
  └── 5. Переименовать temp → финальный путь
         INSERT ingestion_jobs { checksum, source }
           ON CONFLICT DO NOTHING RETURNING id
         → conflict: вернуть существующий job (race condition)
         → ok: вернуть 202 { job_id }

Background worker (asyncio task, запускается при старте FastAPI):
  loop:
    SELECT id, source FROM ingestion_jobs
    WHERE status = 'pending'
    ORDER BY created_at
    LIMIT 1
    FOR UPDATE SKIP LOCKED
    → UPDATE status = 'processing'
    → запустить Ingestion Pipeline
    → UPDATE status = 'done' | 'error'
    → sleep 1s если нет задач

GET /api/jobs/{job_id} → { status, doc_id, error }
```

**Защита от race condition:** `INSERT ... ON CONFLICT DO NOTHING` + частичный уникальный индекс по `checksum WHERE status IN ('pending', 'processing')`. Два одновременных запроса с одним файлом — один вставит, второй получит conflict и вернёт существующий job.

**Хранение файлов (сейчас):** локальный диск, путь в `ingestion_jobs.source`.
**Хранение файлов (будущее):** S3 — меняется только место сохранения файла в `POST /upload` и чтение в `AdapterRegistry`. Очередь, worker и дедупликация не меняются.

**Весь код — async.** FastAPI async endpoints, asyncpg для PostgreSQL, httpx для OpenAI API. Синхронные операции (sentence-transformers reranker) — через `asyncio.run_in_executor`.

## Последствия

**Придётся:**
- Создать таблицу `ingestion_jobs` с `checksum` и частичным уникальным индексом
- Добавить `checksum TEXT UNIQUE` в таблицу `documents`
- Реализовать `IngestionWorker` (asyncio task) в `src/ingestion/worker.py`
- Вычислять SHA-256 при каждом upload до создания job
- Запускать worker через FastAPI lifespan event
- Добавить `GET /api/jobs/{job_id}` endpoint
- Sentence-transformers reranker оборачивать в `run_in_executor` (синхронная библиотека)

**Стало проще:**
- Пользователь не ждёт — сразу получает `job_id`
- Можно загружать несколько файлов параллельно
- При добавлении S3 — меняется только upload логика

**Путь к Celery:** если в будущем нужны тысячи задач — `IngestionWorker` заменяется на Celery task. Интерфейс `ingestion_jobs` таблицы остаётся как audit log.
