from src.llm.protocol import LLMResponse


class OpenAIProvider:
    def __init__(self, api_key: str, model: str = "gpt-4o",
                 max_tokens: int = 2048, temperature: float = 0.1):
        import openai
        self._client = openai.AsyncOpenAI(api_key=api_key)
        self.model = model
        self.max_tokens = max_tokens
        self.temperature = temperature

    async def complete(self, prompt: str) -> LLMResponse:
        response = await self._client.chat.completions.create(
            model=self.model,
            max_tokens=self.max_tokens,
            temperature=self.temperature,
            messages=[{"role": "user", "content": prompt}],
        )
        choice = response.choices[0]
        usage = response.usage
        return LLMResponse(
            answer=choice.message.content or "",
            input_tokens=usage.prompt_tokens if usage else 0,
            output_tokens=usage.completion_tokens if usage else 0,
        )
