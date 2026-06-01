import pytest
from pydantic import ValidationError
from src.config import Settings


def _base(**kwargs) -> dict:
    """Minimal valid settings dict — extend per test."""
    return {
        "database_url": "postgresql+asyncpg://x:x@localhost/x",
        "openai_api_key": "sk-test",
        **kwargs,
    }


# ── valid configurations ───────────────────────────────────────────────────

def test_settings_openai_provider():
    s = Settings(_env_file=None, **_base(llm_provider="openai", llm_model="gpt-4o", openai_llm_api_key="sk-llm"))
    assert s.llm_provider == "openai"
    assert s.llm_model == "gpt-4o"


def test_settings_claude_provider():
    s = Settings(_env_file=None, **_base(llm_provider="claude", llm_model="claude-opus-4-5", anthropic_api_key="sk-ant-test"))
    assert s.llm_provider == "claude"


def test_settings_defaults_unchanged():
    s = Settings(_env_file=None, **_base(llm_provider="openai", llm_model="gpt-4o-mini", openai_llm_api_key="sk-llm"))
    assert s.local_embedding_model == "intfloat/multilingual-e5-small"
    assert s.l2_chunk_size == 512
    assert s.l1_chunk_size == 128
    assert s.rrf_k == 60
    assert s.llm_max_tokens == 2048
    assert s.llm_temperature == 0.1


def test_settings_upload_dir_is_path():
    from pathlib import Path
    s = Settings(_env_file=None, **_base(llm_provider="openai", llm_model="gpt-4o-mini", openai_llm_api_key="sk-llm"))
    assert isinstance(s.upload_dir, Path)


# ── missing required fields ────────────────────────────────────────────────

def test_missing_llm_provider_raises():
    with pytest.raises(ValidationError, match="llm_provider"):
        Settings(_env_file=None, **_base(llm_model="gpt-4o"))


def test_missing_llm_model_raises():
    with pytest.raises(ValidationError, match="llm_model"):
        Settings(_env_file=None, **_base(llm_provider="openai"))


def test_missing_database_url_raises():
    with pytest.raises(ValidationError):
        Settings(_env_file=None, openai_api_key="sk-test", llm_provider="openai", llm_model="gpt-4o", openai_llm_api_key="sk-llm")


def test_missing_openai_api_key_raises():
    with pytest.raises(ValidationError):
        Settings(_env_file=None, database_url="postgresql+asyncpg://x:x@localhost/x",
                 llm_provider="openai", llm_model="gpt-4o", openai_llm_api_key="sk-llm")


# ── credential validation ──────────────────────────────────────────────────

def test_claude_without_anthropic_key_raises():
    with pytest.raises(ValidationError, match="ANTHROPIC_API_KEY"):
        Settings(_env_file=None, **_base(llm_provider="claude", llm_model="claude-opus-4-5"))


def test_claude_without_anthropic_key_raises_even_with_openai_llm_key():
    with pytest.raises(ValidationError, match="ANTHROPIC_API_KEY"):
        Settings(_env_file=None, **_base(llm_provider="claude", llm_model="claude-opus-4-5", openai_llm_api_key="sk-llm"))


def test_openai_without_llm_api_key_raises():
    with pytest.raises(ValidationError, match="OPENAI_LLM_API_KEY"):
        Settings(_env_file=None, **_base(llm_provider="openai", llm_model="gpt-4o"))


def test_openai_without_llm_api_key_raises_even_with_anthropic_key():
    with pytest.raises(ValidationError, match="OPENAI_LLM_API_KEY"):
        Settings(_env_file=None, **_base(llm_provider="openai", llm_model="gpt-4o", anthropic_api_key="sk-ant-test"))


def test_invalid_provider_raises():
    with pytest.raises(ValidationError):
        Settings(_env_file=None, **_base(llm_provider="gemini", llm_model="gemini-pro", openai_llm_api_key="sk-llm"))
