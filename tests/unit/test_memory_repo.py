import uuid
import pytest
from unittest.mock import AsyncMock, MagicMock
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
