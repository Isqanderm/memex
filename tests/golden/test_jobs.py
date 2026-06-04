# tests/golden/test_jobs.py
import time
from pathlib import Path

import httpx
import pytest

FIXTURES_DIR = Path(__file__).parent / "fixtures"


@pytest.mark.unit
class TestJobsContract:
    """API contract tests for /api/jobs/:id — no LLM required."""

    def _get_job_id(self, client: httpx.Client) -> str:
        """Upload sample file and return its job_id (upload must be 'pending')."""
        unique_name = f"jobs_test_{int(time.time())}.txt"
        content = b"Jobs contract test document. Unique content for regression suite."
        resp = client.post(
            "/api/documents",
            files={"file": (unique_name, content, "text/plain")},
        )
        assert resp.status_code == 200
        data = resp.json()
        job_id = data.get("job_id")
        if not job_id:
            pytest.skip("Upload returned already_indexed — no new job_id available")
        return job_id

    def test_job_status_returns_200(self, client: httpx.Client) -> None:
        job_id = self._get_job_id(client)
        resp = client.get(f"/api/jobs/{job_id}")
        assert resp.status_code == 200

    def test_job_response_has_job_id_field(self, client: httpx.Client) -> None:
        job_id = self._get_job_id(client)
        data = client.get(f"/api/jobs/{job_id}").json()
        assert "job_id" in data
        assert data["job_id"] == job_id

    def test_job_response_has_status_field(self, client: httpx.Client) -> None:
        job_id = self._get_job_id(client)
        data = client.get(f"/api/jobs/{job_id}").json()
        assert "status" in data
        assert data["status"] in ("pending", "processing", "done", "error")

    def test_job_status_transitions_to_done_or_error(self, client: httpx.Client) -> None:
        """Within 30 seconds the job must reach terminal state."""
        job_id = self._get_job_id(client)
        terminal = {"done", "error"}
        for _ in range(30):
            data = client.get(f"/api/jobs/{job_id}").json()
            if data["status"] in terminal:
                break
            time.sleep(1)
        else:
            pytest.fail(f"Job {job_id} did not reach terminal state in 30s, last status: {data['status']}")
        assert data["status"] in terminal

    def test_nonexistent_job_returns_404(self, client: httpx.Client) -> None:
        resp = client.get("/api/jobs/nonexistent-job-that-does-not-exist")
        assert resp.status_code == 404
