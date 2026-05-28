# Memex Implementation Plan

> **For Hermes:** Use arch:subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Personal RAG система — индексирует документы (PDF, DOCX, MD, TXT), отвечает на вопросы по ним через Hybrid Search + LLM.

**Architecture:** Ingestion pipeline (Adapter → SmallToBigChunker → Embed → PG) + Retrieval pipeline (Semantic + BM25 → RRF → Expand L2 → Reranker → LLM). Async throughout. PostgreSQL как очередь задач.

**Tech Stack:** Python 3.12, FastAPI, SQLAlchemy 2.0 async, asyncpg, Alembic, pgvector, OpenAI SDK, sentence-transformers, Anthropic SDK, Jinja2, HTMX, pytest, testcontainers-python.

**Design doc:** `docs/superpowers/specs/2026-05-29-memex-design.md`
**Architecture:** `docs/architecture/AGENTS.md` — читать перед реализацией каждой задачи.

---

## Phase 1: Foundation

### Task 1: Project setup — pyproject.toml и структура директорий

**Objective:** Создать структуру проекта и установить зависимости.

**Files:**
- Create: `pyproject.toml`
- Create: `src/__init__.py`
- Create: `src/api/__init__.py`
- Create: `src/ui/__init__.py`
- Create: `src/mcp/__init__.py`
- Create: `src/adapters/__init__.py`
- Create: `src/ingestion/__init__.py`
- Create: `src/retrieval/__init__.py`
- Create: `src/llm/__init__.py`
- Create: `src/models/__init__.py`
- Create: `src/db/__init__.py`
- Create: `src/db/repositories/__init__.py`
- Create: `tests/__init__.py`
- Create: `tests/unit/__init__.py`
- Create: `tests/integration/__init__.py`
- Create: `tests/e2e/__init__.py`
- Create: `tests/mocks/__init__.py`
- Create: `data/uploads/.gitkeep`
- Create: `templates/.gitkeep`
- Create: `static/.gitkeep`
- Create: `.env.example`

**Step 1: Создать pyproject.toml**

```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "memex"
version = "0.1.0"
requires-python = ">=3.12"
dependencies = [
    "fastapi>=0.115",
    "uvicorn[standard]>=0.30",
    "sqlalchemy[asyncio]>=2.0",
    "asyncpg>=0.30",
    "alembic>=1.13",
    "pgvector>=0.3",
    "pydantic-settings>=2.0",
    "openai>=1.50",
    "anthropic>=0.40",
    "sentence-transformers>=3.0",
    "langdetect>=1.0",
    "pypdf>=5.0",
    "python-docx>=1.1",
    "jinja2>=3.1",
    "python-multipart>=0.0.12",
    "httpx>=0.27",
    "aiofiles>=24.0",
]

[project.optional-dependencies]
dev = [
    "pytest>=8.0",
    "pytest-asyncio>=0.24",
    "testcontainers[postgres]>=4.8",
    "pytest-cov>=5.0",
]

[tool.pytest.ini_options]
asyncio_mode = "auto"
markers = [
    "unit: unit tests (no DB)",
    "integration: integration tests (requires Docker)",
    "e2e: end-to-end tests (requires API keys)",
]

[tool.hatch.build.targets.wheel]
packages = ["src"]
```

**Step 2: Создать все директории и `__init__.py` файлы**

```bash
mkdir -p src/{api,ui,mcp,adapters,ingestion,retrieval,llm,models,db/repositories}
mkdir -p tests/{unit,integration,e2e,mocks}
mkdir -p data/uploads templates static alembic
touch src/__init__.py src/api/__init__.py src/ui/__init__.py
touch src/mcp/__init__.py src/adapters/__init__.py src/ingestion/__init__.py
touch src/retrieval/__init__.py src/llm/__init__.py src/models/__init__.py
touch src/db/__init__.py src/db/repositories/__init__.py
touch tests/__init__.py tests/unit/__init__.py tests/integration/__init__.py
touch tests/e2e/__init__.py tests/mocks/__init__.py
touch data/uploads/.gitkeep templates/.gitkeep static/.gitkeep
```

**Step 3: Создать `.env.example`**

```bash
# Database
DATABASE_URL=postgresql+asyncpg://memex:memex@localhost:5432/memex

# OpenAI
OPENAI_API_KEY=sk-...
EMBEDDING_MODEL=text-embedding-3-small
EMBEDDING_DIMENSIONS=1536

# LLM Provider
LLM_PROVIDER=claude
LLM_MODEL=claude-opus-4-7
LLM_MAX_TOKENS=2048
LLM_TEMPERATURE=0.1
ANTHROPIC_API_KEY=sk-ant-...
# OPENAI_LLM_API_KEY=sk-...  # если LLM_PROVIDER=openai

# Chunking
L2_CHUNK_SIZE=512
L1_CHUNK_SIZE=128
L2_CHUNK_OVERLAP=64

# Retrieval
SEMANTIC_TOP_K=20
BM25_TOP_K=20
RRF_K=60
RERANKER_TOP_N=5

# Storage
UPLOAD_DIR=data/uploads
```

**Step 4: Установить зависимости**

```bash
pip install -e ".[dev]"
```

**Step 5: Commit**

```bash
git add .
git commit -m "feat: initial project structure and dependencies"
```

---

### Task 2: Pydantic Settings — конфигурация

**Objective:** Единый `config.py` читает все настройки из env.

**Files:**
- Create: `src/config.py`
- Create: `tests/unit/test_config.py`

**Step 1: Написать тест**

```python
# tests/unit/test_config.py
import pytest
from src.config import Settings

def test_settings_defaults():
    s = Settings(
        database_url="postgresql+asyncpg://x:x@localhost/x",
        openai_api_key="sk-test",
        anthropic_api_key="sk-ant-test",
    )
    assert s.embedding_model == "text-embedding-3-small"
    assert s.l2_chunk_size == 512
    assert s.l1_chunk_size == 128
    assert s.rrf_k == 60
    assert s.llm_provider == "claude"

def test_settings_upload_dir_is_path():
    from pathlib import Path
    s = Settings(
        database_url="postgresql+asyncpg://x:x@localhost/x",
        openai_api_key="sk-test",
        anthropic_api_key="sk-ant-test",
    )
    assert isinstance(s.upload_dir, Path)
```

**Step 2: Запустить — убедиться что FAIL**

```bash
pytest tests/unit/test_config.py -v
```

**Step 3: Реализовать**

```python
# src/config.py
from pathlib import Path
from typing import Literal
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    # Database
    database_url: str

    # OpenAI Embeddings
    openai_api_key: str
    embedding_model: str = "text-embedding-3-small"
    embedding_dimensions: int = 1536

    # LLM
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

    # Storage
    upload_dir: Path = Path("data/uploads")

    model_config = SettingsConfigDict(env_file=".env", extra="ignore")


_settings: Settings | None = None


def get_settings() -> Settings:
    global _settings
    if _settings is None:
        _settings = Settings()
    return _settings
```

**Step 4: Запустить — убедиться что PASS**

```bash
pytest tests/unit/test_config.py -v
```

**Step 5: Commit**

```bash
git add src/config.py tests/unit/test_config.py
git commit -m "feat: pydantic settings configuration"
```

---

### Task 3: Shared models — Document, Chunk, IngestionJob (dataclasses)

**Objective:** Domain models используемые во всех слоях (не SQLAlchemy — чистые dataclasses).

**Files:**
- Create: `src/models/document.py`
- Create: `src/models/chunk.py`
- Create: `src/models/ingestion.py`
- Create: `src/models/parsed.py`
- Modify: `src/models/__init__.py`

**Step 1: `src/models/parsed.py` — выход адаптеров**

```python
from dataclasses import dataclass, field


@dataclass
class Section:
    content: str
    heading: str | None = None
    level: int = 0          # 0=flat, 1=h1, 2=h2
    page_number: int | None = None
    metadata: dict = field(default_factory=dict)


@dataclass
class ParsedDocument:
    source: str
    mime_type: str
    sections: list[Section]
    metadata: dict = field(default_factory=dict)
```

**Step 2: `src/models/chunk.py`**

```python
from dataclasses import dataclass
from uuid import UUID


@dataclass
class ChunkData:
    """Domain model чанка — используется в pipeline до записи в БД."""
    content: str
    chunk_role: str          # 'parent' | 'leaf'
    chunk_index: int
    language: str = "simple"
    section_heading: str | None = None
    section_level: int | None = None
    page_number: int | None = None
    embedding: list[float] | None = None
    parent_temp_index: int | None = None  # индекс L2-родителя в текущем batch
```

**Step 3: Написать тест**

```python
# tests/unit/test_models.py
from src.models.parsed import ParsedDocument, Section
from src.models.chunk import ChunkData

def test_parsed_document_sections():
    doc = ParsedDocument(
        source="test.md",
        mime_type="text/markdown",
        sections=[Section(content="Hello", heading="Intro", level=1)],
    )
    assert len(doc.sections) == 1
    assert doc.sections[0].heading == "Intro"

def test_chunk_data_defaults():
    chunk = ChunkData(content="text", chunk_role="leaf", chunk_index=0)
    assert chunk.language == "simple"
    assert chunk.embedding is None
```

**Step 4: Запустить тест**

```bash
pytest tests/unit/test_models.py -v
```

**Step 5: Commit**

```bash
git add src/models/ tests/unit/test_models.py
git commit -m "feat: domain models (ParsedDocument, ChunkData)"
```

---

### Task 4: SQLAlchemy ORM модели и сессия

**Objective:** ORM модели для PostgreSQL + async session factory.

**Files:**
- Create: `src/db/models.py`
- Create: `src/db/session.py`

**Step 1: `src/db/models.py`**

```python
import uuid
from datetime import datetime
from sqlalchemy import String, Text, Integer, DateTime, ForeignKey, func
from sqlalchemy.dialects.postgresql import UUID, JSONB
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column, relationship
from pgvector.sqlalchemy import Vector


class Base(DeclarativeBase):
    pass


class Document(Base):
    __tablename__ = "documents"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    source: Mapped[str] = mapped_column(Text, nullable=False)
    mime_type: Mapped[str] = mapped_column(String(100), nullable=False)
    title: Mapped[str | None] = mapped_column(Text)
    checksum: Mapped[str] = mapped_column(String(64), unique=True, nullable=False)
    metadata_: Mapped[dict] = mapped_column("metadata", JSONB, default=dict)
    indexed_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())

    chunks: Mapped[list["Chunk"]] = relationship(back_populates="document", cascade="all, delete-orphan")


class Chunk(Base):
    __tablename__ = "chunks"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    doc_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), ForeignKey("documents.id", ondelete="CASCADE"))
    parent_chunk_id: Mapped[uuid.UUID | None] = mapped_column(UUID(as_uuid=True), ForeignKey("chunks.id"))
    chunk_role: Mapped[str] = mapped_column(String(10), nullable=False)  # 'parent' | 'leaf'
    chunk_index: Mapped[int] = mapped_column(Integer, nullable=False)
    section_heading: Mapped[str | None] = mapped_column(Text)
    section_level: Mapped[int | None] = mapped_column(Integer)
    page_number: Mapped[int | None] = mapped_column(Integer)
    prev_chunk_id: Mapped[uuid.UUID | None] = mapped_column(UUID(as_uuid=True), ForeignKey("chunks.id"))
    next_chunk_id: Mapped[uuid.UUID | None] = mapped_column(UUID(as_uuid=True), ForeignKey("chunks.id"))
    language: Mapped[str] = mapped_column(String(20), default="simple")
    content: Mapped[str] = mapped_column(Text, nullable=False)
    content_vector: Mapped[list[float] | None] = mapped_column(Vector(1536))

    document: Mapped["Document"] = relationship(back_populates="chunks")


class IngestionJob(Base):
    __tablename__ = "ingestion_jobs"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    status: Mapped[str] = mapped_column(String(20), default="pending")  # pending|processing|done|error
    source: Mapped[str] = mapped_column(Text, nullable=False)
    checksum: Mapped[str] = mapped_column(String(64), nullable=False)
    doc_id: Mapped[uuid.UUID | None] = mapped_column(UUID(as_uuid=True), ForeignKey("documents.id"))
    error: Mapped[str | None] = mapped_column(Text)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())
    updated_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now(), onupdate=func.now())
```

