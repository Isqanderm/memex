# tests/golden/test_search.py
from typing import Any

import httpx
import pytest


def _extract_chunks(data: Any) -> list:
    """Rust returns a list directly; Python wraps in {chunks: [...]}."""
    if isinstance(data, list):
        return data  # Rust
    if isinstance(data, dict) and "chunks" in data:
        return data["chunks"]  # Python
    pytest.fail(f"Unexpected /api/search/chunks response shape: {type(data).__name__}: {data!r}")


@pytest.mark.unit
class TestSearchContract:
    """API contract tests for /api/search/chunks — requires embedding model, no LLM."""

    def test_chunk_search_returns_200(self, client: httpx.Client) -> None:
        resp = client.post(
            "/api/search/chunks",
            json={"query": "golden test document"},
        )
        assert resp.status_code == 200

    def test_chunk_search_returns_list_or_wrapped_list(self, client: httpx.Client) -> None:
        resp = client.post(
            "/api/search/chunks",
            json={"query": "golden test document"},
        )
        chunks = _extract_chunks(resp.json())
        assert isinstance(chunks, list)

    def test_chunk_search_with_top_k(self, client: httpx.Client) -> None:
        resp = client.post(
            "/api/search/chunks",
            json={"query": "memex rag", "top_k": 3},
        )
        assert resp.status_code == 200
        chunks = _extract_chunks(resp.json())
        assert len(chunks) <= 3

    def test_chunk_item_has_required_fields(self, client: httpx.Client) -> None:
        """If chunks are returned, each must have content and doc_id."""
        resp = client.post(
            "/api/search/chunks",
            json={"query": "fox jumps over lazy dog"},
        )
        chunks = _extract_chunks(resp.json())
        if not chunks:
            pytest.skip("No chunks returned — index may be empty")
        chunk = chunks[0]
        assert "content" in chunk, f"Missing 'content' in chunk: {chunk}"
        assert "doc_id" in chunk, f"Missing 'doc_id' in chunk: {chunk}"

    def test_chunk_search_empty_query_string(self, client: httpx.Client) -> None:
        """Empty query should not crash the server (400 or 200 with empty results)."""
        resp = client.post(
            "/api/search/chunks",
            json={"query": ""},
        )
        assert resp.status_code in (200, 400, 422)

    @pytest.mark.e2e
    def test_query_endpoint_returns_answer_and_sources(self, client: httpx.Client) -> None:
        """Requires LLM. Tests that /api/query returns answer and sources fields."""
        resp = client.post(
            "/api/query",
            json={"query": "What is this document about?", "top_k": 2},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert "answer" in data, f"Missing 'answer' in query response: {data}"
        assert "sources" in data, f"Missing 'sources' in query response: {data}"
        assert isinstance(data["answer"], str)
        assert isinstance(data["sources"], list)
