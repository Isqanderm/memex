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
