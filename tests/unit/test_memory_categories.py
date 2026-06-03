import uuid
from datetime import datetime, timezone
from unittest.mock import AsyncMock, MagicMock

import pytest

from src.db.models import Memory
from src.db.repositories.memory_repo import MemoryRepository
from src.memory.extractor import ExtractedFact, FactExtractor
from src.memory.service import MemoryService
from src.retrieval.memory_search import MemorySearch


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