**Step 2: `src/db/session.py`**

```python
from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine, async_sessionmaker
from src.config import get_settings


def create_engine(database_url: str | None = None):
    url = database_url or get_settings().database_url
    return create_async_engine(url, echo=False, pool_pre_ping=True)


def create_session_factory(engine) -> async_sessionmaker[AsyncSession]:
    return async_sessionmaker(engine, expire_on_commit=False)


# Глобальные синглтоны (инициализируются в lifespan)
_engine = None
_session_factory = None


def get_session_factory() -> async_sessionmaker[AsyncSession]:
    if _session_factory is None:
        raise RuntimeError("Session factory not initialized. Call init_db() first.")
    return _session_factory


async def init_db(database_url: str | None = None):
    global _engine, _session_factory
    _engine = create_engine(database_url)
    _session_factory = create_session_factory(_engine)


async def close_db():
    global _engine
    if _engine:
        await _engine.dispose()
```

**Step 5: Commit**

```bash
git add src/db/
git commit -m "feat: SQLAlchemy ORM models and async session factory"
```

---

### Task 5: Alembic — инициализация и первая миграция

**Objective:** Настроить Alembic, создать таблицы + индексы.

**Files:**
- Create: `alembic.ini`
- Create: `alembic/env.py`
- Create: `alembic/versions/0001_initial.py`

**Step 1: Инициализировать Alembic**

```bash
alembic init alembic
```

**Step 2: Обновить `alembic/env.py`** — подключить наши модели:

```python
# alembic/env.py (ключевые изменения)
from src.db.models import Base
from src.config import get_settings

config.set_main_option("sqlalchemy.url", get_settings().database_url.replace("+asyncpg", ""))

target_metadata = Base.metadata

# В run_migrations_online() использовать синхронный движок:
from sqlalchemy import create_engine as sync_create_engine
connectable = sync_create_engine(config.get_main_option("sqlalchemy.url"))
```

**Step 3: Создать первую миграцию вручную** `alembic/versions/0001_initial.py`:

```python
"""initial schema

Revision ID: 0001
Revises:
Create Date: 2026-05-29
"""
from alembic import op
import sqlalchemy as sa
from pgvector.sqlalchemy import Vector

revision = '0001'
down_revision = None


def upgrade():
    op.execute('CREATE EXTENSION IF NOT EXISTS vector')
    op.execute('CREATE EXTENSION IF NOT EXISTS pg_trgm')

    op.create_table('documents',
        sa.Column('id', sa.UUID(), primary_key=True),
        sa.Column('source', sa.Text(), nullable=False),
        sa.Column('mime_type', sa.String(100), nullable=False),
        sa.Column('title', sa.Text()),
        sa.Column('checksum', sa.String(64), unique=True, nullable=False),
        sa.Column('metadata', sa.JSON(), default={}),
        sa.Column('indexed_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
    )

    op.create_table('chunks',
        sa.Column('id', sa.UUID(), primary_key=True),
        sa.Column('doc_id', sa.UUID(), sa.ForeignKey('documents.id', ondelete='CASCADE'), nullable=False),
        sa.Column('parent_chunk_id', sa.UUID(), sa.ForeignKey('chunks.id')),
        sa.Column('chunk_role', sa.String(10), nullable=False),
        sa.Column('chunk_index', sa.Integer(), nullable=False),
        sa.Column('section_heading', sa.Text()),
        sa.Column('section_level', sa.Integer()),
        sa.Column('page_number', sa.Integer()),
        sa.Column('prev_chunk_id', sa.UUID(), sa.ForeignKey('chunks.id')),
        sa.Column('next_chunk_id', sa.UUID(), sa.ForeignKey('chunks.id')),
        sa.Column('language', sa.String(20), default='simple'),
        sa.Column('content', sa.Text(), nullable=False),
        sa.Column('content_vector', Vector(1536)),
    )

    op.create_table('ingestion_jobs',
        sa.Column('id', sa.UUID(), primary_key=True),
        sa.Column('status', sa.String(20), default='pending'),
        sa.Column('source', sa.Text(), nullable=False),
        sa.Column('checksum', sa.String(64), nullable=False),
        sa.Column('doc_id', sa.UUID(), sa.ForeignKey('documents.id')),
        sa.Column('error', sa.Text()),
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
        sa.Column('updated_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
    )

    # Индексы
    op.execute("CREATE INDEX ON chunks USING hnsw (content_vector vector_cosine_ops) WHERE content_vector IS NOT NULL")
    op.execute("ALTER TABLE chunks ADD COLUMN tsv tsvector")
    op.execute("CREATE INDEX ON chunks USING GIN (tsv)")
    op.execute("CREATE INDEX ON ingestion_jobs(status, created_at) WHERE status = 'pending'")
    op.execute("CREATE UNIQUE INDEX ON ingestion_jobs(checksum) WHERE status IN ('pending', 'processing')")


def downgrade():
    op.drop_table('ingestion_jobs')
    op.drop_table('chunks')
    op.drop_table('documents')
```

**Step 4: Проверить что миграция применяется** (нужен запущенный PostgreSQL):

```bash
# Запустить PostgreSQL через Docker:
docker run -d --name memex-pg \
  -e POSTGRES_USER=memex -e POSTGRES_PASSWORD=memex -e POSTGRES_DB=memex \
  -p 5432:5432 pgvector/pgvector:pg15

# Применить миграцию:
alembic upgrade head
```

**Step 5: Commit**

```bash
git add alembic/ alembic.ini
git commit -m "feat: alembic setup and initial schema migration"
```

---

### Task 6: Test infrastructure — testcontainers conftest

**Objective:** Общие фикстуры для интеграционных тестов.

**Files:**
- Create: `tests/conftest.py`
- Create: `tests/integration/conftest.py`

**Step 1: `tests/conftest.py`**

```python
import pytest


def pytest_configure(config):
    config.addinivalue_line("markers", "unit: unit tests, no external dependencies")
    config.addinivalue_line("markers", "integration: requires Docker")
    config.addinivalue_line("markers", "e2e: requires API keys")
```

**Step 2: `tests/integration/conftest.py`**

```python
import pytest
import pytest_asyncio
from testcontainers.postgres import PostgresContainer
from sqlalchemy.ext.asyncio import create_async_engine, async_sessionmaker, AsyncSession
from alembic.config import Config
from alembic import command
from src.db.models import Base

PG_IMAGE = "pgvector/pgvector:pg15"


@pytest.fixture(scope="session")
def pg_container():
    with PostgresContainer(PG_IMAGE, username="test", password="test", dbname="test") as pg:
        yield pg


@pytest.fixture(scope="session")
def db_url(pg_container):
    host = pg_container.get_container_host_ip()
    port = pg_container.get_exposed_port(5432)
    return f"postgresql+asyncpg://test:test@{host}:{port}/test"


@pytest.fixture(scope="session")
def sync_db_url(pg_container):
    host = pg_container.get_container_host_ip()
    port = pg_container.get_exposed_port(5432)
    return f"postgresql://test:test@{host}:{port}/test"


@pytest.fixture(scope="session", autouse=True)
def apply_migrations(sync_db_url):
    cfg = Config("alembic.ini")
    cfg.set_main_option("sqlalchemy.url", sync_db_url)
    command.upgrade(cfg, "head")


@pytest_asyncio.fixture(scope="session")
async def engine(db_url):
    eng = create_async_engine(db_url)
    yield eng
    await eng.dispose()


@pytest_asyncio.fixture(scope="session")
async def session_factory(engine):
    return async_sessionmaker(engine, expire_on_commit=False)


@pytest_asyncio.fixture
async def db_session(session_factory) -> AsyncSession:
    async with session_factory() as session:
        async with session.begin():
            yield session
            await session.rollback()
```

**Step 3: Smoke test**

```python
# tests/integration/test_db_smoke.py
import pytest
from sqlalchemy import text

@pytest.mark.integration
async def test_pgvector_extension(db_session):
    result = await db_session.execute(text("SELECT extname FROM pg_extension WHERE extname = 'vector'"))
    assert result.scalar() == "vector"

@pytest.mark.integration
async def test_tables_exist(db_session):
    result = await db_session.execute(
        text("SELECT tablename FROM pg_tables WHERE schemaname = 'public'")
    )
    tables = {row[0] for row in result}
    assert {"documents", "chunks", "ingestion_jobs"} <= tables
```

**Step 4: Запустить**

```bash
pytest tests/integration/test_db_smoke.py -v -m integration
```

Expected: 2 passed

**Step 5: Commit**

```bash
git add tests/
git commit -m "test: testcontainers integration test infrastructure"
```

---

## Phase 2: Adapter Layer

### Task 7: DocumentAdapter Protocol + AdapterRegistry

**Objective:** Интерфейс адаптеров и реестр.

**Files:**
- Create: `src/adapters/protocol.py`
- Create: `src/adapters/registry.py`
- Create: `tests/unit/test_adapter_registry.py`

**Step 1: Написать тест**

```python
# tests/unit/test_adapter_registry.py
import pytest
from src.adapters.registry import AdapterRegistry
from src.adapters.protocol import DocumentAdapter, Source
from src.models.parsed import ParsedDocument, Section


class FakePdfAdapter:
    def can_handle(self, source: Source) -> bool:
        return source.mime_type == "application/pdf"

    def parse(self, source: Source) -> ParsedDocument:
        return ParsedDocument(source=source.path, mime_type="application/pdf", sections=[
            Section(content="PDF content")
        ])


def test_registry_finds_correct_adapter():
    registry = AdapterRegistry()
    registry.register(FakePdfAdapter())
    source = Source(path="doc.pdf", mime_type="application/pdf")
    adapter = registry.get(source)
    assert adapter is not None

def test_registry_returns_none_for_unknown():
    registry = AdapterRegistry()
    source = Source(path="doc.xyz", mime_type="application/octet-stream")
    assert registry.get(source) is None

def test_registry_first_match_wins():
    class AlwaysAdapter:
        def can_handle(self, s): return True
        def parse(self, s): return ParsedDocument(source=s.path, mime_type="", sections=[])

    registry = AdapterRegistry()
    registry.register(FakePdfAdapter())
    registry.register(AlwaysAdapter())
    source = Source(path="doc.pdf", mime_type="application/pdf")
    adapter = registry.get(source)
    assert isinstance(adapter, FakePdfAdapter)
```

**Step 2: Запустить — убедиться что FAIL**

```bash
pytest tests/unit/test_adapter_registry.py -v
```

**Step 3: Реализовать**

```python
# src/adapters/protocol.py
from dataclasses import dataclass
from typing import Protocol, runtime_checkable
from src.models.parsed import ParsedDocument


@dataclass
class Source:
    path: str
    mime_type: str
    filename: str = ""


@runtime_checkable
class DocumentAdapter(Protocol):
    def can_handle(self, source: Source) -> bool: ...
    def parse(self, source: Source) -> ParsedDocument: ...
```

```python
# src/adapters/registry.py
from src.adapters.protocol import DocumentAdapter, Source
from src.models.parsed import ParsedDocument


class AdapterRegistry:
    def __init__(self):
        self._adapters: list[DocumentAdapter] = []

    def register(self, adapter: DocumentAdapter) -> None:
        self._adapters.append(adapter)

    def get(self, source: Source) -> DocumentAdapter | None:
        for adapter in self._adapters:
            if adapter.can_handle(source):
                return adapter
        return None

    def parse(self, source: Source) -> ParsedDocument:
        adapter = self.get(source)
        if adapter is None:
            raise ValueError(f"No adapter found for {source.mime_type} ({source.path})")
        return adapter.parse(source)
```

