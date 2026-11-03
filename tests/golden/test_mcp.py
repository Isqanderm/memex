# tests/golden/test_mcp.py
import os
import pytest

pytest.importorskip("mcp")
from mcp import ClientSession
from mcp.client.streamable_http import streamablehttp_client

EXPECTED_TOOLS = {
    "remember", "recall", "forget", "context", "observe", "memories",
    "index_text", "check_indexing", "list_documents", "delete_document",
}


@pytest.mark.golden
@pytest.mark.unit
class TestMcpContract:
    @pytest.mark.asyncio
    async def test_lists_expected_tools(self, base_url: str) -> None:
        async with streamablehttp_client(f"{base_url}/mcp") as (r, w, _):
            async with ClientSession(r, w) as s:
                await s.initialize()
                names = {t.name for t in (await s.list_tools()).tools}
        assert names == EXPECTED_TOOLS

    @pytest.mark.asyncio
    async def test_list_documents_tool_runs(self, base_url: str) -> None:
        async with streamablehttp_client(f"{base_url}/mcp") as (r, w, _):
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
