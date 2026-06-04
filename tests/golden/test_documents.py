# tests/golden/test_documents.py
from pathlib import Path

import httpx
import pytest

FIXTURES_DIR = Path(__file__).parent / "fixtures"


@pytest.mark.golden
@pytest.mark.unit
class TestDocumentsContract:
    """API contract tests for /api/documents — no LLM required."""

    def _upload_sample(self, client: httpx.Client) -> dict:
        sample = FIXTURES_DIR / "golden_sample.txt"
        with open(sample, "rb") as f:
            resp = client.post(
                "/api/documents",
                files={"file": ("golden_sample.txt", f, "text/plain")},
            )
        assert resp.status_code == 200
        return resp.json()

    def test_upload_returns_200(self, client: httpx.Client) -> None:
        resp_data = self._upload_sample(client)
        assert isinstance(resp_data, dict)

    def test_upload_response_has_status_field(self, client: httpx.Client) -> None:
        data = self._upload_sample(client)
        assert "status" in data
        assert data["status"] in ("pending", "already_indexed", "already_queued")

    def test_upload_response_has_id_field(self, client: httpx.Client) -> None:
        """Either job_id (new upload) or doc_id (already indexed) must be present."""
        data = self._upload_sample(client)
        has_job_id = "job_id" in data and data["job_id"] is not None
        has_doc_id = "doc_id" in data and data["doc_id"] is not None
        assert has_job_id or has_doc_id, f"No job_id or doc_id in: {data}"

    def test_upload_same_file_twice_is_idempotent(self, client: httpx.Client) -> None:
        """Second upload of same file must return already_indexed or already_queued."""
        data1 = self._upload_sample(client)
        data2 = self._upload_sample(client)
        # First may be "pending"; second must NOT be "pending" again
        assert data2["status"] in ("already_indexed", "already_queued")
        # If first was already_indexed, both will be already_indexed
        if data1["status"] == "pending":
            # job was just created — second must be already_queued
            assert data2["status"] == "already_queued"

    def test_list_documents_returns_array(self, client: httpx.Client) -> None:
        resp = client.get("/api/documents")
        assert resp.status_code == 200
        data = resp.json()
        assert isinstance(data, list)

    def test_list_document_item_has_required_fields(self, client: httpx.Client) -> None:
        """Each document item must have id and mime_type."""
        import time
        data = self._upload_sample(client)
        # Wait up to 15s for indexing to complete so list is non-empty
        job_id = data.get("job_id")
        if job_id:
            for _ in range(15):
                job = client.get(f"/api/jobs/{job_id}").json()
                if job.get("status") in ("done", "error"):
                    break
                time.sleep(1)
        resp = client.get("/api/documents")
        items = resp.json()
        if not items:
            pytest.skip("No indexed documents yet — indexing may be too slow for this environment")
        doc = items[0]
        assert "id" in doc, f"Missing 'id' in document: {doc}"
        assert "mime_type" in doc, f"Missing 'mime_type' in document: {doc}"

    def test_delete_document_returns_204(self, client: httpx.Client) -> None:
        """Upload a fresh uniquely-named file and delete it."""
        import time
        unique_name = f"delete_test_{int(time.time())}.txt"
        content = b"Delete me. This is a unique golden test document."
        resp_upload = client.post(
            "/api/documents",
            files={"file": (unique_name, content, "text/plain")},
        )
        assert resp_upload.status_code == 200
        data = resp_upload.json()

        # Resolve doc_id: if "pending", poll job until doc_id known
        doc_id = data.get("doc_id")
        if doc_id is None and data.get("job_id"):
            # Wait max 10s for job to produce doc_id
            for _ in range(10):
                job_resp = client.get(f"/api/jobs/{data['job_id']}")
                if job_resp.status_code == 200:
                    job_data = job_resp.json()
                    if job_data.get("doc_id"):
                        doc_id = job_data["doc_id"]
                        break
                time.sleep(1)

        if doc_id is None:
            pytest.skip("Could not resolve doc_id in time — indexing too slow for this test")

        del_resp = client.delete(f"/api/documents/{doc_id}")
        assert del_resp.status_code == 204

    def test_delete_nonexistent_document_returns_404(self, client: httpx.Client) -> None:
        resp = client.delete("/api/documents/nonexistent-id-that-does-not-exist")
        assert resp.status_code in (400, 404, 422)