**Step 4: Запустить — убедиться что PASS**

```bash
pytest tests/unit/test_adapter_registry.py -v
```

**Step 5: Commit**

```bash
git add src/adapters/ tests/unit/test_adapter_registry.py
git commit -m "feat: DocumentAdapter protocol and AdapterRegistry"
```

---

### Task 8: TextAdapter и MarkdownAdapter

**Objective:** Адаптеры для .txt и .md файлов.

**Files:**
- Create: `src/adapters/text.py`
- Create: `src/adapters/markdown.py`
- Create: `tests/unit/test_adapters_text_md.py`
- Create: `tests/fixtures/sample.txt`
- Create: `tests/fixtures/sample.md`

**Step 1: Создать тестовые файлы**

```
# tests/fixtures/sample.txt
Hello world.
This is a plain text document.
It has multiple lines.
```

```markdown
# tests/fixtures/sample.md
# Introduction

This is the first section.

## Details

More content here with **bold** text.
```

**Step 2: Написать тесты**

```python
# tests/unit/test_adapters_text_md.py
import pytest
from pathlib import Path
from src.adapters.protocol import Source
from src.adapters.text import TextAdapter
from src.adapters.markdown import MarkdownAdapter

FIXTURES = Path("tests/fixtures")


def test_text_adapter_can_handle_txt():
    adapter = TextAdapter()
    assert adapter.can_handle(Source(path="doc.txt", mime_type="text/plain"))

def test_text_adapter_parses_content():
    adapter = TextAdapter()
    source = Source(path=str(FIXTURES / "sample.txt"), mime_type="text/plain")
    doc = adapter.parse(source)
    assert len(doc.sections) >= 1
    assert "Hello world" in doc.sections[0].content

def test_markdown_adapter_can_handle_md():
    adapter = MarkdownAdapter()
    assert adapter.can_handle(Source(path="doc.md", mime_type="text/markdown"))
    assert adapter.can_handle(Source(path="doc.md", mime_type="text/plain", filename="README.md"))

def test_markdown_adapter_extracts_headings():
    adapter = MarkdownAdapter()
    source = Source(path=str(FIXTURES / "sample.md"), mime_type="text/markdown")
    doc = adapter.parse(source)
    headings = [s.heading for s in doc.sections if s.heading]
    assert "Introduction" in headings
    assert "Details" in headings
```

**Step 3: Реализовать TextAdapter**

```python
# src/adapters/text.py
from src.adapters.protocol import DocumentAdapter, Source
from src.models.parsed import ParsedDocument, Section


class TextAdapter:
    def can_handle(self, source: Source) -> bool:
        return (source.mime_type == "text/plain"
                or source.path.endswith(".txt"))

    def parse(self, source: Source) -> ParsedDocument:
        with open(source.path, encoding="utf-8", errors="replace") as f:
            content = f.read()
        return ParsedDocument(
            source=source.path,
            mime_type="text/plain",
            sections=[Section(content=content)],
            metadata={"filename": source.filename or source.path},
        )
```

**Step 4: Реализовать MarkdownAdapter**

```python
# src/adapters/markdown.py
import re
from src.adapters.protocol import DocumentAdapter, Source
from src.models.parsed import ParsedDocument, Section


class MarkdownAdapter:
    def can_handle(self, source: Source) -> bool:
        path = source.filename or source.path
        return (source.mime_type in ("text/markdown", "text/x-markdown")
                or path.endswith((".md", ".markdown")))

    def parse(self, source: Source) -> ParsedDocument:
        with open(source.path, encoding="utf-8", errors="replace") as f:
            content = f.read()

        sections = self._split_by_headings(content)
        return ParsedDocument(
            source=source.path,
            mime_type="text/markdown",
            sections=sections,
            metadata={"filename": source.filename or source.path},
        )

    def _split_by_headings(self, content: str) -> list[Section]:
        heading_re = re.compile(r'^(#{1,6})\s+(.+)$', re.MULTILINE)
        sections = []
        last_end = 0
        current_heading = None
        current_level = 0

        for match in heading_re.finditer(content):
            if last_end < match.start():
                text = content[last_end:match.start()].strip()
                if text:
                    sections.append(Section(
                        content=text,
                        heading=current_heading,
                        level=current_level,
                    ))
            current_heading = match.group(2).strip()
            current_level = len(match.group(1))
            last_end = match.end()

        remaining = content[last_end:].strip()
        if remaining:
            sections.append(Section(
                content=remaining,
                heading=current_heading,
                level=current_level,
            ))

        return sections if sections else [Section(content=content)]
```

**Step 5: Запустить**

```bash
pytest tests/unit/test_adapters_text_md.py -v
```

**Step 6: Commit**

```bash
git add src/adapters/text.py src/adapters/markdown.py tests/
git commit -m "feat: TextAdapter and MarkdownAdapter"
```

---

### Task 9: PdfAdapter

**Objective:** Парсинг PDF с сохранением номеров страниц.

**Files:**
- Create: `src/adapters/pdf.py`
- Create: `tests/unit/test_adapter_pdf.py`
- Create: `tests/fixtures/sample.pdf` (сгенерировать программно в тесте)

**Step 1: Написать тест**

```python
# tests/unit/test_adapter_pdf.py
import pytest
import io
from src.adapters.protocol import Source
from src.adapters.pdf import PdfAdapter


def test_pdf_adapter_can_handle():
    adapter = PdfAdapter()
    assert adapter.can_handle(Source(path="doc.pdf", mime_type="application/pdf"))
    assert not adapter.can_handle(Source(path="doc.txt", mime_type="text/plain"))


def test_pdf_adapter_extracts_text(tmp_path):
    # Создаём простой PDF программно через pypdf
    from pypdf import PdfWriter
    writer = PdfWriter()
    page = writer.add_blank_page(width=200, height=200)
    pdf_path = tmp_path / "test.pdf"
    with open(pdf_path, "wb") as f:
        writer.write(f)

    adapter = PdfAdapter()
    source = Source(path=str(pdf_path), mime_type="application/pdf")
    doc = adapter.parse(source)
    assert doc.mime_type == "application/pdf"
    assert isinstance(doc.sections, list)
```

**Step 2: Реализовать**

```python
# src/adapters/pdf.py
from src.adapters.protocol import DocumentAdapter, Source
from src.models.parsed import ParsedDocument, Section


class PdfAdapter:
    def can_handle(self, source: Source) -> bool:
        return (source.mime_type == "application/pdf"
                or source.path.endswith(".pdf"))

    def parse(self, source: Source) -> ParsedDocument:
        from pypdf import PdfReader
        reader = PdfReader(source.path)
        sections = []

        for page_num, page in enumerate(reader.pages, start=1):
            text = page.extract_text() or ""
            text = text.strip()
            if text:
                sections.append(Section(
                    content=text,
                    page_number=page_num,
                ))

        metadata = {}
        if reader.metadata:
            metadata["title"] = reader.metadata.get("/Title", "")
            metadata["author"] = reader.metadata.get("/Author", "")

        return ParsedDocument(
            source=source.path,
            mime_type="application/pdf",
            sections=sections or [Section(content="")],
            metadata=metadata,
        )
```

**Step 3: Запустить + commit**

```bash
pytest tests/unit/test_adapter_pdf.py -v
git add src/adapters/pdf.py tests/unit/test_adapter_pdf.py
git commit -m "feat: PdfAdapter with page number extraction"
```

---

### Task 10: DocxAdapter

**Objective:** Парсинг DOCX с сохранением структуры заголовков.

**Files:**
- Create: `src/adapters/docx.py`
- Create: `tests/unit/test_adapter_docx.py`

**Step 1: Тест**

```python
# tests/unit/test_adapter_docx.py
import pytest
from src.adapters.protocol import Source
from src.adapters.docx import DocxAdapter


def test_docx_can_handle():
    adapter = DocxAdapter()
    assert adapter.can_handle(Source(path="doc.docx", mime_type="application/vnd.openxmlformats-officedocument.wordprocessingml.document"))
    assert adapter.can_handle(Source(path="doc.docx", mime_type="application/octet-stream", filename="doc.docx"))


def test_docx_parses_content(tmp_path):
    from docx import Document as DocxDocument
    doc = DocxDocument()
    doc.add_heading("Introduction", level=1)
    doc.add_paragraph("This is intro text.")
    doc.add_heading("Details", level=2)
    doc.add_paragraph("More details here.")
    path = tmp_path / "test.docx"
    doc.save(str(path))

    adapter = DocxAdapter()
    source = Source(path=str(path), mime_type="application/vnd.openxmlformats-officedocument.wordprocessingml.document")
    result = adapter.parse(source)
    assert len(result.sections) >= 1
    contents = " ".join(s.content for s in result.sections)
    assert "intro text" in contents
```

**Step 2: Реализовать**

```python
# src/adapters/docx.py
from src.adapters.protocol import DocumentAdapter, Source
from src.models.parsed import ParsedDocument, Section

DOCX_MIME = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
HEADING_STYLES = {"Heading 1": 1, "Heading 2": 2, "Heading 3": 3}


class DocxAdapter:
    def can_handle(self, source: Source) -> bool:
        path = source.filename or source.path
        return source.mime_type == DOCX_MIME or path.endswith(".docx")

    def parse(self, source: Source) -> ParsedDocument:
        from docx import Document as DocxDocument
        doc = DocxDocument(source.path)
        sections = []
        current_heading = None
        current_level = 0
        current_paragraphs: list[str] = []

        for para in doc.paragraphs:
            style_name = para.style.name if para.style else ""
            level = HEADING_STYLES.get(style_name)

            if level is not None:
                if current_paragraphs:
                    sections.append(Section(
                        content="\n".join(current_paragraphs),
                        heading=current_heading,
                        level=current_level,
                    ))
                    current_paragraphs = []
                current_heading = para.text.strip()
                current_level = level
            else:
                text = para.text.strip()
                if text:
                    current_paragraphs.append(text)

        if current_paragraphs:
            sections.append(Section(
                content="\n".join(current_paragraphs),
                heading=current_heading,
                level=current_level,
            ))

        return ParsedDocument(
            source=source.path,
            mime_type=DOCX_MIME,
            sections=sections or [Section(content="")],
            metadata={"filename": source.filename or source.path},
        )
```

**Step 3: Запустить + commit**

```bash
pytest tests/unit/test_adapter_docx.py -v
git add src/adapters/docx.py tests/unit/test_adapter_docx.py
git commit -m "feat: DocxAdapter with heading structure"
```

---

## Phase 3: Ingestion Pipeline

### Task 11: LanguageDetector

**Objective:** Определяет язык текста чанка для правильного tsvector конфига.

**Files:**
- Create: `src/ingestion/language.py`
- Create: `tests/unit/test_language_detector.py`

**Step 1: Тест**

```python
# tests/unit/test_language_detector.py
from src.ingestion.language import LanguageDetector

def test_detects_english():
    detector = LanguageDetector()
    assert detector.detect("The quick brown fox jumps over the lazy dog") == "en"

def test_detects_russian():
    detector = LanguageDetector()
    assert detector.detect("Быстрая коричневая лиса прыгает через ленивую собаку") == "ru"

def test_fallback_on_short_text():
    detector = LanguageDetector()
    result = detector.detect("Hi")
    assert result in ("en", "simple")  # короткий текст — может не определить

def test_to_pg_config():
    detector = LanguageDetector()
    assert detector.to_pg_config("ru") == "russian"
    assert detector.to_pg_config("en") == "english"
    assert detector.to_pg_config("xx") == "simple"
```

**Step 2: Реализовать**

