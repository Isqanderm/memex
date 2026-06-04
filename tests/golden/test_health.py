import httpx
import pytest


@pytest.mark.golden
@pytest.mark.unit
class TestHealth:
    def test_health_returns_200(self, client: httpx.Client) -> None:
        resp = client.get("/health")
        assert resp.status_code == 200

    def test_health_body_is_ok(self, client: httpx.Client) -> None:
        resp = client.get("/health")
        # Both versions return plain text "ok"
        assert resp.text.strip().lower() == "ok"
