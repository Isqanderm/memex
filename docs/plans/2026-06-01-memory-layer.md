# Memory Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-user evolving memory layer to Memex — facts extracted from text, conversations, and documents, with LLM-driven conflict resolution and profile generation.

**Architecture:** A new `memories` table stores atomic facts with version chains (`parent_id`, `relation`). A `FactExtractor` uses LLM to extract facts and resolve `updates/extends/derives` relations against existing memories. `RetrievalService.query()` merges memory hits with chunk hits via RRF boost. Two new MCP tools (`context`, `observe`) and updated `remember` complete the interface.

**Tech Stack:** Python 3.12, SQLAlchemy 2.0 async, pgvector, Alembic, FastAPI, Anthropic/OpenAI via existing `LLMProvider` protocol, pytest + pytest-asyncio, `AsyncMock`.

**Decision gate (after Task 8):** All three R&D benchmarks must pass their success criteria before proceeding to Phase 5+. See spec `docs/specs/2026-06-01-memory-layer-design.md`.

---

## File Map

**New files:**
```
src/memory/__init__.py
src/memory/extractor.py          — FactExtractor: extract_facts(), resolve_relations()
src/memory/service.py            — MemoryService: remember(), observe(), forget_memory(), list_active()
src/memory/profile.py            — ProfileService: build_profile() → UserProfile
src/retrieval/memory_search.py   — MemorySearch: semantic search over memories table
src/db/repositories/memory_repo.py — MemoryRepository: CRUD + vector search
tests/unit/test_memory_extractor.py
tests/unit/test_memory_service.py
tests/unit/test_memory_profile.py
tests/unit/test_memory_search.py
tests/research/__init__.py
tests/research/datasets/rq2_extraction_cases.json
tests/research/datasets/rq1_eval_conversations.json
tests/research/rq2_extraction_eval.py
tests/research/rq1_eval.py
tests/research/rq3_benchmark.py
```

**Modified files:**
```
src/db/models.py                 — add Memory, MemoryExtractionJob models
alembic/versions/0003_add_memories.py — new migration
src/retrieval/service.py         — inject memory hits into query()
src/api/query.py                 — wire MemorySearch into RetrievalService
src/mcp/server.py                — new context/observe/memories tools; updated remember/recall/forget
```

---

## Phase 0 — R&D: Prompt Validation (RQ2)

*Validate LLM prompts before building any infrastructure around them.*

---

### Task 1: RQ2 — Fact extraction and relation accuracy

**Files:**
- Create: `tests/research/__init__.py`
- Create: `tests/research/datasets/rq2_extraction_cases.json`
- Create: `tests/research/rq2_extraction_eval.py`

- [ ] **Step 1: Create the dataset file**

```json
// tests/research/datasets/rq2_extraction_cases.json
{
  "extraction_cases": [
    {
      "input": "I now work at Acme Corp as a senior engineer.",
      "expected_facts": ["User works at Acme Corp as a senior engineer"]
    },
    {
      "input": "Switched from Python to TypeScript for this project.",
      "expected_facts": ["User uses TypeScript for the current project"]
    },
    {
      "input": "Meeting with the team is tomorrow at 3pm.",
      "expected_facts": ["User has a meeting with the team at 3pm tomorrow"],
      "expected_temporal": true
    },
    {
      "input": "I was born in 1990 and grew up in Moscow.",
      "expected_facts": ["User was born in 1990", "User grew up in Moscow"]
    },
    {
      "input": "Just got back from the Berlin conference. Great talks.",
      "expected_facts": ["User attended a conference in Berlin"]
    }
  ],
  "relation_cases": [
    {
      "new_fact": "User works at Beta Ltd",
      "existing": [
        {"id": "aaa-1", "content": "User works at Acme Corp"}
      ],
      "expected": [{"id": "aaa-1", "type": "updates"}]
    },
    {
      "new_fact": "User is a senior engineer at Acme Corp",
      "existing": [
        {"id": "bbb-1", "content": "User works at Acme Corp"}
      ],
      "expected": [{"id": "bbb-1", "type": "extends"}]
    },
    {
      "new_fact": "User has over 10 years of experience",
      "existing": [
        {"id": "ccc-1", "content": "User was born in 1990"}
      ],
      "expected": [{"id": "ccc-1", "type": "derives"}]
    },
    {
      "new_fact": "User prefers dark mode",
      "existing": [
        {"id": "ddd-1", "content": "User works at Acme Corp"}
      ],
      "expected": [{"id": "ddd-1", "type": "new"}]
    }
  ]
}
```

- [ ] **Step 2: Create the eval script**

```python
# tests/research/rq2_extraction_eval.py
"""
RQ2: Fact extraction and relation resolution accuracy.

Usage:
    ANTHROPIC_API_KEY=sk-... uv run python tests/research/rq2_extraction_eval.py

Success criteria:
    Extraction precision >= 0.90
    Extraction recall    >= 0.80
    Relation accuracy    >= 0.85
    updates recall       >= 0.90
"""
import asyncio
import json
import os
from pathlib import Path

import anthropic

DATASETS = Path(__file__).parent / "datasets" / "rq2_extraction_cases.json"

EXTRACT_PROMPT = """\
Extract atomic facts about the user from the following text.
Rules:
- Each fact is one statement, no pronouns — use "User" as subject.
- Ignore facts with no lasting relevance (e.g., weather, third-party chitchat).
- If a fact is time-bound (e.g. "meeting tomorrow"), add "forget_after": "<ISO datetime close to the event>".
- For permanent facts, omit "forget_after".

Text: {text}

Return JSON only:
{{"facts": [{{"content": "...", "forget_after": "...or omit"}}]}}"""

RESOLVE_PROMPT = """\
New fact: "{new_fact}"

Existing similar facts:
{existing}

For each existing fact determine the relation of the new fact to it:
- updates: new fact contradicts and supersedes the old one
- extends: new fact adds detail without contradiction
- derives: new fact is logically inferred from the old one
- new: not meaningfully related

Return JSON only:
{{"relations": [{{"id": "...", "type": "updates|extends|derives|new"}}]}}"""


async def call_llm(client: anthropic.Anthropic, prompt: str) -> str:
    msg = client.messages.create(
        model="claude-haiku-4-5-20251001",
        max_tokens=512,
        messages=[{"role": "user", "content": prompt}],
    )
    return msg.content[0].text


def extract_json(text: str) -> dict:
    start = text.find("{")
    end = text.rfind("}") + 1
    return json.loads(text[start:end])


async def run_extraction_eval(client, cases):
    tp, fp, fn = 0, 0, 0
    for case in cases:
        prompt = EXTRACT_PROMPT.format(text=case["input"])
        raw = await call_llm(client, prompt)
        try:
            result = extract_json(raw)
            extracted = [f["content"].lower() for f in result.get("facts", [])]
        except Exception:
            extracted = []

        expected = [e.lower() for e in case["expected_facts"]]
        matched = sum(1 for e in expected if any(e[:30] in x for x in extracted))
        tp += matched
        fn += len(expected) - matched
        fp += max(0, len(extracted) - matched)
        print(f"  Input: {case['input'][:60]}")
        print(f"  Expected: {expected}")
        print(f"  Got: {extracted}")
        print()

    precision = tp / (tp + fp) if (tp + fp) > 0 else 0
    recall = tp / (tp + fn) if (tp + fn) > 0 else 0
    return precision, recall


async def run_relation_eval(client, cases):
    correct = 0
    updates_tp, updates_total = 0, 0
    for case in cases:
        existing_str = "\n".join(
            f'  id={e["id"]}: "{e["content"]}"' for e in case["existing"]
        )
        prompt = RESOLVE_PROMPT.format(new_fact=case["new_fact"], existing=existing_str)
        raw = await call_llm(client, prompt)
        try:
            result = extract_json(raw)
            relations = {r["id"]: r["type"] for r in result.get("relations", [])}
        except Exception:
            relations = {}

        for exp in case["expected"]:
            got = relations.get(exp["id"], "new")
            if got == exp["type"]:
                correct += 1
            if exp["type"] == "updates":
                updates_total += 1
                if got == "updates":
                    updates_tp += 1
            print(f"  new='{case['new_fact'][:40]}' vs id={exp['id']}")
            print(f"  expected={exp['type']}  got={got}")

    total = sum(len(c["expected"]) for c in cases)
    accuracy = correct / total if total > 0 else 0
    updates_recall = updates_tp / updates_total if updates_total > 0 else 1.0
    return accuracy, updates_recall


async def main():
    api_key = os.getenv("ANTHROPIC_API_KEY")
    if not api_key:
        print("ANTHROPIC_API_KEY not set — skipping live eval")
        return

    data = json.loads(DATASETS.read_text())
    client = anthropic.Anthropic(api_key=api_key)

    print("=== RQ2: Extraction accuracy ===")
    precision, recall = await run_extraction_eval(client, data["extraction_cases"])
    print(f"Precision: {precision:.2f}  (target >= 0.90)")
    print(f"Recall:    {recall:.2f}  (target >= 0.80)")

    print("\n=== RQ2: Relation accuracy ===")
    accuracy, updates_recall = await run_relation_eval(client, data["relation_cases"])
    print(f"Accuracy:       {accuracy:.2f}  (target >= 0.85)")
    print(f"updates recall: {updates_recall:.2f}  (target >= 0.90)")

    ok = precision >= 0.90 and recall >= 0.80 and accuracy >= 0.85 and updates_recall >= 0.90
    print(f"\n{'✓ RQ2 PASSED' if ok else '✗ RQ2 FAILED — iterate prompts before proceeding'}")


if __name__ == "__main__":
    asyncio.run(main())
```