```python
# src/ingestion/language.py
LANG_TO_PG = {
    "ru": "russian",
    "en": "english",
    "de": "german",
    "fr": "french",
    "es": "spanish",
}


class LanguageDetector:
    def detect(self, text: str) -> str:
        if len(text.strip()) < 20:
            return "simple"
        try:
            from langdetect import detect
            return detect(text)
        except Exception:
            return "simple"

    def to_pg_config(self, lang: str) -> str:
        return LANG_TO_PG.get(lang, "simple")
```

**Step 3: Запустить + commit**

```bash
pytest tests/unit/test_language_detector.py -v
git add src/ingestion/language.py tests/unit/test_language_detector.py
git commit -m "feat: LanguageDetector with PostgreSQL config mapping"
```

---

### Task 12: SmallToBigChunker

**Objective:** Нарезает ParsedDocument на L2 (~512 tok) и L1 (~128 tok) чанки.

**Files:**
- Create: `src/ingestion/chunker.py`
- Create: `tests/unit/test_chunker.py`

**Step 1: Написать тесты**

```python
# tests/unit/test_chunker.py
import pytest
from src.ingestion.chunker import SmallToBigChunker
from src.models.parsed import ParsedDocument, Section
from src.models.chunk import ChunkData


def make_doc(content: str) -> ParsedDocument:
    return ParsedDocument(
        source="test.txt",
        mime_type="text/plain",
        sections=[Section(content=content)],
    )


def test_produces_both_levels():
    chunker = SmallToBigChunker(l2_size=100, l1_size=30, l2_overlap=10)
    doc = make_doc("word " * 200)  # длинный текст
    chunks = chunker.chunk(doc)
    roles = {c.chunk_role for c in chunks}
    assert "parent" in roles
    assert "leaf" in roles


def test_leaves_reference_parents():
    chunker = SmallToBigChunker(l2_size=100, l1_size=30, l2_overlap=10)
    doc = make_doc("word " * 200)
    chunks = chunker.chunk(doc)
    leaves = [c for c in chunks if c.chunk_role == "leaf"]
    parents = [c for c in chunks if c.chunk_role == "parent"]
    assert len(leaves) > 0
    assert all(c.parent_temp_index is not None for c in leaves)
    # parent_temp_index указывает на валидный parent
    assert all(0 <= c.parent_temp_index < len(parents) for c in leaves)


def test_short_doc_has_at_least_one_chunk():
    chunker = SmallToBigChunker(l2_size=512, l1_size=128, l2_overlap=64)
    doc = make_doc("Short text.")
    chunks = chunker.chunk(doc)
    assert len(chunks) >= 2  # минимум 1 parent + 1 leaf


def test_chunk_index_is_sequential():
    chunker = SmallToBigChunker(l2_size=100, l1_size=30, l2_overlap=10)
    doc = make_doc("word " * 200)
    chunks = chunker.chunk(doc)
    parents = sorted([c for c in chunks if c.chunk_role == "parent"], key=lambda c: c.chunk_index)
    assert [c.chunk_index for c in parents] == list(range(len(parents)))
```

**Step 2: Реализовать**

```python
# src/ingestion/chunker.py
from src.models.parsed import ParsedDocument, Section
from src.models.chunk import ChunkData


def _split_text(text: str, size: int, overlap: int) -> list[str]:
    """Разбивает текст на части по ~size символов с overlap."""
    if not text.strip():
        return []
    words = text.split()
    if not words:
        return []

    chunks = []
    start = 0
    while start < len(words):
        end = min(start + size, len(words))
        chunks.append(" ".join(words[start:end]))
        if end == len(words):
            break
        start += size - overlap

    return chunks if chunks else [text]


class SmallToBigChunker:
    def __init__(
        self,
        l2_size: int = 512,
        l1_size: int = 128,
        l2_overlap: int = 64,
    ):
        self.l2_size = l2_size
        self.l1_size = l1_size
        self.l2_overlap = l2_overlap

    def chunk(self, doc: ParsedDocument) -> list[ChunkData]:
        all_chunks: list[ChunkData] = []
        parent_index = 0

        for section in doc.sections:
            l2_texts = _split_text(section.content, self.l2_size, self.l2_overlap)
            if not l2_texts:
                continue

            for l2_text in l2_texts:
                parent = ChunkData(
                    content=l2_text,
                    chunk_role="parent",
                    chunk_index=parent_index,
                    section_heading=section.heading,
                    section_level=section.level,
                    page_number=section.page_number,
                )
                all_chunks.append(parent)
                current_parent_index = parent_index
                parent_index += 1

                l1_texts = _split_text(l2_text, self.l1_size, 0)
                for leaf_idx, l1_text in enumerate(l1_texts):
                    leaf = ChunkData(
                        content=l1_text,
                        chunk_role="leaf",
                        chunk_index=leaf_idx,
                        section_heading=section.heading,
                        section_level=section.level,
                        page_number=section.page_number,
                        parent_temp_index=current_parent_index,
                    )
                    all_chunks.append(leaf)

        return all_chunks
```

**Step 3: Запустить + commit**

```bash
pytest tests/unit/test_chunker.py -v
git add src/ingestion/chunker.py tests/unit/test_chunker.py
git commit -m "feat: SmallToBigChunker (L2 parent + L1 leaf chunks)"
```

---

### Task 13: EmbeddingStage

**Objective:** Батчевое получение векторов для L1 чанков через OpenAI API.

**Files:**
- Create: `src/ingestion/embedding.py`
- Create: `tests/mocks/mock_embedding.py`
- Create: `tests/unit/test_embedding_stage.py`

**Step 1: Mock и тест**

```python
# tests/mocks/mock_embedding.py
import random


class MockEmbeddingClient:
    def __init__(self, dimensions: int = 1536):
        self.dimensions = dimensions
        self.calls: list[list[str]] = []

    async def embed_batch(self, texts: list[str]) -> list[list[float]]:
        self.calls.append(texts)
        return [[random.uniform(-1, 1) for _ in range(self.dimensions)] for _ in texts]
```

```python
# tests/unit/test_embedding_stage.py
import pytest
from src.ingestion.embedding import EmbeddingStage
from src.models.chunk import ChunkData
from tests.mocks.mock_embedding import MockEmbeddingClient


@pytest.mark.asyncio
async def test_embeds_only_leaf_chunks():
    client = MockEmbeddingClient(dimensions=4)
    stage = EmbeddingStage(client=client)

    chunks = [
        ChunkData(content="parent text", chunk_role="parent", chunk_index=0),
        ChunkData(content="leaf text 1", chunk_role="leaf", chunk_index=0, parent_temp_index=0),
        ChunkData(content="leaf text 2", chunk_role="leaf", chunk_index=1, parent_temp_index=0),
    ]

    result = await stage.process(chunks)

    parents = [c for c in result if c.chunk_role == "parent"]
    leaves = [c for c in result if c.chunk_role == "leaf"]

    assert all(p.embedding is None for p in parents)
    assert all(l.embedding is not None for l in leaves)
    assert all(len(l.embedding) == 4 for l in leaves)


@pytest.mark.asyncio
async def test_batches_requests():
    client = MockEmbeddingClient()
    stage = EmbeddingStage(client=client, batch_size=2)
    chunks = [
        ChunkData(content=f"leaf {i}", chunk_role="leaf", chunk_index=i, parent_temp_index=0)
        for i in range(5)
    ]
    await stage.process(chunks)
    # 5 чанков / batch_size=2 = 3 батча
    assert len(client.calls) == 3
```

**Step 2: Реализовать**

```python
# src/ingestion/embedding.py
from src.models.chunk import ChunkData


class OpenAIEmbeddingClient:
    def __init__(self, api_key: str, model: str = "text-embedding-3-small"):
        import openai
        self._client = openai.AsyncOpenAI(api_key=api_key)
        self.model = model

    async def embed_batch(self, texts: list[str]) -> list[list[float]]:
        response = await self._client.embeddings.create(input=texts, model=self.model)
        return [item.embedding for item in response.data]


class EmbeddingStage:
    def __init__(self, client, batch_size: int = 512):
        self.client = client
        self.batch_size = batch_size

    async def process(self, chunks: list[ChunkData]) -> list[ChunkData]:
        leaves = [c for c in chunks if c.chunk_role == "leaf"]

        for i in range(0, len(leaves), self.batch_size):
            batch = leaves[i:i + self.batch_size]
            texts = [c.content for c in batch]
            embeddings = await self.client.embed_batch(texts)
            for chunk, embedding in zip(batch, embeddings):
                chunk.embedding = embedding

        return chunks
```

**Step 3: Запустить + commit**

```bash
pytest tests/unit/test_embedding_stage.py -v
git add src/ingestion/embedding.py tests/mocks/ tests/unit/test_embedding_stage.py
git commit -m "feat: EmbeddingStage with batch OpenAI embed"
```

---

### Task 14: IndexingStage — запись в PostgreSQL

**Objective:** INSERT documents + chunks (L1 с векторами, L2 без), tsvector через raw SQL.

**Files:**
- Create: `src/db/repositories/document_repo.py`
- Create: `src/db/repositories/chunk_repo.py`
- Create: `src/ingestion/indexing.py`
- Create: `tests/integration/test_indexing_stage.py`

**Step 1: Репозитории**

```python
# src/db/repositories/document_repo.py
import uuid
import hashlib
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from src.db.models import Document


class DocumentRepository:
    def __init__(self, session: AsyncSession):
        self.session = session

    async def get_by_checksum(self, checksum: str) -> Document | None:
        result = await self.session.execute(
            select(Document).where(Document.checksum == checksum)
        )
        return result.scalar_one_or_none()

    async def create(self, source: str, mime_type: str, checksum: str,
                     title: str | None = None, metadata: dict | None = None) -> Document:
        doc = Document(
            id=uuid.uuid4(),
            source=source,
            mime_type=mime_type,
            checksum=checksum,
            title=title,
            metadata_=metadata or {},
        )
        self.session.add(doc)
        await self.session.flush()
        return doc

    async def delete_chunks(self, doc_id: uuid.UUID) -> None:
        from sqlalchemy import delete
        from src.db.models import Chunk
        await self.session.execute(delete(Chunk).where(Chunk.doc_id == doc_id))
```

```python
# src/db/repositories/chunk_repo.py
import uuid
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import text
from src.models.chunk import ChunkData


class ChunkRepository:
    def __init__(self, session: AsyncSession):
        self.session = session

    async def bulk_insert(
        self,
        doc_id: uuid.UUID,
        chunks: list[ChunkData],
        parent_ids: dict[int, uuid.UUID],  # parent_temp_index → uuid
    ) -> None:
        for chunk in chunks:
            chunk_id = uuid.uuid4()
            parent_id = parent_ids.get(chunk.parent_temp_index) if chunk.parent_temp_index is not None else None
            pg_lang = chunk.language if chunk.language in ("russian","english","german","french","simple") else "simple"

            await self.session.execute(text("""
                INSERT INTO chunks
                    (id, doc_id, parent_chunk_id, chunk_role, chunk_index,
                     section_heading, section_level, page_number,
                     language, content, content_vector, tsv)
                VALUES
                    (:id, :doc_id, :parent_id, :role, :idx,
                     :heading, :level, :page,
                     :lang, :content,
                     :vector,
                     to_tsvector(:pg_lang::regconfig, :content))
            """), {
                "id": chunk_id,
                "doc_id": doc_id,
                "parent_id": parent_id,
                "role": chunk.chunk_role,
                "idx": chunk.chunk_index,
                "heading": chunk.section_heading,
                "level": chunk.section_level,
                "page": chunk.page_number,
                "lang": chunk.language,
                "content": chunk.content,
                "vector": chunk.embedding,
                "pg_lang": pg_lang,
            })
```

**Step 2: IndexingStage**

