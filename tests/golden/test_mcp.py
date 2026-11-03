# tests/golden/test_mcp.py
import asyncio
import re
import uuid

import pytest
from mcp import ClientSession
from mcp.client.streamable_http import streamable_http_client

EXPECTED_TOOLS = {
    "remember", "recall", "forget", "context", "observe", "memories",
    "index_text", "check_indexing", "list_documents", "delete_document",
}


@pytest.mark.golden
@pytest.mark.unit
class TestMcpContract:
    @pytest.mark.asyncio
    async def test_lists_expected_tools(self, base_url: str) -> None:
        async with streamable_http_client(f"{base_url}/mcp") as (r, w, _):
            async with ClientSession(r, w) as s:
                await s.initialize()
                names = {t.name for t in (await s.list_tools()).tools}
        assert names == EXPECTED_TOOLS

    @pytest.mark.asyncio
    async def test_list_documents_tool_runs(self, base_url: str) -> None:
        async with streamable_http_client(f"{base_url}/mcp") as (r, w, _):
            async with ClientSession(r, w) as s:
                await s.initialize()
                res = await s.call_tool("list_documents", {})
        assert not res.isError

    @pytest.mark.asyncio
    async def test_host_header_memex_is_accepted(self, base_url: str) -> None:
        import httpx
        body = {"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                           "clientInfo": {"name": "t", "version": "0"}}}
        r = httpx.post(f"{base_url}/mcp", json=body,
                       headers={"Host": "memex:8000", "Accept": "application/json, text/event-stream"})
        assert r.status_code == 200, r.text

    @pytest.mark.asyncio
    async def test_browser_origin_is_rejected(self, base_url: str) -> None:
        import httpx
        body = {"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                           "clientInfo": {"name": "t", "version": "0"}}}
        r = httpx.post(f"{base_url}/mcp", json=body,
                       headers={"Host": "memex:8000", "Origin": "http://evil.example",
                                "Accept": "application/json, text/event-stream"})
        assert r.status_code == 403, r.text


def _text(res) -> str:
    return "\n".join(c.text for c in res.content if getattr(c, "text", None))


@pytest.mark.golden
@pytest.mark.unit
class TestMcpRoundTrip:
    @pytest.mark.asyncio
    async def test_index_text_round_trip_keeps_title(self, base_url: str) -> None:
        title = "Round Trip Title"
        body = f"Round trip body text. {uuid.uuid4()}"
        async with streamable_http_client(f"{base_url}/mcp") as (r, w, _):
            async with ClientSession(r, w) as s:
                await s.initialize()
                res = await s.call_tool("index_text", {"content": body, "title": title})
                assert not res.isError, _text(res)
                m = re.search(r"job_id: (\S+)", _text(res))
                assert m, _text(res)
                job_id = m.group(1)

                doc_id = None
                for _ in range(30):
                    st = _text(await s.call_tool("check_indexing", {"job_id": job_id}))
                    d = re.search(r"Done\. doc_id: (\S+)", st)
                    if d:
                        doc_id = d.group(1)
                        break
                    assert not st.startswith("Error"), st
                    await asyncio.sleep(1)
                assert doc_id, "indexing did not finish in 30 s"

                listing = _text(await s.call_tool("list_documents", {}))
                assert title in listing

                first = await s.call_tool("delete_document", {"doc_id": doc_id})
                assert not first.isError, _text(first)
                second = await s.call_tool("delete_document", {"doc_id": doc_id})
                assert second.isError is True

    @pytest.mark.asyncio
    async def test_forget_unknown_id_is_an_error(self, base_url: str) -> None:
        async with streamable_http_client(f"{base_url}/mcp") as (r, w, _):
            async with ClientSession(r, w) as s:
                await s.initialize()
                res = await s.call_tool("forget", {"memory_id": "nonexistent-id"})
        assert res.isError is True
