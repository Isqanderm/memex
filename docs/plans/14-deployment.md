# Task 14: Deployment (Dockerfile + RPi)

**Goal:** Dockerfile для ARM64/RPi, инструкция по кросс-компиляции, обновлённый docker-compose.yml без PostgreSQL.

**Files:**
- Create: `rust/Dockerfile`
- Create: `rust/.env.example`
- Create: `docker-compose.rust.yml`

---

### Task 14.1: Dockerfile (multi-stage, ARM64)

- [ ] **Шаг 1: Создать rust/Dockerfile**

```dockerfile
# ── Stage 1: Builder ─────────────────────────────────────────────────────────
FROM --platform=$BUILDPLATFORM rust:1.82-slim AS builder

# Зависимости для сборки
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Кешировать зависимости отдельно (быстрее повторные сборки)
COPY rust/Cargo.toml rust/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release 2>&1 || true
RUN rm -rf src

# Собрать приложение
COPY rust/src ./src
# Сбросить timestamp чтобы cargo пересобрал
RUN touch src/main.rs
RUN cargo build --release

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim

# Poppler для pdftotext, CA-сертификаты для HTTPS (LLM API)
RUN apt-get update && apt-get install -y \
    poppler-utils \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Бинарь
COPY --from=builder /app/target/release/memex ./memex

# Шаблоны и статика
COPY rust/templates ./templates
COPY rust/static    ./static

# ONNX модели будут скачаны при первом запуске в DATA_DIR
# Или можно предварительно смонтировать volume с моделями

RUN mkdir -p data/uploads data/tantivy

EXPOSE 8000

ENTRYPOINT ["./memex"]
```

- [ ] **Шаг 2: Проверить сборку образа (x86_64)**

```bash
cd /path/to/repo && docker build -f rust/Dockerfile -t memex-rust:latest .
docker run --rm memex-rust:latest --help 2>&1 || echo "no --help flag"
```

---

### Task 14.2: .env.example

- [ ] **Шаг 1: Создать rust/.env.example**

```bash
# База данных
DATABASE_PATH=data/memex.db
TANTIVY_PATH=data/tantivy
UPLOAD_DIR=data/uploads

# Эмбеддинги (ONNX, скачиваются автоматически)
LOCAL_EMBEDDING_MODEL=intfloat/multilingual-e5-small
EMBEDDING_DIMENSIONS=384

# LLM провайдер (claude | openai)
LLM_PROVIDER=claude
LLM_MODEL=claude-sonnet-4-6
LLM_MAX_TOKENS=2048
LLM_TEMPERATURE=0.1

# Ключи API (один из двух обязателен)
ANTHROPIC_API_KEY=sk-ant-...
# OPENAI_LLM_API_KEY=sk-...

# Chunking
L2_CHUNK_SIZE=512
L1_CHUNK_SIZE=128
L2_CHUNK_OVERLAP=64

# Retrieval
SEMANTIC_TOP_K=20
BM25_TOP_K=20
RRF_K=60
RERANKER_TOP_N=5

# HTTP
HOST=0.0.0.0
PORT=8000
```

---

### Task 14.3: docker-compose.rust.yml (без PostgreSQL)

- [ ] **Шаг 1: Создать docker-compose.rust.yml**

