from src.llm.protocol import LLMProvider


def create_llm_provider(settings) -> LLMProvider:
    if settings.llm_provider == "claude":
        from src.llm.claude import ClaudeProvider
        return ClaudeProvider(
            api_key=settings.anthropic_api_key or "",
            model=settings.llm_model,
            max_tokens=settings.llm_max_tokens,
            temperature=settings.llm_temperature,
        )
    elif settings.llm_provider == "openai":
        from src.llm.openai_provider import OpenAIProvider
        return OpenAIProvider(
            api_key=settings.openai_llm_api_key or "",
            model=settings.llm_model,
            max_tokens=settings.llm_max_tokens,
            temperature=settings.llm_temperature,
        )
    raise ValueError(f"Unknown LLM provider: {settings.llm_provider}")
