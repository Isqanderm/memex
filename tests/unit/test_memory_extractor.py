import uuid

import pytest

from src.memory.extractor import FactExtractor
from tests.mocks.mock_llm import MockLLMProvider


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
        response='{"relations": [{"id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "type": "updates"}]}'
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
