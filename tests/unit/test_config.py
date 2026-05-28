import pytest
from src.config import Settings

def test_settings_defaults():
    s = Settings(
        database_url="postgresql+asyncpg://x:x@localhost/x",
        openai_api_key="sk-test",
    )
    assert s.embedding_model == "text-embedding-3-small"
    assert s.l2_chunk_size == 512
    assert s.l1_chunk_size == 128
    assert s.rrf_k == 60
    assert s.llm_provider == "claude"

def test_settings_upload_dir_is_path():
    from pathlib import Path
    s = Settings(
        database_url="postgresql+asyncpg://x:x@localhost/x",
        openai_api_key="sk-test",
    )
    assert isinstance(s.upload_dir, Path)
