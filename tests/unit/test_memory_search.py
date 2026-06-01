import uuid
import pytest
from unittest.mock import AsyncMock, MagicMock
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
