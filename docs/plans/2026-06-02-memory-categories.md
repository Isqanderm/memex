# Memory Categories Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `category` and `project` fields to memories so personal notes, research, reminders and decisions are distinguishable — and show that context in retrieval results.

**Architecture:** New columns on `memories` table extracted by LLM during `remember()`. `MemoryHit` exposes the fields. `ContextBuilder` formats them as `[memory | research | Memex | 2026-06-02]`. API and MCP get optional `category` filter.

**Tech Stack:** Python 3.12, SQLAlchemy 2.0 async, Alembic, FastAPI, pgvector, pytest-asyncio.

---

## File Map

```
Modified:
  src/db/models.py                        — add category + project columns
  src/memory/extractor.py                 — update EXTRACT_PROMPT, ExtractedFact
  src/db/repositories/memory_repo.py      — category filter in get_all_active + get_active_by_vector
  src/memory/service.py                   — pass category/project to repo.create()
  src/retrieval/memory_search.py          — expose category/project in MemoryHit, add filter
  src/retrieval/context.py                — display [memory | category | project | date]
  src/api/memories.py                     — category filter on /list and /query
  src/mcp/server.py                       — category param on recall tool

Created:
  alembic/versions/0006_memory_categories.py
  tests/unit/test_memory_categories.py
```

---

## Task 1: Migration 0006 — add category and project columns

**Files:**
- Create: `alembic/versions/0006_memory_categories.py`

- [ ] **Step 1: Create migration file**

```python
# alembic/versions/0006_memory_categories.py
"""add category and project to memories

Revision ID: 0006
Revises: 0005
Create Date: 2026-06-02
"""
from alembic import op
import sqlalchemy as sa

revision = '0006'
down_revision = '0005'
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.add_column('memories', sa.Column('category', sa.String(20), nullable=True))
    op.add_column('memories', sa.Column('project', sa.String(100), nullable=True))
    op.create_index('ix_memories_category', 'memories', ['category'])


def downgrade() -> None:
    op.drop_index('ix_memories_category', 'memories')
    op.drop_column('memories', 'project')
    op.drop_column('memories', 'category')
```

- [ ] **Step 2: Apply migration**

```bash
DATABASE_URL=postgresql+asyncpg://memex:memex@localhost:5432/memex uv run alembic upgrade head
```

Expected output: `Running upgrade 0005 -> 0006, add category and project to memories`

- [ ] **Step 3: Commit**

```bash
git add alembic/versions/0006_memory_categories.py
git commit -m "feat(memory): migration 0006 — add category and project columns"
```

---

## Task 2: Update Memory model + ExtractedFact

**Files:**
- Modify: `src/db/models.py` — add two columns to Memory
- Modify: `src/memory/extractor.py` — update dataclass + prompt + parse logic

- [ ] **Step 1: Write failing test**

```python
# tests/unit/test_memory_categories.py
import pytest
from src.db.models import Memory
from src.memory.extractor import ExtractedFact


def test_memory_model_has_category_and_project():
    m = Memory()
    assert hasattr(m, 'category')
    assert hasattr(m, 'project')


def test_extracted_fact_has_category_and_project():
    f = ExtractedFact(content="User works at Acme")
    assert f.category is None
    assert f.project is None


def test_extracted_fact_accepts_category():
    f = ExtractedFact(content="User works at Acme", category="decision", project="work")
    assert f.category == "decision"
    assert f.project == "work"
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_memory_categories.py -v
```

Expected: FAILED — `Memory has no attribute 'category'`

- [ ] **Step 3: Add columns to Memory model in `src/db/models.py`**

Add after the `parent_id` field:

```python
    category: Mapped[str | None] = mapped_column(String(20), nullable=True)
    project: Mapped[str | None] = mapped_column(String(100), nullable=True)
```

- [ ] **Step 4: Update ExtractedFact in `src/memory/extractor.py`**

Replace the dataclass:

```python
@dataclass
class ExtractedFact:
    content: str
    forget_after: datetime | None = None
    category: str | None = None   # research|reminder|thought|decision|preference
    project: str | None = None
```

- [ ] **Step 5: Update EXTRACT_PROMPT in `src/memory/extractor.py`**

Replace the existing `EXTRACT_PROMPT` constant:

```python
EXTRACT_PROMPT = """\
Extract atomic facts about the user from the following text.
Rules:
- Each fact is one statement, no pronouns — use "User" as subject.
- Include: identity, skills, location, work, relationships, projects, preferences, events the user participated in.
- Exclude: opinions and emotional reactions ("talks were great", "enjoyed it"), third-party info, weather. Always extract the underlying event/fact even if accompanied by an opinion.
- Normalize state: prefer "User uses X" over "User switched from Y to X".
- Time-bound facts (meetings, trips, deadlines) ARE included — add "forget_after" as an ISO datetime for them.
- For permanent facts, omit "forget_after".
- Set "category" to one of: research, reminder, thought, decision, preference. Omit if none fits.
- Set "project" to the project/context name if the fact belongs to one (e.g. "Memex", "work", "personal"). Omit if unclear.

Text: {text}

Return JSON only:
{{"facts": [{{"content": "...", "forget_after": "...or omit", "category": "...or omit", "project": "...or omit"}}]}}"""
```

- [ ] **Step 6: Update `extract_facts` parse logic in `src/memory/extractor.py`**

Replace the inner loop inside `extract_facts`:

```python
            for f in data.get("facts", []):
                forget_after = None
                if fa := f.get("forget_after"):
                    try:
                        forget_after = datetime.fromisoformat(fa)
                    except ValueError:
                        pass
                results.append(ExtractedFact(
                    content=f["content"],
                    forget_after=forget_after,
                    category=f.get("category") or None,
                    project=f.get("project") or None,
                ))
```

- [ ] **Step 7: Run tests to verify pass**

```bash
uv run pytest tests/unit/test_memory_categories.py -v
```

Expected: 3 tests PASSED

- [ ] **Step 8: Run full suite**

```bash
uv run pytest tests/unit/ -q --tb=short
```

Expected: all pass

- [ ] **Step 9: Commit**

```bash
git add src/db/models.py src/memory/extractor.py tests/unit/test_memory_categories.py
git commit -m "feat(memory): add category+project to Memory model and ExtractedFact"
```

---

## Task 3: MemoryRepository — pass and filter category/project

**Files:**
- Modify: `src/db/repositories/memory_repo.py`

- [ ] **Step 1: Add failing tests** (append to `tests/unit/test_memory_categories.py`)

```python
import uuid
from unittest.mock import AsyncMock, MagicMock
from src.db.repositories.memory_repo import MemoryRepository
from src.db.models import Memory


def make_memory(category=None, project=None):
    m = Memory()
    m.id = uuid.uuid4()
    m.content = "User works at Acme"
    m.raw_input = "I work at Acme"
    m.source = "explicit"
    m.is_active = True
    m.content_vector = [0.1] * 384
    m.forget_after = None
    m.relation = None
    m.parent_id = None
    m.category = category
    m.project = project
    return m


@pytest.mark.asyncio
async def test_repo_create_stores_category_and_project():
    session = AsyncMock()
    session.flush = AsyncMock()
    repo = MemoryRepository(session)
    m = await repo.create(
        content="Decided to use PostgreSQL",
        raw_input="Let's use PostgreSQL",
        source="explicit",
        vector=[0.1] * 384,
        category="decision",
        project="Memex",
    )
    session.add.assert_called_once()
    assert m.category == "decision"
    assert m.project == "Memex"


@pytest.mark.asyncio
async def test_repo_get_all_active_filters_by_category():
    session = AsyncMock()
    mem = make_memory(category="research")
    result_mock = MagicMock()
    result_mock.scalars.return_value.all.return_value = [mem]
    session.execute = AsyncMock(return_value=result_mock)

    repo = MemoryRepository(session)
    results = await repo.get_all_active(category="research")
    assert len(results) == 1
    assert results[0].category == "research"
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_memory_categories.py::test_repo_create_stores_category_and_project tests/unit/test_memory_categories.py::test_repo_get_all_active_filters_by_category -v
```

Expected: FAILED — `create() got unexpected keyword argument 'category'`

- [ ] **Step 3: Update `create()` in `src/db/repositories/memory_repo.py`**

Replace the `create` method signature and body:

```python
    async def create(
        self,
        content: str,
        raw_input: str,
        source: str,
        vector: list[float],
        parent_id: uuid.UUID | None = None,
        relation: str | None = None,
        forget_after: datetime | None = None,
        category: str | None = None,
        project: str | None = None,
    ) -> Memory:
        m = Memory(
            id=uuid.uuid4(),
            content=content,
            raw_input=raw_input,
            source=source,
            is_active=True,
            forget_after=forget_after,
            relation=relation,
            parent_id=parent_id,
            content_vector=vector,
            category=category,
            project=project,
        )
        self.session.add(m)
        await self.session.flush()
        return m
```