```yaml
# docker-compose.rust.yml — минимальный стек для RPi и других ARM устройств.
# Не требует PostgreSQL: SQLite + sqlite-vec встроены в бинарь.
#
# Запуск:
#   docker compose -f docker-compose.rust.yml up
#
# Миграция с PostgreSQL:
#   docker compose -f docker-compose.rust.yml run memex-migrate

services:
  memex:
    image: ghcr.io/your-org/memex-rust:latest
    # Для локальной сборки:
    # build:
    #   context: .
    #   dockerfile: rust/Dockerfile
    ports:
      - "8000:8000"
    env_file: .env
    volumes:
      # Данные: SQLite база, tantivy индекс, загруженные файлы
      - ./data:/app/data
      # ONNX модели (кешируются между перезапусками)
      - fastembed_cache:/root/.cache/huggingface
    restart: unless-stopped
    # Для RPi 4 (4GB): 512MB достаточно в idle, до 1GB под нагрузкой с моделями
    # mem_limit: 1500m

  # Отдельный сервис для миграции с PostgreSQL
  # Запустить однократно: docker compose -f docker-compose.rust.yml run memex-migrate
  memex-migrate:
    image: ghcr.io/your-org/memex-rust:latest
    entrypoint: ["./memex-migrate"]
    env_file: .env
    environment:
      DATABASE_URL: ${POSTGRES_URL:-postgresql://memex:memex@postgres:5432/memex}
      SQLITE_PATH: /app/data/memex.db
      TANTIVY_PATH: /app/data/tantivy
    volumes:
      - ./data:/app/data
      - fastembed_cache:/root/.cache/huggingface
    profiles: ["migrate"]

volumes:
  fastembed_cache:
```

---

### Task 14.4: Кросс-компиляция для RPi (без Docker)

- [ ] **Шаг 1: Установить cross**

```bash
cargo install cross
```

- [ ] **Шаг 2: Собрать для ARM64 (RPi 4/5)**

```bash
cd rust && cross build --release --target aarch64-unknown-linux-gnu
```

Бинарь будет в: `target/aarch64-unknown-linux-gnu/release/memex`

- [ ] **Шаг 3: Скопировать на RPi**

```bash
# Заменить rpi_ip на IP Raspberry Pi
scp target/aarch64-unknown-linux-gnu/release/memex pi@rpi_ip:/home/pi/memex/
scp -r rust/templates pi@rpi_ip:/home/pi/memex/
scp -r rust/static    pi@rpi_ip:/home/pi/memex/
scp rust/.env.example pi@rpi_ip:/home/pi/memex/.env
```

- [ ] **Шаг 4: Установить poppler на RPi**

```bash
# На Raspberry Pi:
sudo apt install poppler-utils
```

- [ ] **Шаг 5: Запустить на RPi**

```bash
# На RPi:
cd /home/pi/memex
# Отредактировать .env с реальными ключами
./memex
```

---

### Task 14.5: Замена Python-кода (финал)

После проверки что Rust-сервис полностью работает:

- [ ] **Шаг 1: Проверить что все Python тесты (smoke) проходят против Rust-сервера**

```bash
# Запустить Rust-сервер
cd rust && cargo run &

# Проверить ключевые эндпоинты
curl http://localhost:8000/health
curl http://localhost:8000/api/documents | jq .
curl -X POST http://localhost:8000/api/query \
  -H 'Content-Type: application/json' \
  -d '{"query": "test"}' | jq .
```

- [ ] **Шаг 2: Обновить README с новыми инструкциями деплоя**

- [ ] **Шаг 3: Удалить Python-код (опционально — после полной верификации)**

```bash
# ВНИМАНИЕ: только после полного тестирования!
git rm -r src/ pyproject.toml alembic/ mcp_server.py docker-compose.yml
```

> **Сохранить:** `mcp_server.py` если он используется как MCP клиент в Claude Code.

- [ ] **Шаг 4: Финальный коммит**

```bash
git add rust/Dockerfile docker-compose.rust.yml rust/.env.example
git commit -m "feat(rust): Dockerfile multi-stage ARM64, docker-compose без PostgreSQL, кросс-компиляция для RPi"
```

---

## Ожидаемые метрики на RPi 4 (4GB RAM)

| Метрика | Python (было) | Rust (будет) |
|---|---|---|
| RAM при старте | ~2.5 GB | ~80-120 MB |
| RAM под нагрузкой | ~3 GB | ~200-400 MB* |
| Время старта | ~15 сек | ~2-3 сек |
| Время запроса (p50) | ~3-8 сек | ~1-3 сек |
| Размер Docker образа | ~4 GB | ~150 MB |
| Бинарь (stripped) | N/A | ~25 MB |

*Пиковое потребление — во время inference ONNX моделей (embeddings + reranker).
