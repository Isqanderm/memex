# ADR-0011: Конфигурация — Pydantic Settings

**Статус:** accepted
**Дата:** 2026-05-29
**Автор:** Александр Мельник

---

## Контекст

Система требует конфигурации из env переменных: ключи API, параметры БД, выбор LLM провайдера. Нужен единый способ читать, валидировать и документировать конфигурацию.

## Решение

**Pydantic Settings** (`pydantic-settings`) — единственный файл `src/config.py`.

Читает из env переменных и `.env` файла. Все значения типизированы и валидируются при старте. Если обязательная переменная отсутствует — приложение падает с понятной ошибкой сразу, не в момент первого использования.

```python
class Settings(BaseSettings):
    # Database
    database_url: str                    # postgresql+asyncpg://...

    # OpenAI
    openai_api_key: str
    embedding_model: str = "text-embedding-3-small"
    embedding_dimensions: int = 1536

    # LLM Provider
    llm_provider: Literal["claude", "openai"] = "claude"
    llm_model: str = "claude-opus-4-7"
    llm_max_tokens: int = 2048
    llm_temperature: float = 0.1
    anthropic_api_key: str | None = None
    openai_llm_api_key: str | None = None

    # Chunking
    l2_chunk_size: int = 512
    l1_chunk_size: int = 128
    l2_chunk_overlap: int = 64

    # Retrieval
    semantic_top_k: int = 20
    bm25_top_k: int = 20
    rrf_k: int = 60
    reranker_top_n: int = 5

    # File storage
    upload_dir: Path = Path("data/uploads")

    model_config = SettingsConfigDict(env_file=".env", extra="ignore")
```

## Последствия

**Правила:**
- Нет `os.environ.get()` в коде — только через `Settings`
- `Settings` создаётся один раз, передаётся через FastAPI dependency injection
- Чанкер, ретривер, провайдеры — получают нужные значения из `Settings`, не читают env напрямую
- `.env.example` в корне репозитория — шаблон со всеми переменными и комментариями
- `.env` в `.gitignore`