- [ ] **Step 4: Update `get_all_active()` to accept optional category filter**

Replace the `get_all_active` method:

```python
    async def get_all_active(self, category: str | None = None) -> list[Memory]:
        q = select(Memory).where(Memory.is_active == True)
        if category:
            q = q.where(Memory.category == category)
        result = await self.session.execute(q.order_by(Memory.created_at.desc()))
        return list(result.scalars().all())
```

- [ ] **Step 5: Run tests to verify pass**

```bash
uv run pytest tests/unit/test_memory_categories.py -v
```

Expected: 5 tests PASSED

- [ ] **Step 6: Run full suite**

```bash
uv run pytest tests/unit/ -q --tb=short
```

- [ ] **Step 7: Commit**

```bash
git add src/db/repositories/memory_repo.py tests/unit/test_memory_categories.py
git commit -m "feat(memory): repository supports category+project in create and filter"
```

---

## Task 4: MemoryService — pass category/project through

**Files:**
- Modify: `src/memory/service.py`

- [ ] **Step 1: Add failing test** (append to `tests/unit/test_memory_categories.py`)

```python
from src.memory.service import MemoryService, RememberResult
from src.memory.extractor import FactExtractor, RelationResult


@pytest.mark.asyncio
async def test_service_remember_passes_category_to_repo():
    repo = MagicMock()
    repo.get_active_by_vector = AsyncMock(return_value=[])
    repo.create = AsyncMock(return_value=make_memory(category="decision"))

    extractor = MagicMock(spec=FactExtractor)
    extractor.extract_facts = AsyncMock(
        return_value=[ExtractedFact(content="User decided to use PG", category="decision", project="Memex")]
    )
    extractor.resolve_relations = AsyncMock(return_value=[])
    embed_fn = AsyncMock(return_value=[0.1] * 384)

    service = MemoryService(repo=repo, extractor=extractor, embed_fn=embed_fn)
    await service.remember(AsyncMock(), "decided to use PG")

    call_kwargs = repo.create.call_args.kwargs
    assert call_kwargs["category"] == "decision"
    assert call_kwargs["project"] == "Memex"
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_memory_categories.py::test_service_remember_passes_category_to_repo -v
```

