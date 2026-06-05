# tests/golden/test_memory.py
import httpx
import pytest


def _assert_context_shape(data: dict) -> None:
    assert "raw_count" in data, f"Missing 'raw_count' in context: {data}"
    assert "static" in data, f"Missing 'static' in context: {data}"
    assert "dynamic" in data, f"Missing 'dynamic' in context: {data}"


def _assert_delete_memory_ok(resp: httpx.Response) -> None:
    assert resp.status_code == 200, f"Unexpected delete status: {resp.status_code}"
    assert resp.json().get("status") == "deleted", f"Expected status=deleted, got: {resp.json()}"


@pytest.mark.golden
@pytest.mark.unit
class TestMemoryContract:
    """API contract tests for /api/memory/* — requires LLM only for remember/observe."""

    def test_memory_list_returns_array(self, client: httpx.Client) -> None:
        resp = client.get("/api/memory/list")
        assert resp.status_code == 200
        data = resp.json()
        assert isinstance(data, list)

    def test_memory_list_with_category_filter(self, client: httpx.Client) -> None:
        resp = client.get("/api/memory/list", params={"category": "insight"})
        assert resp.status_code == 200
        data = resp.json()
        assert isinstance(data, list)

    def test_memory_context_returns_200(self, client: httpx.Client) -> None:
        resp = client.get("/api/memory/context")
        assert resp.status_code == 200

    def test_memory_context_shape(self, client: httpx.Client) -> None:
        data = client.get("/api/memory/context").json()
        _assert_context_shape(data)

    def test_memory_context_raw_count_is_int(self, client: httpx.Client) -> None:
        data = client.get("/api/memory/context").json()
        assert isinstance(data["raw_count"], int)
        assert data["raw_count"] >= 0

    @pytest.mark.e2e
    def test_remember_returns_expected_shape(self, client: httpx.Client) -> None:
        """Requires LLM. Tests that remember returns facts_extracted and memories_updated."""
        resp = client.post(
            "/api/memory/remember",
            json={"content": "Alex prefers dark mode in all his editors.", "source": "explicit"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert "facts_extracted" in data, f"Missing facts_extracted: {data}"
        assert "memories_updated" in data, f"Missing memories_updated: {data}"
        assert isinstance(data["facts_extracted"], int)
        assert isinstance(data["memories_updated"], int)

    @pytest.mark.e2e
    def test_remember_then_list_shows_memory(self, client: httpx.Client) -> None:
        """Requires LLM. After remember, the memory must appear in list."""
        client.post(
            "/api/memory/remember",
            json={"content": "The team uses Rust for performance-critical services.", "source": "explicit"},
        )
        resp = client.get("/api/memory/list")
        assert resp.status_code == 200
        items = resp.json()
        assert len(items) > 0, "Expected at least one memory after remember"
        # Each item must have required fields
        item = items[0]
        assert "id" in item, f"Missing 'id' in memory item: {item}"
        assert "content" in item, f"Missing 'content' in memory item: {item}"

    @pytest.mark.e2e
    def test_delete_memory_removes_it(self, client: httpx.Client) -> None:
        """Requires LLM. Remember → list → delete → verify gone."""
        client.post(
            "/api/memory/remember",
            json={"content": "Temporary fact for deletion test.", "source": "explicit"},
        )
        items = client.get("/api/memory/list").json()
        assert items, "No memories to delete"
        memory_id = items[0]["id"]

        del_resp = client.delete(f"/api/memory/{memory_id}")
        _assert_delete_memory_ok(del_resp)

        # Verify it's gone
        items_after = client.get("/api/memory/list").json()
        ids_after = {m["id"] for m in items_after}
        assert memory_id not in ids_after, f"Memory {memory_id} still present after delete"

    def test_delete_nonexistent_memory_returns_404(self, client: httpx.Client) -> None:
        resp = client.delete("/api/memory/nonexistent-id-00000000")
        assert resp.status_code in (400, 404, 422)
