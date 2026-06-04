from typing import AsyncIterator

from anthropic.types import TextBlock

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
        text_blocks = [b for b in response.content if isinstance(b, TextBlock)]
        answer = text_blocks[0].text if text_blocks else ""
        return LLMResponse(
            answer=answer,
            input_tokens=response.usage.input_tokens,
            output_tokens=response.usage.output_tokens,
        )

    async def complete_stream(self, prompt: str) -> AsyncIterator[str]:
        async with self._client.messages.stream(
            model=self.model,
            max_tokens=self.max_tokens,
            temperature=self.temperature,
            messages=[{"role": "user", "content": prompt}],
        ) as stream:
            async for text in stream.text_stream:
                yield text