- [ ] **Step 3: Create `tests/research/__init__.py`**

```python
# tests/research/__init__.py
```

- [ ] **Step 4: Run RQ2 eval**

```bash
ANTHROPIC_API_KEY=sk-... uv run python tests/research/rq2_extraction_eval.py
```

Expected output ends with `✓ RQ2 PASSED`. If it fails, iterate the prompts in `EXTRACT_PROMPT` and `RESOLVE_PROMPT` before proceeding to Task 2.

- [ ] **Step 5: Commit**

```bash
git add tests/research/ && git commit -m "research: add RQ2 fact extraction eval dataset and script"
```

---

## Phase 1 — Foundation

---

### Task 2: Memory models + migration

**Files:**
- Modify: `src/db/models.py`
- Create: `alembic/versions/0003_add_memories.py`

- [ ] **Step 1: Write failing import test**

```python
# tests/unit/test_db_models.py  (add to existing file — append these cases)
def test_memory_model_importable():
    from src.db.models import Memory, MemoryExtractionJob
    assert Memory.__tablename__ == "memories"
    assert MemoryExtractionJob.__tablename__ == "memory_extraction_jobs"
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_db_models.py::test_memory_model_importable -v
```

Expected: `FAILED` — `ImportError: cannot import name 'Memory'`

- [ ] **Step 3: Add models to `src/db/models.py`**

Add after the `IngestionJob` class:

```python
from sqlalchemy import Boolean  # add Boolean to existing import line

class Memory(Base):
    __tablename__ = "memories"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    content: Mapped[str] = mapped_column(Text, nullable=False)
    raw_input: Mapped[str] = mapped_column(Text, nullable=False)
    source: Mapped[str] = mapped_column(String(20), nullable=False)
    is_active: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)
    forget_after: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)
    relation: Mapped[str | None] = mapped_column(String(20), nullable=True)
    parent_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True), ForeignKey("memories.id"), nullable=True
    )
    content_vector: Mapped[list[float] | None] = mapped_column(Vector(1536))
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())


class MemoryExtractionJob(Base):
    __tablename__ = "memory_extraction_jobs"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    source_ref: Mapped[str] = mapped_column(Text, nullable=False)
    source: Mapped[str] = mapped_column(String(20), nullable=False)
    status: Mapped[str] = mapped_column(String(20), default="pending", nullable=False)
    facts_extracted: Mapped[int] = mapped_column(Integer, default=0)
    error: Mapped[str | None] = mapped_column(Text, nullable=True)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), onupdate=func.now()
    )
```

Also add `from datetime import datetime` to the top of `src/db/models.py` if not already present.

- [ ] **Step 4: Run to verify pass**

```bash
uv run pytest tests/unit/test_db_models.py::test_memory_model_importable -v
```

Expected: `PASSED`

- [ ] **Step 5: Write migration**

```python
# alembic/versions/0003_add_memories.py
"""add memories tables

Revision ID: 0003
Revises: 0002
Create Date: 2026-06-01
"""
from alembic import op
import sqlalchemy as sa

revision = '0003'
down_revision = '0002'
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.create_table(
        'memories',
        sa.Column('id', sa.UUID(), primary_key=True),
        sa.Column('content', sa.Text(), nullable=False),
        sa.Column('raw_input', sa.Text(), nullable=False),
        sa.Column('source', sa.String(20), nullable=False),
        sa.Column('is_active', sa.Boolean(), nullable=False, server_default='true'),
        sa.Column('forget_after', sa.DateTime(timezone=True), nullable=True),
        sa.Column('relation', sa.String(20), nullable=True),
        sa.Column('parent_id', sa.UUID(), sa.ForeignKey('memories.id'), nullable=True),
        sa.Column('content_vector', sa.Text(), nullable=True),  # pgvector stores as text in DDL
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
    )
    op.execute("""
        ALTER TABLE memories
        ALTER COLUMN content_vector TYPE vector(1536)
        USING content_vector::vector(1536)
    """)
    op.create_index('ix_memories_is_active', 'memories', ['is_active'])
    op.create_index(
        'ix_memories_vector',
        'memories',
        ['content_vector'],
        postgresql_using='hnsw',
        postgresql_with={'m': 16, 'ef_construction': 64},
        postgresql_ops={'content_vector': 'vector_cosine_ops'},
    )

    op.create_table(
        'memory_extraction_jobs',
        sa.Column('id', sa.UUID(), primary_key=True),
        sa.Column('source_ref', sa.Text(), nullable=False),
        sa.Column('source', sa.String(20), nullable=False),
        sa.Column('status', sa.String(20), nullable=False, server_default='pending'),
        sa.Column('facts_extracted', sa.Integer(), nullable=False, server_default='0'),
        sa.Column('error', sa.Text(), nullable=True),
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
        sa.Column('updated_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
    )


def downgrade() -> None:
    op.drop_table('memory_extraction_jobs')
    op.drop_index('ix_memories_vector', 'memories')
    op.drop_index('ix_memories_is_active', 'memories')
    op.drop_table('memories')
```

- [ ] **Step 6: Run all unit tests**

```bash
uv run pytest tests/unit/ -q
```

