FROM python:3.12-slim

WORKDIR /app

# Устанавливаем системные зависимости для pgvector и sentence-transformers
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc \
    libpq-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Копируем только pyproject.toml для кэширования зависимостей
COPY pyproject.toml .

# Устанавливаем зависимости
RUN pip install --no-cache-dir -e .

# Копируем исходники
COPY src/ src/
COPY alembic/ alembic/
COPY alembic.ini .
COPY templates/ templates/
COPY static/ static/

# Создаём нужные директории
RUN mkdir -p data/uploads

# Применяем миграции и запускаем
CMD ["sh", "-c", "alembic upgrade head && uvicorn src.main:app --host 0.0.0.0 --port 8000"]
