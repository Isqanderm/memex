from src.llm.protocol import LLMResponse


class MockLLMProvider:
    def __init__(self, response: str = "Mock answer"):
        self.response = response
        self.calls: list[str] = []

    async def complete(self, prompt: str) -> LLMResponse:
        self.calls.append(prompt)
        return LLMResponse(answer=self.response, input_tokens=10, output_tokens=5)