```python
# src/ingestion/indexing.py
import hashlib
import uuid
from sqlalchemy.ext.asyncio import AsyncSession
from src.db.repositories.document_repo import DocumentRepository
from src.db.repositories.chunk_repo import ChunkRepository
from src.models.chunk import ChunkData
from src.models.parsed import ParsedDocument


class IndexingStage:
    async def index(
        self,
        session: AsyncSession,
        parsed_doc: ParsedDocument,
        chunks: list[ChunkData],
        checksum: str,
    ) -> uuid.UUID:
        doc_repo = DocumentRepository(session)
        chunk_repo = ChunkRepository(session)

        # Создать документ
        doc = await doc_repo.create(
            source=parsed_doc.source,
            mime_type=parsed_doc.mime_type,
            checksum=checksum,
            title=parsed_doc.metadata.get("title"),
            metadata=parsed_doc.metadata,
        )

        # Сначала вставить L2 (parents), собрать их UUID
        parents = [c for c in chunks if c.chunk_role == "parent"]
        parent_ids: dict[int, uuid.UUID] = {}

        for parent in parents:
            parent_id = uuid.uuid4()
            parent_ids[parent.chunk_index] = parent_id

        # Переиспользуем bulk_insert, передаём готовые parent_ids
        # Вставляем parents отдельно (без parent_chunk_id)
        for parent in parents:
            from sqlalchemy import text
            pg_lang = "simple"
            await session.execute(text("""
                INSERT INTO chunks (id, doc_id, chunk_role, chunk_index,
                    section_heading, section_level, page_number, language, content, tsv)
                VALUES (:id, :doc_id, :role, :idx, :heading, :level, :page, :lang, :content,
                    to_tsvector(:pg_lang::regconfig, :content))
            """), {
                "id": parent_ids[parent.chunk_index],
                "doc_id": doc.id,
                "role": "parent",
                "idx": parent.chunk_index,
                "heading": parent.section_heading,
                "level": parent.section_level,
                "page": parent.page_number,
                "lang": parent.language,
                "content": parent.content,
                "pg_lang": pg_lang,
            })

        # Вставить L1 (leaves)
        leaves = [c for c in chunks if c.chunk_role == "leaf"]
        await chunk_repo.bulk_insert(doc.id, leaves, parent_ids)

        return doc.id
```

**Step 3: Интеграционный тест**

```python
# tests/integration/test_indexing_stage.py
import pytest
from src.ingestion.indexing import IndexingStage
from src.models.parsed import ParsedDocument, Section
from src.models.chunk import ChunkData
from tests.mocks.mock_embedding import MockEmbeddingClient

@pytest.mark.integration
async def test_indexing_creates_document_and_chunks(db_session):
    parsed = ParsedDocument(
        source="test.txt",
        mime_type="text/plain",
        sections=[Section(content="Hello world. This is a test document with enough content.")],
    )
    chunks = [
        ChunkData(content="Hello world.", chunk_role="parent", chunk_index=0),
        ChunkData(content="Hello", chunk_role="leaf", chunk_index=0, parent_temp_index=0,
                  embedding=[0.1] * 1536),
        ChunkData(content="world.", chunk_role="leaf", chunk_index=1, parent_temp_index=0,
                  embedding=[0.2] * 1536),
    ]

    stage = IndexingStage()
    doc_id = await stage.index(db_session, parsed, chunks, checksum="abc123")

    assert doc_id is not None

    from sqlalchemy import text
    result = await db_session.execute(
        text("SELECT count(*) FROM chunks WHERE doc_id = :id"), {"id": doc_id}
    )
    assert result.scalar() == 3  # 1 parent + 2 leaves
```

**Step 4: Запустить + commit**

```bash
pytest tests/integration/test_indexing_stage.py -v -m integration
git add src/db/repositories/ src/ingestion/indexing.py tests/
git commit -m "feat: IndexingStage with document and chunk repositories"
```

---

### Task 15: IngestionWorker — фоновая обработка очереди

**Objective:** asyncio task читает `ingestion_jobs`, запускает полный pipeline, обновляет статус.

**Files:**
- Create: `src/db/repositories/job_repo.py`
- Create: `src/ingestion/worker.py`
- Create: `src/ingestion/pipeline.py`
- Create: `tests/integration/test_ingestion_worker.py`

**Step 1: JobRepository**

```python
# src/db/repositories/job_repo.py
import uuid
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, text, update
from src.db.models import IngestionJob


class JobRepository:
    def __init__(self, session: AsyncSession):
        self.session = session

    async def create(self, source: str, checksum: str) -> IngestionJob:
        job = IngestionJob(id=uuid.uuid4(), source=source, checksum=checksum)
        self.session.add(job)
        await self.session.flush()
        return job

    async def get_by_checksum_active(self, checksum: str) -> IngestionJob | None:
        result = await self.session.execute(
            select(IngestionJob).where(
                IngestionJob.checksum == checksum,
                IngestionJob.status.in_(["pending", "processing"]),
            )
        )
        return result.scalar_one_or_none()

    async def claim_next(self, session: AsyncSession) -> IngestionJob | None:
        result = await session.execute(text("""
            SELECT id, source, checksum FROM ingestion_jobs
            WHERE status = 'pending'
            ORDER BY created_at
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        """))
        row = result.fetchone()
        if not row:
            return None
        await session.execute(text("""
            UPDATE ingestion_jobs SET status = 'processing', updated_at = now()
            WHERE id = :id
        """), {"id": row.id})
        return await session.get(IngestionJob, row.id)

    async def mark_done(self, job_id: uuid.UUID, doc_id: uuid.UUID) -> None:
        await self.session.execute(
            update(IngestionJob)
            .where(IngestionJob.id == job_id)
            .values(status="done", doc_id=doc_id)
        )

    async def mark_error(self, job_id: uuid.UUID, error: str) -> None:
        await self.session.execute(
            update(IngestionJob)
            .where(IngestionJob.id == job_id)
            .values(status="error", error=error[:2000])
        )
```

**Step 2: IngestionPipeline (собирает все шаги)**

```python
# src/ingestion/pipeline.py
import hashlib
from pathlib import Path
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker
from src.adapters.registry import AdapterRegistry
from src.adapters.protocol import Source
from src.ingestion.chunker import SmallToBigChunker
from src.ingestion.language import LanguageDetector
from src.ingestion.embedding import EmbeddingStage
from src.ingestion.indexing import IndexingStage
from src.config import Settings


class IngestionPipeline:
    def __init__(
        self,
        adapter_registry: AdapterRegistry,
        chunker: SmallToBigChunker,
        embedding_stage: EmbeddingStage,
        indexing_stage: IndexingStage,
        language_detector: LanguageDetector,
    ):
        self.adapter_registry = adapter_registry
        self.chunker = chunker
        self.embedding_stage = embedding_stage
        self.indexing_stage = indexing_stage
        self.language_detector = language_detector

    async def process(self, session: AsyncSession, source_path: str, checksum: str):
        import mimetypes
        mime_type, _ = mimetypes.guess_type(source_path)
        mime_type = mime_type or "application/octet-stream"

        source = Source(
            path=source_path,
            mime_type=mime_type,
            filename=Path(source_path).name,
        )

        parsed = self.adapter_registry.parse(source)

        chunks = self.chunker.chunk(parsed)

        for chunk in chunks:
            chunk.language = self.language_detector.detect(chunk.content[:200])

        chunks = await self.embedding_stage.process(chunks)

        doc_id = await self.indexing_stage.index(session, parsed, chunks, checksum)
        return doc_id
```

**Step 3: IngestionWorker**

```python
# src/ingestion/worker.py
import asyncio
import logging
from sqlalchemy.ext.asyncio import async_sessionmaker
from src.db.repositories.job_repo import JobRepository
from src.ingestion.pipeline import IngestionPipeline

logger = logging.getLogger(__name__)


class IngestionWorker:
    def __init__(self, session_factory: async_sessionmaker, pipeline: IngestionPipeline):
        self.session_factory = session_factory
        self.pipeline = pipeline
        self._running = False

    async def start(self):
        self._running = True
        logger.info("IngestionWorker started")
        while self._running:
            processed = await self._process_one()
            if not processed:
                await asyncio.sleep(1)

    def stop(self):
        self._running = False

    async def _process_one(self) -> bool:
        async with self.session_factory() as session:
            async with session.begin():
                repo = JobRepository(session)
                job = await repo.claim_next(session)
                if not job:
                    return False

                try:
                    doc_id = await self.pipeline.process(session, job.source, job.checksum)
                    await repo.mark_done(job.id, doc_id)
                    logger.info(f"Job {job.id} done → doc {doc_id}")
                    return True
                except Exception as e:
                    await repo.mark_error(job.id, str(e))
                    logger.exception(f"Job {job.id} failed: {e}")
                    return True  # задача обработана (с ошибкой), продолжаем
```

**Step 4: Интеграционный тест**

```python
# tests/integration/test_ingestion_worker.py
import pytest
import asyncio
from pathlib import Path
from src.ingestion.worker import IngestionWorker
from src.ingestion.pipeline import IngestionPipeline
from src.adapters.registry import AdapterRegistry
from src.adapters.text import TextAdapter
from src.ingestion.chunker import SmallToBigChunker
from src.ingestion.language import LanguageDetector
from src.ingestion.embedding import EmbeddingStage
from src.ingestion.indexing import IndexingStage
from src.db.repositories.job_repo import JobRepository
from tests.mocks.mock_embedding import MockEmbeddingClient

@pytest.mark.integration
async def test_worker_processes_job(db_session, session_factory, tmp_path):
    # Создать тестовый файл
    test_file = tmp_path / "test.txt"
    test_file.write_text("Hello world. This is a test document.")

    # Создать job
    async with session_factory() as session:
        async with session.begin():
            repo = JobRepository(session)
            job = await repo.create(source=str(test_file), checksum="test123")
            job_id = job.id

    # Собрать pipeline
    registry = AdapterRegistry()
    registry.register(TextAdapter())
    pipeline = IngestionPipeline(
        adapter_registry=registry,
        chunker=SmallToBigChunker(l2_size=50, l1_size=20, l2_overlap=5),
        embedding_stage=EmbeddingStage(client=MockEmbeddingClient()),
        indexing_stage=IndexingStage(),
        language_detector=LanguageDetector(),
    )

    worker = IngestionWorker(session_factory=session_factory, pipeline=pipeline)
    processed = await worker._process_one()
    assert processed is True

    # Проверить статус
    from sqlalchemy import select
    from src.db.models import IngestionJob
    async with session_factory() as session:
        result = await session.execute(select(IngestionJob).where(IngestionJob.id == job_id))
        job = result.scalar_one()
        assert job.status == "done"
        assert job.doc_id is not None
```

**Step 5: Запустить + commit**

```bash
pytest tests/integration/test_ingestion_worker.py -v -m integration
git add src/ingestion/ src/db/repositories/ tests/
git commit -m "feat: IngestionPipeline and IngestionWorker with PG queue"
```

---

## Phase 4: Retrieval Pipeline

### Task 16: LLM Provider Abstraction

**Objective:** Protocol + MockProvider + ClaudeProvider + OpenAIProvider + factory.

**Files:**
- Create: `src/llm/protocol.py`
- Create: `src/llm/claude.py`
- Create: `src/llm/openai.py`
- Create: `src/llm/factory.py`
- Create: `tests/mocks/mock_llm.py`
- Create: `tests/unit/test_llm_providers.py`

**Step 1: Protocol**

```python
# src/llm/protocol.py
from dataclasses import dataclass
from typing import Protocol


@dataclass
class LLMResponse:
    answer: str
    input_tokens: int = 0
    output_tokens: int = 0


class LLMProvider(Protocol):
    async def complete(self, prompt: str) -> LLMResponse: ...
```

**Step 2: MockProvider**

