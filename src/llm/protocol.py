from dataclasses import dataclass
from typing import AsyncIterator, Protocol


@dataclass
class LLMResponse:
    answer: str
    input_tokens: int = 0
    output_tokens: int = 0


class LLMProvider(Protocol):
    async def complete(self, prompt: str) -> LLMResponse: ...
    def complete_stream(self, prompt: str) -> AsyncIterator[str]: ...