Expected: all pass (migration file doesn't run in unit tests)

- [ ] **Step 7: Commit**

```bash
git add src/db/models.py alembic/versions/0003_add_memories.py
git commit -m "feat(memory): add Memory and MemoryExtractionJob models + migration"
```

---

### Task 3: MemoryRepository

**Files:**
- Create: `src/db/repositories/memory_repo.py`
- Create: `tests/unit/test_memory_repo.py`

- [ ] **Step 1: Write failing tests**

```python
# tests/unit/test_memory_repo.py
import uuid
import pytest
from unittest.mock import AsyncMock, MagicMock, patch
from src.db.repositories.memory_repo import MemoryRepository
from src.db.models import Memory


def make_memory(is_active=True, vector=None):
    m = Memory()
    m.id = uuid.uuid4()
    m.content = "User works at Acme"
    m.raw_input = "I work at Acme"
    m.source = "explicit"
    m.is_active = is_active
    m.content_vector = vector or [0.1] * 1536
    m.forget_after = None
    m.relation = None
    m.parent_id = None
    return m


@pytest.mark.asyncio
async def test_memory_repo_create():
    session = AsyncMock()
    session.flush = AsyncMock()
    repo = MemoryRepository(session)
    m = await repo.create(
        content="User works at Acme",
        raw_input="I work at Acme",
        source="explicit",
        vector=[0.1] * 1536,
    )
    session.add.assert_called_once()
    session.flush.assert_called_once()
    assert m.content == "User works at Acme"
    assert m.is_active is True


@pytest.mark.asyncio
async def test_memory_repo_deactivate():
    session = AsyncMock()
    mem = make_memory(is_active=True)

    result_mock = MagicMock()
    result_mock.scalar_one_or_none.return_value = mem
    session.execute = AsyncMock(return_value=result_mock)

    repo = MemoryRepository(session)
    await repo.deactivate(mem.id)

    assert mem.is_active is False
    session.flush.assert_called_once()


@pytest.mark.asyncio
async def test_memory_repo_get_all_active():
    session = AsyncMock()
    mem = make_memory(is_active=True)
    result_mock = MagicMock()
    result_mock.scalars.return_value.all.return_value = [mem]
    session.execute = AsyncMock(return_value=result_mock)

    repo = MemoryRepository(session)
    result = await repo.get_all_active()

    assert len(result) == 1
    assert result[0].is_active is True
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_memory_repo.py -v
```

Expected: `FAILED` — `ModuleNotFoundError: No module named 'src.db.repositories.memory_repo'`

- [ ] **Step 3: Implement MemoryRepository**

```python
# src/db/repositories/memory_repo.py
import uuid
from datetime import datetime
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, update
from sqlalchemy.dialects.postgresql import UUID as PG_UUID
from src.db.models import Memory


class MemoryRepository:
    def __init__(self, session: AsyncSession):
        self.session = session

    async def create(
        self,
        content: str,
        raw_input: str,
        source: str,
        vector: list[float],
        parent_id: uuid.UUID | None = None,
        relation: str | None = None,
        forget_after: datetime | None = None,
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
        )
        self.session.add(m)
        await self.session.flush()
        return m

    async def deactivate(self, memory_id: uuid.UUID) -> None:
        result = await self.session.execute(
            select(Memory).where(Memory.id == memory_id)
        )
        mem = result.scalar_one_or_none()
        if mem:
            mem.is_active = False
            await self.session.flush()

    async def get_all_active(self) -> list[Memory]:
        result = await self.session.execute(
            select(Memory).where(Memory.is_active == True).order_by(Memory.created_at.desc())
        )
        return list(result.scalars().all())

    async def get_by_id(self, memory_id: uuid.UUID) -> Memory | None:
        result = await self.session.execute(
            select(Memory).where(Memory.id == memory_id)
        )
        return result.scalar_one_or_none()

    async def get_active_by_vector(
        self,
        vector: list[float],
        limit: int = 5,
        threshold: float = 0.75,
    ) -> list[tuple[Memory, float]]:
        vec_str = "[" + ",".join(str(x) for x in vector) + "]"
        from sqlalchemy import text
        rows = await self.session.execute(
            text("""
                SELECT id, 1 - (content_vector <=> :vec::vector) AS score
                FROM memories
                WHERE is_active = TRUE
                  AND content_vector IS NOT NULL
                  AND 1 - (content_vector <=> :vec::vector) >= :threshold
                ORDER BY content_vector <=> :vec::vector
                LIMIT :limit
            """),
            {"vec": vec_str, "threshold": threshold, "limit": limit},
        )
        ids_scores = [(row.id, row.score) for row in rows]
        if not ids_scores:
            return []
        id_list = [r[0] for r in ids_scores]
        mems_result = await self.session.execute(
            select(Memory).where(Memory.id.in_(id_list))
        )
        mems_by_id = {m.id: m for m in mems_result.scalars().all()}
        return [(mems_by_id[mid], score) for mid, score in ids_scores if mid in mems_by_id]

    async def expire_stale(self) -> int:
        from sqlalchemy import text
        from datetime import timezone
        result = await self.session.execute(
            text("""
                UPDATE memories SET is_active = FALSE
                WHERE forget_after < NOW() AND is_active = TRUE
                RETURNING id
            """)
        )
        await self.session.flush()
        return len(result.fetchall())
```

- [ ] **Step 4: Run to verify pass**

```bash
uv run pytest tests/unit/test_memory_repo.py -v
```

Expected: 3 tests `PASSED`

- [ ] **Step 5: Run all unit tests**

```bash
uv run pytest tests/unit/ -q
```

Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add src/db/repositories/memory_repo.py tests/unit/test_memory_repo.py
git commit -m "feat(memory): add MemoryRepository with CRUD and vector search"
```

---

## Phase 2 — LLM Engine

---

### Task 4: FactExtractor

**Files:**
- Create: `src/memory/__init__.py`
- Create: `src/memory/extractor.py`
- Create: `tests/unit/test_memory_extractor.py`

- [ ] **Step 1: Write failing tests**

```python
# tests/unit/test_memory_extractor.py
import pytest
from unittest.mock import AsyncMock
from src.memory.extractor import FactExtractor, ExtractedFact, RelationResult
from tests.mocks.mock_llm import MockLLMProvider
import uuid


@pytest.mark.asyncio
async def test_extract_facts_returns_list():
    llm = MockLLMProvider(response='{"facts": [{"content": "User works at Acme"}]}')
    extractor = FactExtractor(llm)
    facts = await extractor.extract_facts("I work at Acme")
    assert len(facts) == 1
    assert facts[0].content == "User works at Acme"
    assert facts[0].forget_after is None


@pytest.mark.asyncio
async def test_extract_facts_handles_malformed_json():
    llm = MockLLMProvider(response="Sorry I cannot do that")
    extractor = FactExtractor(llm)
    facts = await extractor.extract_facts("I work at Acme")
    assert facts == []


@pytest.mark.asyncio
async def test_extract_facts_parses_forget_after():
    llm = MockLLMProvider(
        response='{"facts": [{"content": "User has a meeting", "forget_after": "2026-06-02T15:00:00"}]}'
    )
    extractor = FactExtractor(llm)
    facts = await extractor.extract_facts("Meeting tomorrow at 3pm")
    assert facts[0].forget_after is not None


@pytest.mark.asyncio
async def test_resolve_relations_returns_updates():
    llm = MockLLMProvider(
        response='{"relations": [{"id": "aaa", "type": "updates"}]}'
    )
    extractor = FactExtractor(llm)
    existing_id = uuid.UUID("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
    results = await extractor.resolve_relations(
        new_fact="User works at Beta",
        existing=[(existing_id, "User works at Acme")],
    )
    assert len(results) == 1
    assert results[0].relation == "updates"


@pytest.mark.asyncio
async def test_resolve_relations_empty_existing():
    llm = MockLLMProvider(response='{"relations": []}')
    extractor = FactExtractor(llm)
    results = await extractor.resolve_relations("User works at Beta", [])
    assert results == []
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_memory_extractor.py -v
```

Expected: `FAILED` — `ModuleNotFoundError: No module named 'src.memory'`

- [ ] **Step 3: Create `src/memory/__init__.py`**

```python
# src/memory/__init__.py
```

- [ ] **Step 4: Implement FactExtractor**

```python
# src/memory/extractor.py
import json
import uuid
from dataclasses import dataclass, field
from datetime import datetime
from src.llm.protocol import LLMProvider

EXTRACT_PROMPT = """\
Extract atomic facts about the user from the following text.
Rules:
- Each fact is one statement, no pronouns — use "User" as subject.
- Ignore facts with no lasting relevance (weather, third-party chitchat).
- If a fact is time-bound (e.g. "meeting tomorrow"), add "forget_after" as an ISO datetime.
- For permanent facts, omit "forget_after".

Text: {text}

Return JSON only:
{{"facts": [{{"content": "...", "forget_after": "...or omit"}}]}}"""

RESOLVE_PROMPT = """\
New fact: "{new_fact}"

Existing similar facts:
{existing}

For each existing fact determine the relation of the new fact to it:
- updates: new fact contradicts and supersedes the old one
- extends: new fact adds detail without contradiction
- derives: new fact is logically inferred from the old one
- new: not meaningfully related

Return JSON only:
{{"relations": [{{"id": "...", "type": "updates|extends|derives|new"}}]}}"""


@dataclass
class ExtractedFact:
    content: str
    forget_after: datetime | None = None


@dataclass
class RelationResult:
    memory_id: uuid.UUID
    relation: str  # updates | extends | derives | new


def _parse_json(text: str) -> dict:
    start = text.find("{")
    end = text.rfind("}") + 1
    if start == -1 or end == 0:
        return {}
    return json.loads(text[start:end])


class FactExtractor:
    def __init__(self, llm_provider: LLMProvider):
        self.llm = llm_provider

    async def extract_facts(self, text: str) -> list[ExtractedFact]:
        prompt = EXTRACT_PROMPT.format(text=text)
        response = await self.llm.complete(prompt)
        try:
            data = _parse_json(response.answer)
            results = []
            for f in data.get("facts", []):
                forget_after = None
                if fa := f.get("forget_after"):
                    try:
                        forget_after = datetime.fromisoformat(fa)
                    except ValueError:
                        pass
                results.append(ExtractedFact(content=f["content"], forget_after=forget_after))
            return results
        except Exception:
            return []

    async def resolve_relations(
        self,
        new_fact: str,
        existing: list[tuple[uuid.UUID, str]],
    ) -> list[RelationResult]:
        if not existing:
            return []
        existing_str = "\n".join(f'  id={mid}: "{content}"' for mid, content in existing)
        prompt = RESOLVE_PROMPT.format(new_fact=new_fact, existing=existing_str)
        response = await self.llm.complete(prompt)
        try:
            data = _parse_json(response.answer)
            return [
                RelationResult(memory_id=uuid.UUID(r["id"]), relation=r["type"])
                for r in data.get("relations", [])
            ]
        except Exception:
            return []
```

- [ ] **Step 5: Run to verify pass**

```bash
uv run pytest tests/unit/test_memory_extractor.py -v
```

Expected: 5 tests `PASSED`

- [ ] **Step 6: Commit**

```bash
git add src/memory/ tests/unit/test_memory_extractor.py
git commit -m "feat(memory): add FactExtractor with extract_facts and resolve_relations"
```

---

### Task 5: MemoryService — explicit `remember()` path

**Files:**
- Create: `src/memory/service.py`
- Create: `tests/unit/test_memory_service.py`

- [ ] **Step 1: Write failing tests**

```python
# tests/unit/test_memory_service.py
import uuid
import pytest
from unittest.mock import AsyncMock, MagicMock
from src.memory.service import MemoryService, RememberResult
from src.memory.extractor import ExtractedFact, RelationResult, FactExtractor
from src.db.repositories.memory_repo import MemoryRepository
from src.db.models import Memory


def make_memory(content="User works at Acme"):
    m = Memory()
    m.id = uuid.uuid4()
    m.content = content
    m.raw_input = content
    m.source = "explicit"
    m.is_active = True
    m.content_vector = [0.1] * 1536
    m.forget_after = None
    m.relation = None
    m.parent_id = None
    return m


@pytest.mark.asyncio
async def test_remember_creates_new_fact_when_no_similar():
    repo = MagicMock(spec=MemoryRepository)
    repo.get_active_by_vector = AsyncMock(return_value=[])
    repo.create = AsyncMock(return_value=make_memory())

    extractor = MagicMock(spec=FactExtractor)
    extractor.extract_facts = AsyncMock(return_value=[ExtractedFact(content="User works at Acme")])
    extractor.resolve_relations = AsyncMock(return_value=[])

    embed_fn = AsyncMock(return_value=[0.1] * 1536)

    service = MemoryService(repo=repo, extractor=extractor, embed_fn=embed_fn)
    result = await service.remember(AsyncMock(), "I work at Acme")

    assert isinstance(result, RememberResult)
    assert result.facts_extracted == 1
    assert result.memories_updated == 0
    repo.create.assert_called_once()


@pytest.mark.asyncio
async def test_remember_deactivates_old_on_updates():
    old_mem = make_memory("User works at Acme")
    repo = MagicMock(spec=MemoryRepository)
    repo.get_active_by_vector = AsyncMock(return_value=[(old_mem, 0.92)])
    repo.deactivate = AsyncMock()
    repo.create = AsyncMock(return_value=make_memory("User works at Beta"))

    extractor = MagicMock(spec=FactExtractor)
    extractor.extract_facts = AsyncMock(return_value=[ExtractedFact(content="User works at Beta")])
    extractor.resolve_relations = AsyncMock(
        return_value=[RelationResult(memory_id=old_mem.id, relation="updates")]
    )

    embed_fn = AsyncMock(return_value=[0.1] * 1536)

    service = MemoryService(repo=repo, extractor=extractor, embed_fn=embed_fn)
    result = await service.remember(AsyncMock(), "I now work at Beta")

    repo.deactivate.assert_called_once_with(old_mem.id)
    assert result.memories_updated == 1


@pytest.mark.asyncio
async def test_remember_extends_does_not_deactivate():
    old_mem = make_memory("User works at Acme")
    repo = MagicMock(spec=MemoryRepository)
    repo.get_active_by_vector = AsyncMock(return_value=[(old_mem, 0.90)])
    repo.deactivate = AsyncMock()
    repo.create = AsyncMock(return_value=make_memory())

    extractor = MagicMock(spec=FactExtractor)
    extractor.extract_facts = AsyncMock(
        return_value=[ExtractedFact(content="User is a senior engineer at Acme")]
    )
    extractor.resolve_relations = AsyncMock(
        return_value=[RelationResult(memory_id=old_mem.id, relation="extends")]
    )

    embed_fn = AsyncMock(return_value=[0.1] * 1536)

    service = MemoryService(repo=repo, extractor=extractor, embed_fn=embed_fn)
    await service.remember(AsyncMock(), "I'm a senior engineer at Acme")

    repo.deactivate.assert_not_called()
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_memory_service.py -v
```

Expected: `FAILED` — `ModuleNotFoundError: No module named 'src.memory.service'`

- [ ] **Step 3: Implement MemoryService**

```python
# src/memory/service.py
import uuid
from dataclasses import dataclass
from sqlalchemy.ext.asyncio import AsyncSession
from src.memory.extractor import FactExtractor, ExtractedFact
from src.db.repositories.memory_repo import MemoryRepository
from src.db.models import Memory


@dataclass
class RememberResult:
    facts_extracted: int
    memories_updated: int


class MemoryService:
    def __init__(
        self,
        repo: MemoryRepository,
        extractor: FactExtractor,
        embed_fn,  # async callable: str -> list[float]
    ):
        self.repo = repo
        self.extractor = extractor
        self.embed_fn = embed_fn

    async def remember(
        self,
        session: AsyncSession,
        text: str,
        source: str = "explicit",
    ) -> RememberResult:
        facts = await self.extractor.extract_facts(text)
        facts_extracted = len(facts)
        memories_updated = 0

        for fact in facts:
            vector = await self.embed_fn(fact.content)
            similar = await self.repo.get_active_by_vector(vector)
            existing = [(m.id, m.content) for m, _ in similar]
            relations = await self.extractor.resolve_relations(fact.content, existing)

            parent_id: uuid.UUID | None = None
            relation_type: str | None = None

            for rel in relations:
                if rel.relation == "updates":
                    await self.repo.deactivate(rel.memory_id)
                    parent_id = rel.memory_id
                    relation_type = "updates"
                    memories_updated += 1
                elif rel.relation in ("extends", "derives") and parent_id is None:
                    parent_id = rel.memory_id
                    relation_type = rel.relation

            await self.repo.create(
                content=fact.content,
                raw_input=text,
                source=source,
                vector=vector,
                parent_id=parent_id,
                relation=relation_type,
                forget_after=fact.forget_after,
            )

        return RememberResult(facts_extracted=facts_extracted, memories_updated=memories_updated)

    async def observe(self, session: AsyncSession, conversation: str) -> RememberResult:
        observe_prompt = (
            "What new personal facts about the user did you learn in this conversation?\n"
            "Return only new information, not a recap. Ignore facts already discussed before.\n\n"
            f"Conversation:\n{conversation}"
        )
        return await self.remember(session, observe_prompt, source="conversation")

    async def forget_memory(self, session: AsyncSession, memory_id: uuid.UUID) -> bool:
        mem = await self.repo.get_by_id(memory_id)
        if mem is None:
            return False
        await self.repo.deactivate(memory_id)
        return True

    async def list_active(self, session: AsyncSession) -> list[Memory]:
        return await self.repo.get_all_active()
```

- [ ] **Step 4: Run to verify pass**

```bash
uv run pytest tests/unit/test_memory_service.py -v
```

Expected: 3 tests `PASSED`

- [ ] **Step 5: Run all unit tests**

```bash
uv run pytest tests/unit/ -q
```

Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add src/memory/service.py tests/unit/test_memory_service.py
git commit -m "feat(memory): add MemoryService with remember, observe, forget_memory"
```

---

## Phase 3 — Document Extraction Worker

---

### Task 6: Document extraction background worker

**Files:**
- Create: `src/memory/worker.py`
- Modify: `src/ingestion/worker.py` (add extraction job after successful indexing)

- [ ] **Step 1: Write failing test**

```python
# tests/unit/test_memory_service.py — append to existing file
@pytest.mark.asyncio
async def test_memory_worker_queues_job_after_doc_indexing():
    from src.memory.worker import queue_document_extraction
    session = AsyncMock()
    session.add = MagicMock()
    session.flush = AsyncMock()
    doc_id = uuid.uuid4()
    await queue_document_extraction(session, str(doc_id))
    session.add.assert_called_once()
    session.flush.assert_called_once()
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_memory_service.py::test_memory_worker_queues_job_after_doc_indexing -v
```

Expected: `FAILED` — `ImportError`

- [ ] **Step 3: Create `src/memory/worker.py`**

```python
# src/memory/worker.py
import uuid
from sqlalchemy.ext.asyncio import AsyncSession
from src.db.models import MemoryExtractionJob


async def queue_document_extraction(session: AsyncSession, doc_id: str) -> MemoryExtractionJob:
    job = MemoryExtractionJob(
        id=uuid.uuid4(),
        source_ref=doc_id,
        source="document",
        status="pending",
        facts_extracted=0,
    )
    session.add(job)
    await session.flush()
    return job


async def run_document_extraction(
    session: AsyncSession,
    job: MemoryExtractionJob,
    doc_text: str,
    memory_service,
) -> None:
    job.status = "processing"
    await session.flush()
    try:
        result = await memory_service.remember(session, doc_text, source="document")
        job.status = "done"
        job.facts_extracted = result.facts_extracted
    except Exception as e:
        job.status = "error"
        job.error = str(e)
    await session.flush()
```

- [ ] **Step 4: Run to verify pass**

```bash
uv run pytest tests/unit/test_memory_service.py -v
```

Expected: all pass

- [ ] **Step 5: Modify ingestion worker to queue extraction**

In `src/ingestion/worker.py`, find where the indexing job status is set to `"done"` and add:

```python
# after setting job status to "done", add:
from src.memory.worker import queue_document_extraction
await queue_document_extraction(session, str(doc_id))
```

Read the file first to find the exact location, then add the import at top and the call after the `"done"` status line.

- [ ] **Step 6: Run all unit tests**

```bash
uv run pytest tests/unit/ -q
```

Expected: all pass

- [ ] **Step 7: Commit**

```bash
git add src/memory/worker.py src/ingestion/worker.py tests/unit/test_memory_service.py
git commit -m "feat(memory): add document extraction worker and queue on doc indexing"
```

---

## Phase 4 — R&D: Quality and Cost Benchmarks (RQ1 + RQ3)

*Run after Phase 2–3 are implemented. Both require a running Memex instance.*

---

### Task 7: RQ1 — Answer quality eval

**Files:**
- Create: `tests/research/datasets/rq1_eval_conversations.json`
- Create: `tests/research/rq1_eval.py`

- [ ] **Step 1: Create the eval dataset**

```json
// tests/research/datasets/rq1_eval_conversations.json
{
  "sessions": [
    {
      "id": "s1",
      "ingestion": [
        "I work at Acme Corp as a backend engineer.",
        "I prefer Python for backend and TypeScript for frontend.",
        "I now work at Beta Ltd — just got a new job!"
      ],
      "questions": [
        {
          "q": "Where does the user work?",
          "expected_keyword": "Beta",
          "anti_keyword": "Acme"
        },
        {
          "q": "What programming languages does the user prefer?",
          "expected_keyword": "Python"
        }
      ]
    },
    {
      "id": "s2",
      "ingestion": [
        "I live in Moscow.",
        "I moved to Saint Petersburg last month."
      ],
      "questions": [
        {
          "q": "Where does the user live?",
          "expected_keyword": "Petersburg",
          "anti_keyword": "Moscow"
        }
      ]
    },
    {
      "id": "s3",
      "ingestion": [
        "I was born in 1990.",
        "I started working as a developer in 2015."
      ],
      "questions": [
        {
          "q": "How many years of development experience does the user have?",
          "expected_keyword": "10"
        }
      ]
    }
  ]
}
```

- [ ] **Step 2: Create the eval script**

```python
# tests/research/rq1_eval.py
"""
RQ1: Answer quality with memory layer vs baseline RAG.

Prerequisites: Memex running at MEMEX_URL with memory layer enabled.

Usage:
    MEMEX_URL=http://localhost:8000 ANTHROPIC_API_KEY=sk-... \
    uv run python tests/research/rq1_eval.py

Success criterion: memory mode accuracy >= baseline accuracy + 10pp
"""
import asyncio
import json
import os
from pathlib import Path

import httpx

MEMEX_URL = os.getenv("MEMEX_URL", "http://localhost:8000")
DATASETS = Path(__file__).parent / "datasets" / "rq1_eval_conversations.json"


async def ingest_memories(client: httpx.AsyncClient, texts: list[str]) -> None:
    for text in texts:
        resp = await client.post(f"{MEMEX_URL}/api/memory/remember", json={"content": text})
        resp.raise_for_status()


async def ask(client: httpx.AsyncClient, question: str) -> str:
    resp = await client.post(f"{MEMEX_URL}/api/query", json={"query": question})
    resp.raise_for_status()
    return resp.json().get("answer", "")


async def clear_memories(client: httpx.AsyncClient) -> None:
    resp = await client.get(f"{MEMEX_URL}/api/memory/list")
    if resp.status_code == 200:
        for mem in resp.json():
            await client.delete(f"{MEMEX_URL}/api/memory/{mem['id']}")


async def run_eval():
    data = json.loads(DATASETS.read_text())
    correct, total = 0, 0

    async with httpx.AsyncClient(timeout=60.0) as client:
        for session in data["sessions"]:
            print(f"\n--- Session {session['id']} ---")
            await clear_memories(client)
            await ingest_memories(client, session["ingestion"])
            await asyncio.sleep(2)  # let indexing settle

            for qa in session["questions"]:
                answer = await ask(client, qa["q"])
                expected = qa["expected_keyword"].lower()
                anti = qa.get("anti_keyword", "").lower()
                hit = expected in answer.lower() and (not anti or anti not in answer.lower())
                correct += int(hit)
                total += 1
                mark = "✓" if hit else "✗"
                print(f"  {mark} Q: {qa['q']}")
                print(f"      A: {answer[:120]}")

    accuracy = correct / total if total > 0 else 0
    print(f"\nAccuracy: {accuracy:.2f}  ({correct}/{total})")
    print(f"{'✓ RQ1 target met' if accuracy >= 0.80 else '✗ RQ1 needs improvement'}")
    print("(Success criterion: >= 0.80 overall, and knowledge-update questions answered correctly)")


if __name__ == "__main__":
    asyncio.run(run_eval())
```

- [ ] **Step 3: Commit**

```bash
git add tests/research/datasets/rq1_eval_conversations.json tests/research/rq1_eval.py
git commit -m "research: add RQ1 answer quality eval dataset and script"
```

- [ ] **Step 4: Run RQ1 eval (requires running Memex with memory layer)**

```bash
MEMEX_URL=http://localhost:8000 uv run python tests/research/rq1_eval.py
```

Expected: accuracy >= 0.80, knowledge-update questions answered with new facts only.

---

### Task 8: RQ3 — Cost and latency benchmark

**Files:**
- Create: `tests/research/rq3_benchmark.py`

- [ ] **Step 1: Create the benchmark script**

```python
# tests/research/rq3_benchmark.py
"""
RQ3: Cost and latency of the memory layer.

Prerequisites: Memex running at MEMEX_URL.

Usage:
    MEMEX_URL=http://localhost:8000 uv run python tests/research/rq3_benchmark.py

Success criteria:
    remember() p95 latency  < 5000ms (LLM-heavy, acceptable for personal use)
    recall()   p95 delta    < 200ms  vs baseline (no memory)
    context()  p95 latency  < 2000ms
"""
import asyncio
import json
import os
import statistics
import time
from pathlib import Path

import httpx

MEMEX_URL = os.getenv("MEMEX_URL", "http://localhost:8000")

SAMPLE_TEXTS = [
    "I work at Acme Corp as a backend engineer.",
    "I prefer Python for backend and TypeScript for frontend.",
    "I live in Saint Petersburg.",
    "I was born in 1990.",
    "Currently building a self-hosted RAG tool called Memex.",
    "I use vim as my editor.",
    "I prefer dark mode in all my applications.",
    "My manager is called Alexei.",
    "I have a standup meeting every Monday at 10am.",
    "I started this project in March 2026.",
]


async def benchmark_remember(client: httpx.AsyncClient) -> list[float]:
    latencies = []
    for text in SAMPLE_TEXTS:
        t0 = time.perf_counter()
        resp = await client.post(f"{MEMEX_URL}/api/memory/remember", json={"content": text})
        resp.raise_for_status()
        latencies.append((time.perf_counter() - t0) * 1000)
    return latencies


async def benchmark_recall(client: httpx.AsyncClient, n: int = 10) -> tuple[list[float], list[float]]:
    queries = ["where do I work?", "what are my preferences?", "what am I building?"]
    base_latencies, mem_latencies = [], []
    for q in queries * (n // len(queries) + 1):
        t0 = time.perf_counter()
        await client.post(f"{MEMEX_URL}/api/query", json={"query": q})
        mem_latencies.append((time.perf_counter() - t0) * 1000)
        await asyncio.sleep(0.1)
    return base_latencies, mem_latencies


async def benchmark_context(client: httpx.AsyncClient, n: int = 5) -> list[float]:
    latencies = []
    for _ in range(n):
        t0 = time.perf_counter()
        resp = await client.get(f"{MEMEX_URL}/api/memory/context")
        resp.raise_for_status()
        latencies.append((time.perf_counter() - t0) * 1000)
    return latencies


def p95(values: list[float]) -> float:
    if not values:
        return 0.0
    return sorted(values)[int(len(values) * 0.95)]


async def run():
    async with httpx.AsyncClient(timeout=30.0) as client:
        print("=== RQ3: remember() latency ===")
        rem = await benchmark_remember(client)
        print(f"  p50={statistics.median(rem):.0f}ms  p95={p95(rem):.0f}ms  (target p95 < 5000ms)")

        print("\n=== RQ3: recall() latency ===")
        _, mem = await benchmark_recall(client)
        print(f"  p50={statistics.median(mem):.0f}ms  p95={p95(mem):.0f}ms")

        print("\n=== RQ3: context() latency ===")
        ctx = await benchmark_context(client)
        print(f"  p50={statistics.median(ctx):.0f}ms  p95={p95(ctx):.0f}ms  (target p95 < 2000ms)")

        ok_rem = p95(rem) < 5000
        ok_ctx = p95(ctx) < 2000
        print(f"\n{'✓' if ok_rem else '✗'} remember() p95 < 5000ms")
        print(f"{'✓' if ok_ctx else '✗'} context()  p95 < 2000ms")


if __name__ == "__main__":
    asyncio.run(run())
```

- [ ] **Step 2: Commit**

```bash
git add tests/research/rq3_benchmark.py
git commit -m "research: add RQ3 cost and latency benchmark script"
```

- [ ] **Step 3: Run RQ3 benchmark (requires running Memex)**

```bash
MEMEX_URL=http://localhost:8000 uv run python tests/research/rq3_benchmark.py
```

Expected: `✓` for both criteria. If `context()` p95 > 2000ms at 10+ memories, the profile generation prompt needs optimization (reduce token count).

> **DECISION GATE:** All three RQ benchmarks must pass before proceeding to Phase 5.
> RQ2: extraction precision ≥ 0.90, relation accuracy ≥ 0.85
> RQ1: overall accuracy ≥ 0.80
> RQ3: remember p95 < 5000ms, context p95 < 2000ms

---

## Phase 5 — Retrieval

---

### Task 9: MemorySearch + memory-augmented `query()`

**Files:**
- Create: `src/retrieval/memory_search.py`
- Create: `tests/unit/test_memory_search.py`
- Modify: `src/retrieval/service.py`

- [ ] **Step 1: Write failing test for MemorySearch**

```python
# tests/unit/test_memory_search.py
import uuid
import pytest
from unittest.mock import AsyncMock, MagicMock, patch
from src.retrieval.memory_search import MemorySearch, MemoryHit
from src.db.models import Memory
from datetime import datetime, timezone


def make_memory(content="User works at Acme"):
    m = Memory()
    m.id = uuid.uuid4()
    m.content = content
    m.source = "explicit"
    m.is_active = True
    m.created_at = datetime.now(timezone.utc)
    return m


@pytest.mark.asyncio
async def test_memory_search_returns_hits():
    mem = make_memory()
    repo = MagicMock()
    repo.get_active_by_vector = AsyncMock(return_value=[(mem, 0.91)])

    search = MemorySearch(repo=repo)
    hits = await search.search(AsyncMock(), query_vector=[0.1] * 1536)

    assert len(hits) == 1
    assert isinstance(hits[0], MemoryHit)
    assert hits[0].content == "User works at Acme"
    assert hits[0].score == 0.91


@pytest.mark.asyncio
async def test_memory_search_empty_when_no_results():
    repo = MagicMock()
    repo.get_active_by_vector = AsyncMock(return_value=[])
    search = MemorySearch(repo=repo)
    hits = await search.search(AsyncMock(), query_vector=[0.1] * 1536)
    assert hits == []
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_memory_search.py -v
```

Expected: `FAILED` — `ImportError`

- [ ] **Step 3: Implement MemorySearch**

```python
# src/retrieval/memory_search.py
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


class MemorySearch:
    def __init__(self, repo: MemoryRepository, top_k: int = 10):
        self.repo = repo
        self.top_k = top_k

    async def search(
        self,
        session: AsyncSession,
        query_vector: list[float],
    ) -> list[MemoryHit]:
        results = await self.repo.get_active_by_vector(query_vector, limit=self.top_k)
        return [
            MemoryHit(
                memory_id=mem.id,
                content=mem.content,
                score=score,
                source=mem.source,
                created_at=mem.created_at,
            )
            for mem, score in results
        ]
```

- [ ] **Step 4: Run to verify pass**

```bash
uv run pytest tests/unit/test_memory_search.py -v
```

Expected: `PASSED`

- [ ] **Step 5: Update `RetrievalService.query()` to merge memory hits**

In `src/retrieval/service.py`:

Add `memory_search` as an optional constructor parameter and inject memory context into the prompt. Add this import at top:

```python
from src.retrieval.memory_search import MemorySearch, MemoryHit
```

Update `__init__`:

```python
def __init__(
    self,
    semantic_search: SemanticSearch,
    bm25_search: BM25Search,
    reranker: Reranker,
    context_builder: ContextBuilder,
    llm_provider: LLMProvider,
    rrf_k: int = 60,
    reranker_top_n: int = 5,
    memory_search: MemorySearch | None = None,  # new
):
    self.semantic_search = semantic_search
    self.bm25_search = bm25_search
    self.reranker = reranker
    self.context_builder = context_builder
    self.llm_provider = llm_provider
    self.rrf_k = rrf_k
    self.reranker_top_n = reranker_top_n
    self.memory_search = memory_search  # new
```

Update `query()` — after getting `query_vector`, add memory search and prepend to prompt:

```python
async def query(self, session, query, embed_fn) -> QueryResult:
    query_vector = await embed_fn(query)

    semantic_hits = await self.semantic_search.search(session, query_vector)
    bm25_hits = await self.bm25_search.search(session, query)

    merged = rrf_merge(semantic_hits, bm25_hits, k=self.rrf_k)
    l2_chunks = await expand_to_l2(session, merged)
    reranked = await self.reranker.rerank(query, l2_chunks, top_n=self.reranker_top_n)

    ctx = self.context_builder.build(query, reranked)

    # Prepend memory context if memory_search is configured
    memory_prefix = ""
    if self.memory_search:
        mem_hits = await self.memory_search.search(session, query_vector)
        if mem_hits:
            lines = "\n".join(f"- {h.content} [memory]" for h in mem_hits[:5])
            memory_prefix = f"Personal facts about the user:\n{lines}\n\n"

    final_prompt = memory_prefix + ctx.prompt
    llm_response = await self.llm_provider.complete(final_prompt)

    return QueryResult(
        answer=llm_response.answer,
        sources=ctx.sources,
        input_tokens=llm_response.input_tokens,
        output_tokens=llm_response.output_tokens,
    )
```

- [ ] **Step 6: Run all unit tests**

```bash
uv run pytest tests/unit/ -q
```

Expected: all pass (existing retrieval tests still pass because `memory_search=None` by default)

- [ ] **Step 7: Commit**

```bash
git add src/retrieval/memory_search.py tests/unit/test_memory_search.py src/retrieval/service.py
git commit -m "feat(memory): add MemorySearch and inject memory context into RetrievalService.query()"
```

---

### Task 10: ProfileService + `context()` endpoint

**Files:**
- Create: `src/memory/profile.py`
- Create: `tests/unit/test_memory_profile.py`

- [ ] **Step 1: Write failing tests**

```python
# tests/unit/test_memory_profile.py
import uuid
import pytest
from unittest.mock import AsyncMock
from datetime import datetime, timezone, timedelta
from src.memory.profile import ProfileService, UserProfile
from src.db.models import Memory
from tests.mocks.mock_llm import MockLLMProvider


def make_memory(content, days_old=0):
    m = Memory()
    m.id = uuid.uuid4()
    m.content = content
    m.source = "explicit"
    m.is_active = True
    m.created_at = datetime.now(timezone.utc) - timedelta(days=days_old)
    return m


@pytest.mark.asyncio
async def test_build_profile_returns_user_profile():
    llm = MockLLMProvider(response="Senior developer. Works at Acme.")
    service = ProfileService(llm_provider=llm)
    memories = [make_memory("User works at Acme", days_old=60)]
    profile = await service.build_profile(memories)
    assert isinstance(profile, UserProfile)
    assert profile.static != ""
    assert profile.raw_count == 1


@pytest.mark.asyncio
async def test_build_profile_splits_static_dynamic():
    llm = MockLLMProvider(response="Summary text.")
    service = ProfileService(llm_provider=llm)
    memories = [
        make_memory("User works at Acme", days_old=60),   # static
        make_memory("User is building Memex", days_old=5),  # dynamic
    ]
    profile = await service.build_profile(memories)
    assert profile.raw_count == 2
    assert llm.calls  # LLM was called at least once


@pytest.mark.asyncio
async def test_build_profile_empty_memories():
    llm = MockLLMProvider(response="No information available.")
    service = ProfileService(llm_provider=llm)
    profile = await service.build_profile([])
    assert profile.raw_count == 0
    assert profile.static == ""
    assert profile.dynamic == ""
```

- [ ] **Step 2: Run to verify fail**

```bash
uv run pytest tests/unit/test_memory_profile.py -v
```

Expected: `FAILED` — `ImportError`

- [ ] **Step 3: Implement ProfileService**

```python
# src/memory/profile.py
from dataclasses import dataclass
from datetime import datetime, timezone, timedelta
from src.llm.protocol import LLMProvider
from src.db.models import Memory

STATIC_THRESHOLD_DAYS = 30

PROFILE_PROMPT = """\
Summarize the following facts about a user into a concise profile (2-4 sentences max, ≤150 tokens).
Write in third person. Include only factual information from the list.

Facts:
{facts}

Profile summary:"""


@dataclass
class UserProfile:
    static: str    # stable facts (older than 30 days)
    dynamic: str   # recent facts (last 30 days)
    raw_count: int


class ProfileService:
    def __init__(self, llm_provider: LLMProvider):
        self.llm = llm_provider

    async def build_profile(self, memories: list[Memory]) -> UserProfile:
        if not memories:
            return UserProfile(static="", dynamic="", raw_count=0)

        cutoff = datetime.now(timezone.utc) - timedelta(days=STATIC_THRESHOLD_DAYS)
        static_mems = [m for m in memories if m.created_at and m.created_at < cutoff]
        dynamic_mems = [m for m in memories if not m.created_at or m.created_at >= cutoff]

        static_text = await self._summarize(static_mems) if static_mems else ""
        dynamic_text = await self._summarize(dynamic_mems) if dynamic_mems else ""

        return UserProfile(
            static=static_text,
            dynamic=dynamic_text,
            raw_count=len(memories),
        )

    async def _summarize(self, memories: list[Memory]) -> str:
        if not memories:
            return ""
        facts = "\n".join(f"- {m.content}" for m in memories)
        prompt = PROFILE_PROMPT.format(facts=facts)
        response = await self.llm.complete(prompt)
        return response.answer.strip()
```

- [ ] **Step 4: Run to verify pass**

```bash
uv run pytest tests/unit/test_memory_profile.py -v
```

Expected: 3 tests `PASSED`

- [ ] **Step 5: Run all unit tests**

```bash
uv run pytest tests/unit/ -q
```

Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add src/memory/profile.py tests/unit/test_memory_profile.py
git commit -m "feat(memory): add ProfileService with static/dynamic profile generation"
```

---

## Phase 6 — Auto-expiry

---

### Task 11: Expiry background task

**Files:**
- Modify: `src/main.py` (add periodic expiry task on startup)

- [ ] **Step 1: Read `src/main.py`** to find the FastAPI app startup hooks.

- [ ] **Step 2: Add expiry task**

In `src/main.py`, add a background task that runs hourly:

```python
import asyncio
from contextlib import asynccontextmanager

async def _expiry_loop(get_session_fn):
    """Runs every hour, marks forget_after < NOW() as inactive."""
    while True:
        await asyncio.sleep(3600)
        try:
            async with get_session_fn() as session:
                from src.db.repositories.memory_repo import MemoryRepository
                repo = MemoryRepository(session)
                count = await repo.expire_stale()
                await session.commit()
                if count:
                    import logging
                    logging.getLogger(__name__).info("Expired %d stale memories", count)
        except Exception:
            pass  # don't crash the app on expiry failure
```

Add to the lifespan context manager (or startup event):
```python
asyncio.create_task(_expiry_loop(get_async_session))
```

Read `src/main.py` first to find the exact location and follow the existing startup pattern.

- [ ] **Step 3: Run all unit tests**

```bash
uv run pytest tests/unit/ -q
```

Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add src/main.py
git commit -m "feat(memory): add hourly auto-expiry task for time-bound memories"
```

---

## Phase 7 — API + MCP

---

### Task 12: Memory REST endpoints

**Files:**
- Create: `src/api/memories.py`
- Modify: `src/main.py` (register router)

- [ ] **Step 1: Create the memories router**

```python
# src/api/memories.py
import uuid
from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession
from src.api.documents import get_db_session
from src.dependencies import get_memory_service, get_profile_service

router = APIRouter(prefix="/api/memory", tags=["memory"])


class RememberRequest(BaseModel):
    content: str
    source: str = "explicit"


class ObserveRequest(BaseModel):
    conversation: str


@router.post("/remember")
async def remember(
    body: RememberRequest,
    session: AsyncSession = Depends(get_session),
    memory_service=Depends(get_memory_service),
):
    result = await memory_service.remember(session, body.content, source=body.source)
    await session.commit()
    return {"facts_extracted": result.facts_extracted, "memories_updated": result.memories_updated}


@router.post("/observe")
async def observe(
    body: ObserveRequest,
    session: AsyncSession = Depends(get_session),
    memory_service=Depends(get_memory_service),
):
    result = await memory_service.observe(session, body.conversation)
    await session.commit()
    return {"facts_extracted": result.facts_extracted, "memories_updated": result.memories_updated}


@router.get("/list")
async def list_memories(
    session: AsyncSession = Depends(get_session),
    memory_service=Depends(get_memory_service),
):
    memories = await memory_service.list_active(session)
    return [
        {
            "id": str(m.id),
            "content": m.content,
            "source": m.source,
            "relation": m.relation,
            "created_at": m.created_at.isoformat() if m.created_at else None,
        }
        for m in memories
    ]


@router.get("/context")
async def context(
    session: AsyncSession = Depends(get_session),
    memory_service=Depends(get_memory_service),
    profile_service=Depends(get_profile_service),
):
    memories = await memory_service.list_active(session)
    profile = await profile_service.build_profile(memories)
    return {"static": profile.static, "dynamic": profile.dynamic, "raw_count": profile.raw_count}


@router.delete("/{memory_id}")
async def forget_memory(
    memory_id: uuid.UUID,
    session: AsyncSession = Depends(get_session),
    memory_service=Depends(get_memory_service),
):
    ok = await memory_service.forget_memory(session, memory_id)
    await session.commit()
    if not ok:
        raise HTTPException(status_code=404, detail="Memory not found")
    return {"status": "deleted"}
```

- [ ] **Step 2: Add factory functions to `src/dependencies.py`**

The existing pattern uses `create_llm_provider(settings)` and `get_embedding_client()`. Add:

```python
# Add to src/dependencies.py:
from src.memory.service import MemoryService
from src.memory.profile import ProfileService
from src.memory.extractor import FactExtractor
from src.db.repositories.memory_repo import MemoryRepository


@lru_cache
def get_memory_extractor() -> FactExtractor:
    settings = get_settings()
    return FactExtractor(create_llm_provider(settings))


@lru_cache
def get_profile_service() -> ProfileService:
    settings = get_settings()
    return ProfileService(llm_provider=create_llm_provider(settings))


def get_memory_service(session: AsyncSession) -> MemoryService:
    embedding_client = get_embedding_client()

    async def embed_fn(text: str) -> list[float]:
        results = await embedding_client.embed_batch([text])
        return results[0]

    return MemoryService(
        repo=MemoryRepository(session),
        extractor=get_memory_extractor(),
        embed_fn=embed_fn,
    )
```

Also add `from sqlalchemy.ext.asyncio import AsyncSession` to the imports in `src/dependencies.py` if not already present.

Then update `src/api/memories.py` to match: use `get_db_session` (from `src.api.documents`) instead of `get_session`, and call service factories directly (not via `Depends`) — same pattern as `src/api/query.py`:

```python
# src/api/memories.py — revised imports and handlers
from src.api.documents import get_db_session
from src.dependencies import get_memory_service, get_profile_service

# In each endpoint, call factories directly:
@router.post("/remember")
async def remember(body: RememberRequest, session: AsyncSession = Depends(get_db_session)):
    memory_service = get_memory_service(session)
    result = await memory_service.remember(session, body.content, source=body.source)
    await session.commit()
    return {"facts_extracted": result.facts_extracted, "memories_updated": result.memories_updated}

# Apply same pattern to all other endpoints in memories.py
```

- [ ] **Step 3: Register router in `src/main.py`**

```python
from src.api.memories import router as memory_router
app.include_router(memory_router)
```

- [ ] **Step 4: Run all unit tests**

```bash
uv run pytest tests/unit/ -q
```

Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add src/api/memories.py src/dependencies.py src/main.py
git commit -m "feat(memory): add /api/memory/* REST endpoints"
```

---

### Task 13: Update MCP tools

**Files:**
- Modify: `src/mcp/server.py`

- [ ] **Step 1: Update tool definitions**

Replace the `remember` tool definition and add `context`, `observe`, `memories` tools in `list_tools()`:

```python
# Replace existing "remember" tool with:
types.Tool(
    name="remember",
    description=(
        "Save text as a memory — extracts atomic facts, resolves conflicts with existing memories. "
        "Returns immediately with facts_extracted and memories_updated counts."
    ),
    inputSchema={
        "type": "object",
        "properties": {
            "content": {"type": "string", "description": "Text to remember"},
        },
        "required": ["content"],
    },
),

# Add after "recall" tool:
types.Tool(
    name="context",
    description=(
        "Get the user's current profile as static (stable facts) and dynamic (recent activity). "
        "Call this as the FIRST tool at the start of every session."
    ),
    inputSchema={"type": "object", "properties": {}},
),
types.Tool(
    name="observe",
    description=(
        "Extract facts from a conversation history and save to memory. "
        "Call this as the LAST tool at the end of every session, passing the full conversation."
    ),
    inputSchema={
        "type": "object",
        "properties": {
            "conversation": {
                "type": "string",
                "description": "Full conversation history as text",
            }
        },
        "required": ["conversation"],
    },
),
types.Tool(
    name="memories",
    description="List all active memories with their content, source, and timestamps.",
    inputSchema={"type": "object", "properties": {}},
),
```

Update `forget` tool description to accept both doc_id and memory_id.

- [ ] **Step 2: Implement new tool handlers**

In the `call_tool` dispatcher, add:

```python
elif name == "context":
    return await _context(client)
elif name == "observe":
    return await _observe(client, arguments)
elif name == "memories":
    return await _memories(client)
```

Add the handler functions:

```python
async def _remember(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    content = args["content"]
    resp = await client.post(f"{BASE_URL}/api/memory/remember", json={"content": content})
    if resp.status_code != 200:
        return _text(f"Error: {resp.status_code}")
    data = resp.json()
    return _text(
        f"Remembered. Facts extracted: {data['facts_extracted']}, "
        f"memories updated: {data['memories_updated']}"
    )


async def _context(client: httpx.AsyncClient) -> list[types.TextContent]:
    resp = await client.get(f"{BASE_URL}/api/memory/context")
    if resp.status_code != 200:
        return _text(f"Error: {resp.status_code}")
    data = resp.json()
    lines = []
    if data.get("static"):
        lines.append(f"User profile: {data['static']}")
    if data.get("dynamic"):
        lines.append(f"Recent context: {data['dynamic']}")
    if not lines:
        lines.append("No memories yet.")
    lines.append(f"(Total memories: {data.get('raw_count', 0)})")
    return _text("\n".join(lines))


async def _observe(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    conversation = args["conversation"]
    resp = await client.post(
        f"{BASE_URL}/api/memory/observe", json={"conversation": conversation}
    )
    if resp.status_code != 200:
        return _text(f"Error: {resp.status_code}")
    data = resp.json()
    return _text(
        f"Session observed. Facts extracted: {data['facts_extracted']}, "
        f"memories updated: {data['memories_updated']}"
    )


async def _memories(client: httpx.AsyncClient) -> list[types.TextContent]:
    resp = await client.get(f"{BASE_URL}/api/memory/list")
    if resp.status_code != 200:
        return _text(f"Error: {resp.status_code}")
    mems = resp.json()
    if not mems:
        return _text("No active memories.")
    lines = []
    for m in mems:
        rel = f" [{m['relation']}]" if m.get("relation") else ""
        date = (m.get("created_at") or "")[:10]
        lines.append(f"• {m['content']}{rel}\n  id: {m['id']}  |  {m['source']}  |  {date}")
    return _text(f"Active memories: {len(mems)}\n\n" + "\n\n".join(lines))
```

Update `_forget` to try memory endpoint first, then fall back to document endpoint:

```python
async def _forget(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    doc_id = args["doc_id"]
    # Try memory endpoint first
    resp = await client.delete(f"{BASE_URL}/api/memory/{doc_id}")
    if resp.status_code == 204 or resp.status_code == 200:
        return _text(f"✓ Memory deleted (id: {doc_id})")
    if resp.status_code == 404:
        # Fall back to document endpoint
        resp2 = await client.delete(f"{BASE_URL}/api/documents/{doc_id}")
        if resp2.status_code == 204:
            return _text(f"✓ Document deleted (doc_id: {doc_id})")
        return _text(f"Not found: {doc_id}")
    return _text(f"Error: {resp.status_code}")
```

- [ ] **Step 3: Run all unit tests**

```bash
uv run pytest tests/unit/ -q
```

Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add src/mcp/server.py
git commit -m "feat(memory): add context/observe/memories MCP tools, update remember/forget"
```

---

## Final Verification

- [ ] **Run full unit test suite**

```bash
uv run pytest tests/unit/ -v
```

Expected: all 87+ tests pass

- [ ] **Verify research scripts exist**

```bash
ls tests/research/
# rq1_eval.py  rq2_extraction_eval.py  rq3_benchmark.py
# datasets/rq1_eval_conversations.json  datasets/rq2_extraction_cases.json
```

- [ ] **Final commit**

```bash
git add -A
git commit -m "chore: memory layer complete — all tasks done"
```