```python
# tests/mocks/mock_llm.py
from src.llm.protocol import LLMProvider, LLMResponse


class MockLLMProvider:
    def __init__(self, response: str = "Mock answer"):
        self.response = response
        self.calls: list[str] = []

    async def complete(self, prompt: str) -> LLMResponse:
        self.calls.append(prompt)
        return LLMResponse(answer=self.response, input_tokens=10, output_tokens=5)
```

**Step 3: ClaudeProvider**

```python
# src/llm/claude.py
from src.llm.protocol import LLMResponse


class ClaudeProvider:
    def __init__(self, api_key: str, model: str = "claude-opus-4-7",
                 max_tokens: int = 2048, temperature: float = 0.1):
        import anthropic
        self._client = anthropic.AsyncAnthropic(api_key=api_key)
        self.model = model
        self.max_tokens = max_tokens
        self.temperature = temperature

    async def complete(self, prompt: str) -> LLMResponse:
        response = await self._client.messages.create(
            model=self.model,
            max_tokens=self.max_tokens,
            temperature=self.temperature,
            messages=[{"role": "user", "content": prompt}],
        )
        return LLMResponse(
            answer=response.content[0].text,
            input_tokens=response.usage.input_tokens,
            output_tokens=response.usage.output_tokens,
        )
```

**Step 4: OpenAIProvider**

```python
# src/llm/openai.py
from src.llm.protocol import LLMResponse


class OpenAIProvider:
    def __init__(self, api_key: str, model: str = "gpt-4o",
                 max_tokens: int = 2048, temperature: float = 0.1):
        import openai
        self._client = openai.AsyncOpenAI(api_key=api_key)
        self.model = model
        self.max_tokens = max_tokens
        self.temperature = temperature

    async def complete(self, prompt: str) -> LLMResponse:
        response = await self._client.chat.completions.create(
            model=self.model,
            max_tokens=self.max_tokens,
            temperature=self.temperature,
            messages=[{"role": "user", "content": prompt}],
        )
        choice = response.choices[0]
        usage = response.usage
        return LLMResponse(
            answer=choice.message.content or "",
            input_tokens=usage.prompt_tokens if usage else 0,
            output_tokens=usage.completion_tokens if usage else 0,
        )
```

**Step 5: Factory**

```python
# src/llm/factory.py
from src.config import Settings
from src.llm.protocol import LLMProvider


def create_llm_provider(settings: Settings) -> LLMProvider:
    if settings.llm_provider == "claude":
        from src.llm.claude import ClaudeProvider
        return ClaudeProvider(
            api_key=settings.anthropic_api_key or "",
            model=settings.llm_model,
            max_tokens=settings.llm_max_tokens,
            temperature=settings.llm_temperature,
        )
    elif settings.llm_provider == "openai":
        from src.llm.openai import OpenAIProvider
        return OpenAIProvider(
            api_key=settings.openai_llm_api_key or "",
            model=settings.llm_model,
            max_tokens=settings.llm_max_tokens,
            temperature=settings.llm_temperature,
        )
    raise ValueError(f"Unknown LLM provider: {settings.llm_provider}")
```

**Step 6: Тест**

```python
# tests/unit/test_llm_providers.py
import pytest
from tests.mocks.mock_llm import MockLLMProvider

@pytest.mark.asyncio
async def test_mock_provider_returns_response():
    provider = MockLLMProvider(response="Test answer")
    result = await provider.complete("What is 2+2?")
    assert result.answer == "Test answer"
    assert len(provider.calls) == 1
    assert "What is 2+2?" in provider.calls[0]
```

**Step 7: Запустить + commit**

```bash
pytest tests/unit/test_llm_providers.py -v
git add src/llm/ tests/mocks/mock_llm.py tests/unit/test_llm_providers.py
git commit -m "feat: LLMProvider protocol, Claude/OpenAI providers, factory"
```

---

### Task 17: SemanticSearch + BM25Search

**Objective:** Два независимых поиска по L1 чанкам.

**Files:**
- Create: `src/retrieval/semantic.py`
- Create: `src/retrieval/bm25.py`
- Create: `tests/integration/test_search.py`

**Step 1: SemanticSearch**

```python
# src/retrieval/semantic.py
import uuid
from dataclasses import dataclass
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import text


@dataclass
class SearchHit:
    chunk_id: uuid.UUID
    content: str
    parent_chunk_id: uuid.UUID | None
    doc_id: uuid.UUID
    score: float
    section_heading: str | None = None
    page_number: int | None = None


class SemanticSearch:
    def __init__(self, top_k: int = 20):
        self.top_k = top_k

    async def search(
        self, session: AsyncSession, query_vector: list[float], top_k: int | None = None
    ) -> list[SearchHit]:
        k = top_k or self.top_k
        result = await session.execute(text("""
            SELECT id, content, parent_chunk_id, doc_id,
                   section_heading, page_number,
                   1 - (content_vector <=> :vec::vector) AS score
            FROM chunks
            WHERE chunk_role = 'leaf'
              AND content_vector IS NOT NULL
            ORDER BY content_vector <=> :vec::vector
            LIMIT :k
        """), {"vec": str(query_vector), "k": k})

        return [
            SearchHit(
                chunk_id=row.id,
                content=row.content,
                parent_chunk_id=row.parent_chunk_id,
                doc_id=row.doc_id,
                score=float(row.score),
                section_heading=row.section_heading,
                page_number=row.page_number,
            )
            for row in result
        ]
```

**Step 2: BM25Search**

```python
# src/retrieval/bm25.py
import uuid
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import text
from src.retrieval.semantic import SearchHit
from src.ingestion.language import LanguageDetector


class BM25Search:
    def __init__(self, top_k: int = 20):
        self.top_k = top_k
        self._lang_detector = LanguageDetector()

    async def search(
        self, session: AsyncSession, query: str, top_k: int | None = None
    ) -> list[SearchHit]:
        k = top_k or self.top_k
        lang = self._lang_detector.detect(query)
        pg_config = self._lang_detector.to_pg_config(lang)

        result = await session.execute(text("""
            SELECT c.id, c.content, c.parent_chunk_id, c.doc_id,
                   c.section_heading, c.page_number,
                   ts_rank(c.tsv, query) AS score
            FROM chunks c,
                 plainto_tsquery(:config::regconfig, :query) query
            WHERE c.chunk_role = 'leaf'
              AND c.tsv @@ query
            ORDER BY score DESC
            LIMIT :k
        """), {"config": pg_config, "query": query, "k": k})

        return [
            SearchHit(
                chunk_id=row.id,
                content=row.content,
                parent_chunk_id=row.parent_chunk_id,
                doc_id=row.doc_id,
                score=float(row.score),
                section_heading=row.section_heading,
                page_number=row.page_number,
            )
            for row in result
        ]
```

**Step 3: Интеграционный тест**

```python
# tests/integration/test_search.py
import pytest
import uuid
from sqlalchemy import text
from src.retrieval.semantic import SemanticSearch
from src.retrieval.bm25 import BM25Search

@pytest.mark.integration
async def test_bm25_finds_exact_word(db_session):
    # Вставить тестовый чанк
    doc_id = uuid.uuid4()
    chunk_id = uuid.uuid4()
    await db_session.execute(text("""
        INSERT INTO documents (id, source, mime_type, checksum)
        VALUES (:id, 'test.txt', 'text/plain', :cs)
    """), {"id": doc_id, "cs": f"cs-{doc_id}"})

    await db_session.execute(text("""
        INSERT INTO chunks (id, doc_id, chunk_role, chunk_index, language, content, tsv)
        VALUES (:id, :doc_id, 'leaf', 0, 'english', 'PostgreSQL indexing guide',
                to_tsvector('english', 'PostgreSQL indexing guide'))
    """), {"id": chunk_id, "doc_id": doc_id})

    search = BM25Search(top_k=5)
    results = await search.search(db_session, "PostgreSQL indexing")
    assert len(results) >= 1
    assert any(r.chunk_id == chunk_id for r in results)
```

**Step 4: Запустить + commit**

```bash
pytest tests/integration/test_search.py -v -m integration
git add src/retrieval/semantic.py src/retrieval/bm25.py tests/integration/test_search.py
git commit -m "feat: SemanticSearch (pgvector) and BM25Search (tsvector)"
```

---

### Task 18: RRF Merger + Expand to L2 + Reranker + ContextBuilder

**Objective:** Финальные шаги retrieval pipeline.

**Files:**
- Create: `src/retrieval/rrf.py`
- Create: `src/retrieval/expand.py`
- Create: `src/retrieval/reranker.py`
- Create: `src/retrieval/context.py`
- Create: `tests/unit/test_rrf.py`
- Create: `tests/unit/test_context_builder.py`

**Step 1: RRF Merger**

```python
# src/retrieval/rrf.py
import uuid
from dataclasses import dataclass
from src.retrieval.semantic import SearchHit


def rrf_merge(
    semantic_hits: list[SearchHit],
    bm25_hits: list[SearchHit],
    k: int = 60,
    top_n: int = 20,
) -> list[SearchHit]:
    scores: dict[uuid.UUID, float] = {}
    hit_map: dict[uuid.UUID, SearchHit] = {}

    for rank, hit in enumerate(semantic_hits, start=1):
        scores[hit.chunk_id] = scores.get(hit.chunk_id, 0) + 1 / (rank + k)
        hit_map[hit.chunk_id] = hit

    for rank, hit in enumerate(bm25_hits, start=1):
        scores[hit.chunk_id] = scores.get(hit.chunk_id, 0) + 1 / (rank + k)
        if hit.chunk_id not in hit_map:
            hit_map[hit.chunk_id] = hit

    ranked = sorted(scores.items(), key=lambda x: x[1], reverse=True)
    return [hit_map[chunk_id] for chunk_id, _ in ranked[:top_n]]
```

**Step 2: Тест RRF**

```python
# tests/unit/test_rrf.py
import uuid
from src.retrieval.rrf import rrf_merge
from src.retrieval.semantic import SearchHit


def make_hit(chunk_id: str, score: float = 0.5) -> SearchHit:
    return SearchHit(
        chunk_id=uuid.UUID(chunk_id),
        content="text",
        parent_chunk_id=None,
        doc_id=uuid.uuid4(),
        score=score,
    )


def test_rrf_both_lists_boost_score():
    shared_id = "00000000-0000-0000-0000-000000000001"
    semantic = [make_hit(shared_id), make_hit("00000000-0000-0000-0000-000000000002")]
    bm25 = [make_hit(shared_id), make_hit("00000000-0000-0000-0000-000000000003")]

    result = rrf_merge(semantic, bm25, k=60)
    assert result[0].chunk_id == uuid.UUID(shared_id)  # в обоих списках → первый


def test_rrf_deduplicates():
    shared_id = "00000000-0000-0000-0000-000000000001"
    hits = [make_hit(shared_id)] * 3
    result = rrf_merge(hits, hits, k=60)
    ids = [r.chunk_id for r in result]
    assert len(ids) == len(set(ids))
```

**Step 3: Expand to L2**

```python
# src/retrieval/expand.py
import uuid
from dataclasses import dataclass
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import text
from src.retrieval.semantic import SearchHit


@dataclass
class L2Chunk:
    chunk_id: uuid.UUID
    content: str
    doc_id: uuid.UUID
    section_heading: str | None
    page_number: int | None
    doc_title: str | None


async def expand_to_l2(
    session: AsyncSession,
    hits: list[SearchHit],
) -> list[L2Chunk]:
    parent_ids = list({h.parent_chunk_id for h in hits if h.parent_chunk_id})
    if not parent_ids:
        return []

    result = await session.execute(text("""
        SELECT c.id, c.content, c.doc_id, c.section_heading, c.page_number,
               d.title AS doc_title
        FROM chunks c
        JOIN documents d ON d.id = c.doc_id
        WHERE c.id = ANY(:ids)
    """), {"ids": parent_ids})

    return [
        L2Chunk(
            chunk_id=row.id,
            content=row.content,
            doc_id=row.doc_id,
            section_heading=row.section_heading,
            page_number=row.page_number,
            doc_title=row.doc_title,
        )
        for row in result
    ]
```

