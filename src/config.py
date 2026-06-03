from pathlib import Path
from typing import Literal

from pydantic import model_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    # Database
    database_url: str

    # OpenAI Embeddings (always required — used regardless of LLM provider)
    openai_api_key: str
    embedding_model: str = "text-embedding-3-small"
    embedding_dimensions: int = 1536

    # LLM — both fields are required, no defaults
    llm_provider: Literal["claude", "openai"]
    llm_model: str
    llm_max_tokens: int = 2048
    llm_temperature: float = 0.1

    # LLM credentials — one must be set depending on llm_provider
    anthropic_api_key: str | None = None   # required when llm_provider=claude
    openai_llm_api_key: str | None = None  # required when llm_provider=openai

    # Chunking
    l2_chunk_size: int = 512
    l1_chunk_size: int = 128
    l2_chunk_overlap: int = 64

    # Retrieval
    semantic_top_k: int = 20
    bm25_top_k: int = 20
    rrf_k: int = 60
    reranker_top_n: int = 5

    # Storage
    upload_dir: Path = Path("data/uploads")

    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    @model_validator(mode="after")
    def validate_llm_credentials(self) -> "Settings":
        if self.llm_provider == "claude" and not self.anthropic_api_key:
            raise ValueError(
                "ANTHROPIC_API_KEY is required when LLM_PROVIDER=claude"
            )
        if self.llm_provider == "openai" and not self.openai_llm_api_key:
            raise ValueError(
                "OPENAI_LLM_API_KEY is required when LLM_PROVIDER=openai"
            )
        return self


_settings: Settings | None = None


def get_settings() -> Settings:
    global _settings
    if _settings is None:
        _settings = Settings()
    return _settings
