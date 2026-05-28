from pathlib import Path
from typing import Literal
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    # Database
    database_url: str

    # OpenAI Embeddings
    openai_api_key: str
    embedding_model: str = "text-embedding-3-small"
    embedding_dimensions: int = 1536

    # LLM
    llm_provider: Literal["claude", "openai"] = "claude"
    llm_model: str = "claude-opus-4-7"
    llm_max_tokens: int = 2048
    llm_temperature: float = 0.1
    anthropic_api_key: str | None = None
    openai_llm_api_key: str | None = None

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


_settings: Settings | None = None


def get_settings() -> Settings:
    global _settings
    if _settings is None:
        _settings = Settings()
    return _settings