**Step 4: Reranker**

```python
# src/retrieval/reranker.py
import asyncio
from src.retrieval.expand import L2Chunk


class Reranker:
    _model = None
    MODEL_NAME = "cross-encoder/ms-marco-MiniLM-L-6-v2"

    def _get_model(self):
        if self._model is None:
            from sentence_transformers import CrossEncoder
            Reranker._model = CrossEncoder(self.MODEL_NAME)
        return self._model

    async def rerank(self, query: str, chunks: list[L2Chunk], top_n: int = 5) -> list[L2Chunk]:
        if not chunks:
            return []

        loop = asyncio.get_event_loop()
        pairs = [(query, c.content) for c in chunks]

        scores = await loop.run_in_executor(
            None,
            lambda: self._get_model().predict(pairs),
        )

        ranked = sorted(zip(chunks, scores), key=lambda x: x[1], reverse=True)
        return [chunk for chunk, _ in ranked[:top_n]]
```

**Step 5: ContextBuilder**

```python
# src/retrieval/context.py
from dataclasses import dataclass
from src.retrieval.expand import L2Chunk


@dataclass
class QueryContext:
    prompt: str
    sources: list[dict]


class ContextBuilder:
    SYSTEM = (
        "Отвечай только на основе предоставленных источников. "
        "Если ответа нет в источниках — скажи об этом явно. "
        "Цитируй источники как [1], [2] и т.д."
    )

    def build(self, query: str, chunks: list[L2Chunk]) -> QueryContext:
        sources_text = ""
        sources_meta = []

        for i, chunk in enumerate(chunks, start=1):
            parts = [f"[{i}]"]
            if chunk.doc_title:
                parts.append(chunk.doc_title)
            if chunk.section_heading:
                parts.append(f"— {chunk.section_heading}")
            if chunk.page_number:
                parts.append(f"(стр. {chunk.page_number})")

            sources_text += "\n" + " ".join(parts) + "\n"
            sources_text += "---\n"
            sources_text += chunk.content + "\n"

            sources_meta.append({
                "index": i,
                "doc_id": str(chunk.doc_id),
                "title": chunk.doc_title,
                "section": chunk.section_heading,
                "page": chunk.page_number,
                "preview": chunk.content[:200],
            })

        prompt = f"{self.SYSTEM}\n\nИсточники:\n{sources_text}\nВопрос: {query}"
        return QueryContext(prompt=prompt, sources=sources_meta)
```

**Step 6: Тест ContextBuilder**

```python
# tests/unit/test_context_builder.py
import uuid
from src.retrieval.context import ContextBuilder
from src.retrieval.expand import L2Chunk


def test_context_builder_includes_query():
    builder = ContextBuilder()
    chunk = L2Chunk(
        chunk_id=uuid.uuid4(),
        content="PostgreSQL supports GIN indexes for JSONB.",
        doc_id=uuid.uuid4(),
        section_heading="Indexes",
        page_number=4,
        doc_title="PG Guide",
    )
    ctx = builder.build("how to index JSONB?", [chunk])
    assert "how to index JSONB?" in ctx.prompt
    assert "GIN indexes" in ctx.prompt
    assert "[1]" in ctx.prompt
    assert len(ctx.sources) == 1
    assert ctx.sources[0]["section"] == "Indexes"
```

**Step 7: Запустить + commit**

```bash
pytest tests/unit/test_rrf.py tests/unit/test_context_builder.py -v
git add src/retrieval/ tests/unit/test_rrf.py tests/unit/test_context_builder.py
git commit -m "feat: RRF merger, L2 expand, reranker, context builder"
```

---

### Task 19: RetrievalService — единый фасад поиска

**Objective:** Собирает весь retrieval pipeline в один вызов.

**Files:**
- Create: `src/retrieval/service.py`
- Create: `tests/integration/test_retrieval_service.py`

**Step 1: RetrievalService**

```python
# src/retrieval/service.py
from dataclasses import dataclass
from sqlalchemy.ext.asyncio import AsyncSession
from src.retrieval.semantic import SemanticSearch
from src.retrieval.bm25 import BM25Search
from src.retrieval.rrf import rrf_merge
from src.retrieval.expand import expand_to_l2
from src.retrieval.reranker import Reranker
from src.retrieval.context import ContextBuilder
from src.llm.protocol import LLMProvider, LLMResponse


@dataclass
class QueryResult:
    answer: str
    sources: list[dict]
    input_tokens: int
    output_tokens: int


class RetrievalService:
    def __init__(
        self,
        semantic_search: SemanticSearch,
        bm25_search: BM25Search,
        reranker: Reranker,
        context_builder: ContextBuilder,
        llm_provider: LLMProvider,
        rrf_k: int = 60,
        reranker_top_n: int = 5,
    ):
        self.semantic_search = semantic_search
        self.bm25_search = bm25_search
        self.reranker = reranker
        self.context_builder = context_builder
        self.llm_provider = llm_provider
        self.rrf_k = rrf_k
        self.reranker_top_n = reranker_top_n

    async def query(self, session: AsyncSession, query: str, embed_fn) -> QueryResult:
        query_vector = await embed_fn(query)

        semantic_hits, bm25_hits = await asyncio.gather(
            self.semantic_search.search(session, query_vector),
            self.bm25_search.search(session, query),
        )

        merged = rrf_merge(semantic_hits, bm25_hits, k=self.rrf_k)
        l2_chunks = await expand_to_l2(session, merged)
        reranked = await self.reranker.rerank(query, l2_chunks, top_n=self.reranker_top_n)

        ctx = self.context_builder.build(query, reranked)
        llm_response = await self.llm_provider.complete(ctx.prompt)

        return QueryResult(
            answer=llm_response.answer,
            sources=ctx.sources,
            input_tokens=llm_response.input_tokens,
            output_tokens=llm_response.output_tokens,
        )


import asyncio
```

**Step 2: Commit**

```bash
git add src/retrieval/service.py
git commit -m "feat: RetrievalService facade for full query pipeline"
```

---

## Phase 5: REST API

### Task 20: FastAPI app + lifespan + DI

**Objective:** Инициализация приложения, запуск worker в lifespan, DI фабрики.

**Files:**
- Create: `src/main.py`
- Create: `src/dependencies.py`

**Step 1: Dependencies**

```python
# src/dependencies.py
from functools import lru_cache
from src.config import get_settings, Settings
from src.adapters.registry import AdapterRegistry
from src.adapters.text import TextAdapter
from src.adapters.markdown import MarkdownAdapter
from src.adapters.pdf import PdfAdapter
from src.adapters.docx import DocxAdapter
from src.ingestion.chunker import SmallToBigChunker
from src.ingestion.language import LanguageDetector
from src.ingestion.embedding import EmbeddingStage, OpenAIEmbeddingClient
from src.ingestion.indexing import IndexingStage
from src.ingestion.pipeline import IngestionPipeline
from src.retrieval.semantic import SemanticSearch
from src.retrieval.bm25 import BM25Search
from src.retrieval.reranker import Reranker
from src.retrieval.context import ContextBuilder
from src.retrieval.service import RetrievalService
from src.llm.factory import create_llm_provider


@lru_cache
def get_adapter_registry() -> AdapterRegistry:
    registry = AdapterRegistry()
    registry.register(PdfAdapter())
    registry.register(DocxAdapter())
    registry.register(MarkdownAdapter())
    registry.register(TextAdapter())
    return registry


@lru_cache
def get_embedding_client():
    settings = get_settings()
    return OpenAIEmbeddingClient(api_key=settings.openai_api_key, model=settings.embedding_model)


@lru_cache
def get_ingestion_pipeline() -> IngestionPipeline:
    settings = get_settings()
    return IngestionPipeline(
        adapter_registry=get_adapter_registry(),
        chunker=SmallToBigChunker(
            l2_size=settings.l2_chunk_size,
            l1_size=settings.l1_chunk_size,
            l2_overlap=settings.l2_chunk_overlap,
        ),
        embedding_stage=EmbeddingStage(client=get_embedding_client()),
        indexing_stage=IndexingStage(),
        language_detector=LanguageDetector(),
    )


@lru_cache
def get_retrieval_service() -> RetrievalService:
    settings = get_settings()
    return RetrievalService(
        semantic_search=SemanticSearch(top_k=settings.semantic_top_k),
        bm25_search=BM25Search(top_k=settings.bm25_top_k),
        reranker=Reranker(),
        context_builder=ContextBuilder(),
        llm_provider=create_llm_provider(settings),
        rrf_k=settings.rrf_k,
        reranker_top_n=settings.reranker_top_n,
    )
```

**Step 2: Main app с lifespan**

```python
# src/main.py
import asyncio
from contextlib import asynccontextmanager
from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from src.db.session import init_db, close_db, get_session_factory
from src.ingestion.worker import IngestionWorker
from src.dependencies import get_ingestion_pipeline
from src.config import get_settings


_worker: IngestionWorker | None = None
_worker_task = None


@asynccontextmanager
async def lifespan(app: FastAPI):
    global _worker, _worker_task
    settings = get_settings()

    await init_db(settings.database_url)
    settings.upload_dir.mkdir(parents=True, exist_ok=True)

    _worker = IngestionWorker(
        session_factory=get_session_factory(),
        pipeline=get_ingestion_pipeline(),
    )
    _worker_task = asyncio.create_task(_worker.start())

    yield

    if _worker:
        _worker.stop()
    if _worker_task:
        _worker_task.cancel()
    await close_db()


app = FastAPI(title="Memex", lifespan=lifespan)
app.mount("/static", StaticFiles(directory="static"), name="static")

from src.api import documents, query, jobs
from src.ui import pages

app.include_router(documents.router, prefix="/api")
app.include_router(query.router, prefix="/api")
app.include_router(jobs.router, prefix="/api")
app.include_router(pages.router)
```

**Step 3: Commit**

```bash
git add src/main.py src/dependencies.py
git commit -m "feat: FastAPI app with lifespan, worker startup, DI"
```

---

### Task 21: API endpoints — documents, query, jobs

**Objective:** REST endpoints для загрузки, поиска и статуса.

**Files:**
- Create: `src/api/documents.py`
- Create: `src/api/query.py`
- Create: `src/api/jobs.py`
- Create: `tests/integration/test_api.py`

**Step 1: Documents endpoint**

```python
# src/api/documents.py
import hashlib
import shutil
import uuid
from fastapi import APIRouter, UploadFile, File, HTTPException, Depends
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession
from src.db.session import get_session_factory
from src.db.repositories.document_repo import DocumentRepository
from src.db.repositories.job_repo import JobRepository
from src.config import get_settings

router = APIRouter(tags=["documents"])


class DocumentResponse(BaseModel):
    doc_id: str | None
    job_id: str | None
    status: str


async def get_session():
    factory = get_session_factory()
    async with factory() as session:
        async with session.begin():
            yield session


@router.post("/documents", response_model=DocumentResponse)
async def upload_document(
    file: UploadFile = File(...),
    session: AsyncSession = Depends(get_session),
):
    settings = get_settings()
    content = await file.read()
    checksum = hashlib.sha256(content).hexdigest()

    doc_repo = DocumentRepository(session)
    existing_doc = await doc_repo.get_by_checksum(checksum)
    if existing_doc:
        return DocumentResponse(doc_id=str(existing_doc.id), job_id=None, status="already_indexed")

    job_repo = JobRepository(session)
    existing_job = await job_repo.get_by_checksum_active(checksum)
    if existing_job:
        return DocumentResponse(doc_id=None, job_id=str(existing_job.id), status="already_queued")

    dest = settings.upload_dir / f"{uuid.uuid4()}-{file.filename}"
    dest.write_bytes(content)

    job = await job_repo.create(source=str(dest), checksum=checksum)
    return DocumentResponse(doc_id=None, job_id=str(job.id), status="pending")


@router.get("/documents")
async def list_documents(session: AsyncSession = Depends(get_session)):
    from sqlalchemy import select
    from src.db.models import Document
    result = await session.execute(select(Document).order_by(Document.indexed_at.desc()))
    docs = result.scalars().all()
    return [{"id": str(d.id), "source": d.source, "title": d.title, "indexed_at": d.indexed_at} for d in docs]
```