Expected: FAILED — `create() got unexpected keyword argument 'category'` (service doesn't pass them yet)

- [ ] **Step 3: Update `remember()` in `src/memory/service.py`**

Find the `repo.create(...)` call and add `category` and `project`:

```python
            await self.repo.create(
                content=fact.content,
                raw_input=text,
                source=source,
                vector=vector,
                parent_id=parent_id,
                relation=relation_type,
                forget_after=fact.forget_after,
                category=fact.category,
                project=fact.project,
            )
```

- [ ] **Step 4: Run tests to verify pass**

```bash
uv run pytest tests/unit/test_memory_categories.py -v
```

Expected: all pass

- [ ] **Step 5: Run full suite**

```bash
uv run pytest tests/unit/ -q --tb=short
```

- [ ] **Step 6: Commit**

```bash
git add src/memory/service.py tests/unit/test_memory_categories.py
git commit -m "feat(memory): service passes category+project to repository"
```

---

## Task 5: MemoryHit + MemorySearch — expose fields + category filter

**Files:**
- Modify: `src/retrieval/memory_search.py`

- [ ] **Step 1: Add failing test** (append to `tests/unit/test_memory_categories.py`)

```python
from src.retrieval.memory_search import MemorySearch, MemoryHit
from datetime import datetime, timezone


@pytest.mark.asyncio
async def test_memory_hit_exposes_category_and_project():
    mem = make_memory(category="research", project="Memex")
    mem.created_at = datetime.now(timezone.utc)
    repo = MagicMock()
    repo.get_active_by_vector = AsyncMock(return_value=[(mem, 0.9)])

    search = MemorySearch(repo=repo)
    hits = await search.search(AsyncMock(), query_vector=[0.1] * 384)

    assert hits[0].category == "research"
    assert hits[0].project == "Memex"


@pytest.mark.asyncio
async def test_memory_search_passes_category_to_repo():
    repo = MagicMock()
    repo.get_active_by_vector = AsyncMock(return_value=[])

    search = MemorySearch(repo=repo)
    await search.search(AsyncMock(), query_vector=[0.1] * 384, category="reminder")

    call_kwargs = repo.get_active_by_vector.call_args.kwargs
    assert call_kwargs.get("category") == "reminder"
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_memory_categories.py::test_memory_hit_exposes_category_and_project tests/unit/test_memory_categories.py::test_memory_search_passes_category_to_repo -v
```

Expected: FAILED

- [ ] **Step 3: Update `src/retrieval/memory_search.py`**

```python
import uuid
from dataclasses import dataclass
from datetime import datetime
from sqlalchemy.ext.asyncio import AsyncSession
from src.db.repositories.memory_repo import MemoryRepository


@dataclass
class MemoryHit:
    memory_id: uuid.UUID
    content: str
    score: float
    source: str
    created_at: datetime
    category: str | None = None
    project: str | None = None


class MemorySearch:
    RETRIEVAL_THRESHOLD = 0.30

    def __init__(self, repo: MemoryRepository, top_k: int = 10):
        self.repo = repo
        self.top_k = top_k

    async def search(
        self,
        session: AsyncSession,
        query_vector: list[float],
        category: str | None = None,
    ) -> list[MemoryHit]:
        results = await self.repo.get_active_by_vector(
            query_vector, limit=self.top_k, threshold=self.RETRIEVAL_THRESHOLD, category=category
        )
        return [
            MemoryHit(
                memory_id=mem.id,
                content=mem.content,
                score=score,
                source=mem.source,
                created_at=mem.created_at,
                category=mem.category,
                project=mem.project,
            )
            for mem, score in results
        ]
```

- [ ] **Step 4: Update `get_active_by_vector` in `src/db/repositories/memory_repo.py` to accept category**

Add `category: str | None = None` parameter and SQL filter:

```python
    async def get_active_by_vector(
        self,
        vector: list[float],
        limit: int = 5,
        threshold: float = 0.75,
        category: str | None = None,
    ) -> list[tuple[Memory, float]]:
        vec_str = "[" + ",".join(str(x) for x in vector) + "]"
        category_filter = "AND category = :category" if category else ""
        rows = await self.session.execute(
            text(f"""
                SELECT id, 1 - (content_vector <=> '{vec_str}'::vector) AS score
                FROM memories
                WHERE is_active = TRUE
                  AND content_vector IS NOT NULL
                  AND 1 - (content_vector <=> '{vec_str}'::vector) >= :threshold
                  {category_filter}
                ORDER BY content_vector <=> '{vec_str}'::vector
                LIMIT :limit
            """),
            {"threshold": threshold, "limit": limit, "category": category} if category
            else {"threshold": threshold, "limit": limit},
        )
```

- [ ] **Step 5: Run tests to verify pass**

```bash
uv run pytest tests/unit/test_memory_categories.py -v
```

Expected: all pass

- [ ] **Step 6: Run full suite**

```bash
uv run pytest tests/unit/ -q --tb=short
```

- [ ] **Step 7: Commit**

```bash
git add src/retrieval/memory_search.py src/db/repositories/memory_repo.py tests/unit/test_memory_categories.py
git commit -m "feat(memory): MemoryHit exposes category+project, search accepts category filter"
```

---

## Task 6: ContextBuilder — rich memory display

**Files:**
- Modify: `src/retrieval/context.py`
- Modify: `tests/unit/test_context_builder.py`

- [ ] **Step 1: Add failing test** (append to `tests/unit/test_context_builder.py`)

```python
# append to existing test_context_builder.py

from src.retrieval.memory_search import MemoryHit
from datetime import datetime, timezone
import uuid


def make_hit(content, category=None, project=None):
    return MemoryHit(
        memory_id=uuid.uuid4(),
        content=content,
        score=0.9,
        source="explicit",
        created_at=datetime(2026, 5, 20, tzinfo=timezone.utc),
        category=category,
        project=project,
    )


def test_context_builder_shows_category_in_memory_tag():
    from src.retrieval.context import ContextBuilder
    builder = ContextBuilder()
    hit = make_hit("User decided to use PG", category="decision", project="Memex")
    ctx = builder.build("what db?", chunks=[], memory_hits=[hit], today="2026-06-02")
    assert "decision" in ctx.prompt
    assert "Memex" in ctx.prompt
    assert "2026-05-20" in ctx.prompt


def test_context_builder_bare_memory_tag_when_no_category():
    from src.retrieval.context import ContextBuilder
    builder = ContextBuilder()
    hit = make_hit("User lives in Moscow")
    ctx = builder.build("where?", chunks=[], memory_hits=[hit], today="2026-06-02")
    assert "[memory]" in ctx.prompt
    assert "decision" not in ctx.prompt
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_context_builder.py::test_context_builder_shows_category_in_memory_tag tests/unit/test_context_builder.py::test_context_builder_bare_memory_tag_when_no_category -v
```

Expected: FAILED — `MemoryHit has no attribute 'category'` (MemoryHit didn't have it before Task 5, but now it does — the test should fail on assertion instead)

- [ ] **Step 3: Update memory rendering in `src/retrieval/context.py`**

In the `build()` method, replace the memory section:

```python
        if memory_hits:
            sources_text += "\nPersonal memory facts:\n"
            for hit in memory_hits[:5]:
                parts = ["memory"]
                if hit.category:
                    parts.append(hit.category)
                if hit.project:
                    parts.append(hit.project)
                if hit.created_at:
                    parts.append(hit.created_at.strftime("%Y-%m-%d"))
                tag = " | ".join(parts)
                sources_text += f"  [{tag}] {hit.content}\n"
```

- [ ] **Step 4: Run tests to verify pass**

```bash
uv run pytest tests/unit/test_context_builder.py -v
```

Expected: all pass

- [ ] **Step 5: Run full suite**

```bash
uv run pytest tests/unit/ -q --tb=short
```

- [ ] **Step 6: Commit**

```bash
git add src/retrieval/context.py tests/unit/test_context_builder.py
git commit -m "feat(memory): show category+project+date in retrieval context"
```

---

## Task 7: API + MCP — category filter

**Files:**
- Modify: `src/api/memories.py`
- Modify: `src/memory/service.py` (add category param to `list_active`)
- Modify: `src/mcp/server.py`

- [ ] **Step 1: Add `category` param to `list_active()` in `src/memory/service.py`**

```python
    async def list_active(self, session: AsyncSession, category: str | None = None) -> list[Memory]:
        return await self.repo.get_all_active(category=category)
```

- [ ] **Step 2: Update `GET /api/memory/list` in `src/api/memories.py`**

```python
from typing import Literal
from fastapi import Query

@router.get("/list")
async def list_memories(
    session: AsyncSession = Depends(get_db_session),
    category: str | None = Query(default=None, description="Filter by category: research|reminder|thought|decision|preference"),
):
    service = get_memory_service(session)
    memories = await service.list_active(session, category=category)
    return [
        {
            "id": str(m.id),
            "content": m.content,
            "source": m.source,
            "category": m.category,
            "project": m.project,
            "relation": m.relation,
            "created_at": m.created_at.isoformat() if m.created_at else None,
        }
        for m in memories
    ]
```

Note: also add `"category"` and `"project"` to the response dict — they were missing before.

- [ ] **Step 3: Update `recall` tool in `src/mcp/server.py`**

Add `category` to the tool definition properties:

```python
                    "category": {
                        "type": "string",
                        "enum": ["research", "reminder", "thought", "decision", "preference"],
                        "description": "Filter memories by category (optional)",
                    },
```

- [ ] **Step 4: Update `_recall` handler in `src/mcp/server.py`** to pass category to search

In `_recall`, when calling `POST /api/query`, pass `memory_category` if provided:

```python
async def _recall(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    query = args["query"]
    raw = args.get("raw", False)
    category = args.get("category")

    if raw:
        top_k = args.get("top_k", 5)
        resp = await client.post(f"{BASE_URL}/api/search/chunks", json={"query": query, "top_k": top_k})
        # ... existing raw handling unchanged ...
    
    payload: dict = {"query": query}
    if category:
        payload["memory_category"] = category
    resp = await client.post(f"{BASE_URL}/api/query", json=payload)
    # ... rest unchanged ...
```

- [ ] **Step 5: Update `POST /api/query` in `src/api/query.py`** to accept and pass `memory_category`

```python
class QueryRequest(BaseModel):
    query: str
    top_k: int = 5
    memory_category: str | None = None


@router.post("/query", response_model=QueryResponse)
async def query_documents(
    request: QueryRequest,
    session: AsyncSession = Depends(get_db_session),
):
    service = get_retrieval_service()
    embedding_client = get_embedding_client()

    async def embed(text: str) -> list[float]:
        results = await embedding_client.embed_batch([text], is_query=True)
        return results[0]

    memory_search = MemorySearch(repo=MemoryRepository(session))

    result = await service.query(
        session, request.query, embed_fn=embed,
        memory_search=memory_search,
        memory_category=request.memory_category,
    )
    return QueryResponse(answer=result.answer, sources=result.sources)
```

- [ ] **Step 6: Update `RetrievalService.query()` in `src/retrieval/service.py`** to accept and pass `memory_category`

Add `memory_category: str | None = None` parameter and pass to `memory_search.search()`:

```python
    async def query(
        self,
        session: AsyncSession,
        query: str,
        embed_fn,
        memory_search: "MemorySearch | None" = None,
        memory_category: str | None = None,
    ) -> QueryResult:
        # ... existing code ...
        if effective_memory_search:
            with t.step("memory"):
                mem_hits = await effective_memory_search.search(
                    session, query_vector, category=memory_category
                )
```

Do the same for `query_stream()`.

- [ ] **Step 7: Run full suite**

```bash
uv run pytest tests/unit/ -q --tb=short
```

Expected: all pass

- [ ] **Step 8: Commit**

```bash
git add src/api/memories.py src/api/query.py src/memory/service.py src/mcp/server.py src/retrieval/service.py
git commit -m "feat(memory): category filter on API /list, /query, and MCP recall"
```

---

## Task 8: Update Hermes bridge

**Files:**
- Modify: `hermes/memex-bridge.py`

- [ ] **Step 1: Add `category` to recall tool definition** (same change as MCP server)

In `list_tools()`, find the `recall` tool and add to its `properties`:

```python
                    "category": {
                        "type": "string",
                        "enum": ["research", "reminder", "thought", "decision", "preference"],
                        "description": "Filter memories by category (optional)",
                    },
```

- [ ] **Step 2: Update `_recall` in bridge** to pass category

Same as Task 7 Step 4.

- [ ] **Step 3: Update `_memories` in bridge** to include category/project in output

```python
    for m in mems:
        rel = f" [{m['relation']}]" if m.get("relation") else ""
        cat = f" | {m['category']}" if m.get("category") else ""
        proj = f" | {m['project']}" if m.get("project") else ""
        date = (m.get("created_at") or "")[:10]
        lines.append(f"• {m['content']}{rel}\n  id: {m['id']}  |  {m['source']}{cat}{proj}  |  {date}")
```

- [ ] **Step 4: Run syntax check**

```bash
uv run python -c "import ast; ast.parse(open('hermes/memex-bridge.py').read()); print('OK')"
```

- [ ] **Step 5: Run full suite**

```bash
uv run pytest tests/unit/ -q --tb=short
```

- [ ] **Step 6: Commit**

```bash
git add hermes/memex-bridge.py
git commit -m "feat(hermes): bridge exposes category filter on recall and shows category in memories"
```

---

## Final Verification

- [ ] **Run complete test suite**

```bash
uv run pytest tests/unit/ -v
```

Expected: 106+ tests pass, 0 failures

- [ ] **Manual smoke test (requires running server)**

```bash
# Start server
env $(grep -v '^#' .env | xargs) MEMEX_PROFILE=1 uv run uvicorn src.main:app --host 0.0.0.0 --port 8000

# Store categorized facts
curl -s -X POST http://localhost:8000/api/memory/remember \
  -H "Content-Type: application/json" \
  -d '{"content": "Decided to use PostgreSQL over MongoDB for Memex"}'

# List only decisions
curl -s "http://localhost:8000/api/memory/list?category=decision" | python3 -c "import sys,json; [print(m['category'], m['content'][:60]) for m in json.load(sys.stdin)]"

# Query with category filter
curl -s -X POST http://localhost:8000/api/query \
  -H "Content-Type: application/json" \
  -d '{"query": "what database decisions were made?", "memory_category": "decision"}'
```

- [ ] **Run A/B benchmark to confirm no regression**

```bash
OPENAI_API_KEY=sk-... uv run python tests/research/rq_prompt_ab_test.py
```

Expected: ✓ V2 PASSED
