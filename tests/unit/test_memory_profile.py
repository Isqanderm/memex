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
        make_memory("User works at Acme", days_old=60),
        make_memory("User is building Memex", days_old=5),
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