**Step 2: Query endpoint**

```python
# src/api/query.py
from fastapi import APIRouter, Depends
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession
from src.api.documents import get_session
from src.dependencies import get_retrieval_service, get_embedding_client

router = APIRouter(tags=["query"])


class QueryRequest(BaseModel):
    query: str
    top_k: int = 5


class QueryResponse(BaseModel):
    answer: str
    sources: list[dict]


@router.post("/query", response_model=QueryResponse)
async def query_documents(
    request: QueryRequest,
    session: AsyncSession = Depends(get_session),
):
    service = get_retrieval_service()
    embedding_client = get_embedding_client()

    async def embed(text: str) -> list[float]:
        results = await embedding_client.embed_batch([text])
        return results[0]

    result = await service.query(session, request.query, embed_fn=embed)
    return QueryResponse(answer=result.answer, sources=result.sources)
```

**Step 3: Jobs endpoint**

```python
# src/api/jobs.py
from fastapi import APIRouter, HTTPException, Depends
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from src.api.documents import get_session
from src.db.models import IngestionJob
import uuid

router = APIRouter(tags=["jobs"])


class JobResponse(BaseModel):
    job_id: str
    status: str
    doc_id: str | None
    error: str | None


@router.get("/jobs/{job_id}", response_model=JobResponse)
async def get_job(job_id: uuid.UUID, session: AsyncSession = Depends(get_session)):
    result = await session.execute(select(IngestionJob).where(IngestionJob.id == job_id))
    job = result.scalar_one_or_none()
    if not job:
        raise HTTPException(status_code=404, detail="Job not found")
    return JobResponse(
        job_id=str(job.id),
        status=job.status,
        doc_id=str(job.doc_id) if job.doc_id else None,
        error=job.error,
    )
```

**Step 4: Commit**

```bash
git add src/api/
git commit -m "feat: REST API endpoints for documents, query, jobs"
```

---

## Phase 6: Web UI

### Task 22: Jinja2 шаблоны и базовые страницы

**Objective:** Базовый layout + три страницы UI.

**Files:**
- Create: `src/ui/pages.py`
- Create: `templates/base.html`
- Create: `templates/index.html`
- Create: `templates/documents.html`
- Create: `static/htmx.min.js` (скачать)

**Step 1: Скачать HTMX**

```bash
curl -o static/htmx.min.js https://unpkg.com/htmx.org@2.0.0/dist/htmx.min.js
```

**Step 2: `templates/base.html`**

```html
<!DOCTYPE html>
<html lang="ru">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Memex — {% block title %}{% endblock %}</title>
    <script src="/static/htmx.min.js"></script>
    <style>
        body { font-family: system-ui, sans-serif; max-width: 800px; margin: 40px auto; padding: 0 20px; }
        nav a { margin-right: 16px; }
        .sources { margin-top: 16px; font-size: 0.9em; color: #666; }
        .error { color: red; }
        .status { color: #888; font-size: 0.85em; }
    </style>
</head>
<body>
    <nav><a href="/">Поиск</a><a href="/documents">Документы</a><a href="/upload">Загрузить</a></nav>
    <hr>
    {% block content %}{% endblock %}
</body>
</html>
```

**Step 3: `templates/index.html`**

```html
{% extends "base.html" %}
{% block title %}Поиск{% endblock %}
{% block content %}
<h1>Memex</h1>
<form hx-post="/search" hx-target="#results" hx-swap="innerHTML">
    <input type="text" name="query" placeholder="Задай вопрос..." style="width:70%">
    <button type="submit">Найти</button>
</form>
<div id="results"></div>
{% endblock %}
```

**Step 4: `templates/documents.html`**

```html
{% extends "base.html" %}
{% block title %}Документы{% endblock %}
{% block content %}
<h1>Документы ({{ docs|length }})</h1>
<ul>
{% for doc in docs %}
    <li>{{ doc.title or doc.source }} <span class="status">{{ doc.indexed_at }}</span></li>
{% endfor %}
</ul>
{% endblock %}
```

**Step 5: `src/ui/pages.py`**

```python
from fastapi import APIRouter, Request, Form, Depends
from fastapi.responses import HTMLResponse
from fastapi.templating import Jinja2Templates
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from src.api.documents import get_session
from src.db.models import Document
from src.dependencies import get_retrieval_service, get_embedding_client

router = APIRouter(tags=["ui"])
templates = Jinja2Templates(directory="templates")


@router.get("/", response_class=HTMLResponse)
async def index(request: Request):
    return templates.TemplateResponse("index.html", {"request": request})


@router.get("/documents", response_class=HTMLResponse)
async def documents_page(request: Request, session: AsyncSession = Depends(get_session)):
    result = await session.execute(select(Document).order_by(Document.indexed_at.desc()))
    docs = result.scalars().all()
    return templates.TemplateResponse("documents.html", {"request": request, "docs": docs})


@router.post("/search", response_class=HTMLResponse)
async def search(
    request: Request,
    query: str = Form(...),
    session: AsyncSession = Depends(get_session),
):
    service = get_retrieval_service()
    client = get_embedding_client()

    async def embed(text):
        return (await client.embed_batch([text]))[0]

    try:
        result = await service.query(session, query, embed_fn=embed)
        return templates.TemplateResponse("_results.html", {
            "request": request,
            "answer": result.answer,
            "sources": result.sources,
        })
    except Exception as e:
        return HTMLResponse(f'<p class="error">Ошибка: {e}</p>')


@router.get("/upload", response_class=HTMLResponse)
async def upload_page(request: Request):
    return templates.TemplateResponse("upload.html", {"request": request})
```

**Step 6: `templates/_results.html` и `templates/upload.html`**

```html
<!-- templates/_results.html -->
<div class="answer">{{ answer }}</div>
<div class="sources">
{% for s in sources %}
  <div>[{{ s.index }}] {{ s.title }} — {{ s.section }} (стр. {{ s.page }})</div>
{% endfor %}
</div>
```

```html
<!-- templates/upload.html -->
{% extends "base.html" %}
{% block title %}Загрузить{% endblock %}
{% block content %}
<h1>Загрузить документ</h1>
<form hx-post="/api/documents" hx-encoding="multipart/form-data"
      hx-target="#status" hx-swap="innerHTML">
    <input type="file" name="file" accept=".pdf,.docx,.txt,.md">
    <button type="submit">Загрузить</button>
</form>
<div id="status"></div>
{% endblock %}
```

**Step 7: Commit**

```bash
git add src/ui/ templates/ static/htmx.min.js
git commit -m "feat: Web UI with Jinja2 + HTMX (search, documents, upload)"
```

---

## Phase 7: MCP Server

### Task 23: MCP Server с двумя инструментами

**Objective:** MCP stdio сервер с `add_document` и `query` инструментами.

**Files:**
- Create: `src/mcp/server.py`
- Create: `mcp_server.py` (точка входа)

**Step 1: `src/mcp/server.py`**

```python
# src/mcp/server.py
"""
MCP Server для Memex.
Запуск: python mcp_server.py
Подключение в Claude Code: добавить в .claude/settings.json:
{
  "mcpServers": {
    "memex": { "command": "python", "args": ["mcp_server.py"] }
  }
}
"""
import asyncio
import httpx
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp import types

BASE_URL = "http://localhost:8000"
server = Server("memex")


@server.list_tools()
async def list_tools() -> list[types.Tool]:
    return [
        types.Tool(
            name="add_document",
            description="Добавить документ в Memex для индексации",
            inputSchema={
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "Абсолютный путь к файлу"},
                },
                "required": ["file_path"],
            },
        ),
        types.Tool(
            name="query",
            description="Задать вопрос по проиндексированным документам",
            inputSchema={
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Вопрос на естественном языке"},
                },
                "required": ["query"],
            },
        ),
    ]


@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[types.TextContent]:
    async with httpx.AsyncClient() as client:
        if name == "add_document":
            file_path = arguments["file_path"]
            with open(file_path, "rb") as f:
                response = await client.post(
                    f"{BASE_URL}/api/documents",
                    files={"file": (file_path.split("/")[-1], f)},
                    timeout=30,
                )
            data = response.json()
            return [types.TextContent(type="text", text=str(data))]

        elif name == "query":
            response = await client.post(
                f"{BASE_URL}/api/query",
                json={"query": arguments["query"]},
                timeout=60,
            )
            data = response.json()
            answer = data.get("answer", "")
            sources = data.get("sources", [])
            text = answer + "\n\nИсточники:\n" + "\n".join(
                f"[{s['index']}] {s.get('title', '')} — {s.get('section', '')}" for s in sources
            )
            return [types.TextContent(type="text", text=text)]

    raise ValueError(f"Unknown tool: {name}")


async def main():
    async with stdio_server() as streams:
        await server.run(*streams, server.create_initialization_options())


if __name__ == "__main__":
    asyncio.run(main())
```

**Step 2: Добавить `mcp` в зависимости**

```toml
# pyproject.toml — в dependencies добавить:
"mcp>=1.0",
```

```bash
pip install mcp
```

**Step 3: Конфиг для Claude Code** — добавить в `.claude/settings.json`:

```json
{
  "mcpServers": {
    "memex": {
      "command": "python",
      "args": ["mcp_server.py"],
      "cwd": "/path/to/memex"
    }
  }
}
```

**Step 4: Commit**

```bash
git add src/mcp/ mcp_server.py pyproject.toml
git commit -m "feat: MCP server with add_document and query tools"
```

---

## Phase 8: Final

### Task 24: Docker Compose для локального запуска

**Objective:** Один файл для запуска PostgreSQL + приложения.

**Files:**
- Create: `docker-compose.yml`
- Create: `Dockerfile`

**Step 1: `docker-compose.yml`**

```yaml
services:
  postgres:
    image: pgvector/pgvector:pg15
    environment:
      POSTGRES_USER: memex
      POSTGRES_PASSWORD: memex
      POSTGRES_DB: memex
    ports:
      - "5432:5432"
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U memex"]
      interval: 5s
      retries: 5

  app:
    build: .
    ports:
      - "8000:8000"
    env_file: .env
    depends_on:
      postgres:
        condition: service_healthy
    volumes:
      - ./data:/app/data

volumes:
  pgdata:
```

**Step 2: `Dockerfile`**

```dockerfile
FROM python:3.12-slim

WORKDIR /app
COPY pyproject.toml .
RUN pip install -e .

COPY . .
RUN alembic upgrade head

CMD ["uvicorn", "src.main:app", "--host", "0.0.0.0", "--port", "8000"]
```

**Step 3: Запустить**

```bash
docker compose up -d postgres
alembic upgrade head
uvicorn src.main:app --reload
```

**Step 4: Commit финальный**

```bash
git add docker-compose.yml Dockerfile
git commit -m "feat: docker-compose for local development"
```

---

## Checklist перед завершением

- [ ] `pytest tests/unit/ -v` — все unit тесты зелёные
- [ ] `pytest tests/integration/ -v -m integration` — все интеграционные зелёные
- [ ] `uvicorn src.main:app --reload` запускается без ошибок
- [ ] `POST /api/documents` с файлом → `202 {job_id}`
- [ ] `GET /api/jobs/{job_id}` → `{status: "done"}`
- [ ] `POST /api/query` → `{answer, sources}`
- [ ] `GET /` → HTML страница с формой поиска
- [ ] `GET /documents` → список документов
