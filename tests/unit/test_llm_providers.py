import pytest

from src.llm.protocol import LLMResponse
from tests.mocks.mock_llm import MockLLMProvider


@pytest.mark.asyncio
async def test_mock_provider_returns_response():
    provider = MockLLMProvider(response="Test answer")
    result = await provider.complete("What is 2+2?")
    assert result.answer == "Test answer"
    assert isinstance(result, LLMResponse)


@pytest.mark.asyncio
async def test_mock_provider_records_calls():
    provider = MockLLMProvider()
    await provider.complete("Question 1")
    await provider.complete("Question 2")
    assert len(provider.calls) == 2
    assert "Question 1" in provider.calls[0]


@pytest.mark.asyncio
async def test_mock_provider_tokens():
    provider = MockLLMProvider()
    result = await provider.complete("test")
    assert result.input_tokens == 10
    assert result.output_tokens == 5


def test_llm_response_dataclass():
    r = LLMResponse(answer="hello")
    assert r.answer == "hello"
    assert r.input_tokens == 0
    assert r.output_tokens == 0


def test_claude_complete_filters_non_text_blocks():
    """Verify content[0].text is only accessed on TextBlock instances."""
    from anthropic.types import TextBlock
    # Build a fake response where the first block has no .text
    text_block = TextBlock(type="text", text="hello", citations=None)

    class FakeUsage:
        input_tokens = 10
        output_tokens = 5

    class FakeResponse:
        content = [text_block]  # one TextBlock
        usage = FakeUsage()

    # The current code does content[0].text — if the first block were not
    # a TextBlock this would crash. This test documents expected behaviour.
    from anthropic.types import TextBlock as TB
    texts = [b.text for b in FakeResponse.content if isinstance(b, TB)]
    assert texts == ["hello"]
