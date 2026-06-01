"""
Standalone MCP bridge for Memex — for use with Claude Code.

Prerequisites:
  pip install mcp httpx

Add to .claude/settings.json:
  {
    "mcpServers": {
      "memex": {
        "command": "python3",
        "args": ["/path/to/memex-bridge.py"],
        "env": {"MEMEX_URL": "http://localhost:8000"}
      }
    }
  }

Or run the installer:
  bash <(curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-claude-code.sh)
"""
import asyncio
import os
import tempfile
from pathlib import Path

import httpx
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp import types

BASE_URL = os.getenv("MEMEX_URL", "http://localhost:8000")
server = Server("memex")

_pending_metadata: dict[str, dict] = {}


@server.list_tools()
async def list_tools() -> list[types.Tool]:
    return [
        types.Tool(
            name="remember",
            description=(
                "Save text as a memory in long-term storage. "
                "Indexing is async — use check_indexing(job_id) to confirm the memory is searchable."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": "Text to remember"},
                    "title": {"type": "string", "description": "Short label (optional)"},
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Classification tags, e.g. ['meeting', 'client']",
                    },
                },
                "required": ["content"],
            },
        ),
        types.Tool(
            name="recall",
            description=(
                "Find relevant memories by query. "
                "raw=false (default) — LLM synthesises an answer. "
                "raw=true — returns raw chunks, faster and cheaper."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Question or topic to search"},
                    "raw": {
                        "type": "boolean",
                        "description": "true — raw chunks; false — LLM answer (default)",
                        "default": False,
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Number of chunks (raw=true only, default 5)",
                        "default": 5,
                    },
                },
                "required": ["query"],
            },
        ),
        types.Tool(
            name="index_file",
            description=(
                "Index a file from disk (PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB). "
                "Async — use check_indexing(job_id) to confirm completion."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute path to file"},
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Tags (optional)",
                    },
                },
                "required": ["path"],
            },
        ),
        types.Tool(
            name="check_indexing",
            description="Check indexing status. Returns: pending / processing / done / error.",
            inputSchema={
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "job_id returned by remember() or index_file()",
                    },
                },
                "required": ["job_id"],
            },
        ),
        types.Tool(
            name="list_memories",
            description="List all documents in the knowledge base: id, title, tags, date.",
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="forget",
            description="Delete a memory by doc_id.",
            inputSchema={
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "Document ID (from list_memories or recall)",
                    },
                },
                "required": ["doc_id"],
            },
        ),
    ]


@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[types.TextContent]:
    async with httpx.AsyncClient(timeout=60.0) as client:
        if name == "remember":
            return await _remember(client, arguments)
        elif name == "recall":
            return await _recall(client, arguments)
        elif name == "index_file":
            return await _index_file(client, arguments)
        elif name == "check_indexing":
            return await _check_indexing(client, arguments)
        elif name == "list_memories":
            return await _list_memories(client)
        elif name == "forget":
            return await _forget(client, arguments)
        raise ValueError(f"Unknown tool: {name}")


def _text(s: str) -> list[types.TextContent]:
    return [types.TextContent(type="text", text=s)]


async def _remember(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    content = args["content"]
    title = args.get("title") or content[:60].replace("\n", " ")
    tags = args.get("tags", [])
    safe_title = "".join(c if c.isalnum() or c in "-_ " else "_" for c in title)[:80]

    with tempfile.NamedTemporaryFile(mode="w", suffix=f"-{safe_title}.txt", delete=False, encoding="utf-8") as f:
        f.write(content)
        tmp_path = f.name

    try:
        with open(tmp_path, "rb") as f:
            resp = await client.post(
                f"{BASE_URL}/api/documents",
                files={"file": (f"{safe_title}.txt", f, "text/plain")},
            )
        resp.raise_for_status()
        data = resp.json()
    finally:
        Path(tmp_path).unlink(missing_ok=True)

    status = data.get("status")
    job_id = data.get("job_id")
    doc_id = data.get("doc_id")

    if status == "already_indexed":
        return _text(f"Already indexed (doc_id: {doc_id})")

    if status in ("pending", "already_queued") and job_id:
        _pending_metadata[job_id] = {"title": title, "tags": tags}
        return _text(
            f"Queued for indexing.\njob_id: {job_id}\n"
            f"Use check_indexing('{job_id}') to confirm the memory is ready."
        )

    return _text(f"Unexpected response: {data}")


async def _recall(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    query = args["query"]
    raw = args.get("raw", False)

    if raw:
        top_k = args.get("top_k", 5)
        resp = await client.post(f"{BASE_URL}/api/search/chunks", json={"query": query, "top_k": top_k})
        if resp.status_code != 200:
            return _text(f"Search error: {resp.status_code}")
        chunks = resp.json().get("chunks", [])
        if not chunks:
            return _text("Nothing found.")
        lines = []
        for i, c in enumerate(chunks, 1):
            name = c.get("filename") or c.get("title") or "—"
            page = f" p.{c['page']}" if c.get("page") else ""
            section = f" / {c['section']}" if c.get("section") else ""
            lines.append(f"[{i}] {name}{section}{page}\n{c['text']}\n")
        return _text("\n".join(lines))

    resp = await client.post(f"{BASE_URL}/api/query", json={"query": query})
    if resp.status_code != 200:
        return _text(f"Query error: {resp.status_code}")
    data = resp.json()
    answer = data.get("answer", "")
    sources = data.get("sources", [])
    if sources:
        seen: set[str] = set()
        refs = []
        for s in sources:
            doc_id = s.get("doc_id", "")
            if doc_id in seen:
                continue
            seen.add(doc_id)
            name = s.get("filename") or s.get("title") or f"source {s.get('index', '?')}"
            page = f" · p. {s['page']}" if s.get("page") else ""
            refs.append(f"  • {name}{page}")
        answer += "\n\nSources:\n" + "\n".join(refs)
    return _text(answer)


async def _index_file(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    path = args["path"]
    tags = args.get("tags", [])
    try:
        with open(path, "rb") as f:
            filename = Path(path).name
            resp = await client.post(f"{BASE_URL}/api/documents", files={"file": (filename, f)})
        resp.raise_for_status()
        data = resp.json()
    except FileNotFoundError:
        return _text(f"File not found: {path}")
    except httpx.HTTPStatusError as e:
        return _text(f"Server error: {e.response.status_code}")

    status = data.get("status")
    job_id = data.get("job_id")
    doc_id = data.get("doc_id")

    if status == "already_indexed":
        return _text(f"Already indexed (doc_id: {doc_id})")

    if status in ("pending", "already_queued") and job_id:
        if tags:
            _pending_metadata[job_id] = {"tags": tags}
        return _text(
            f"File accepted: {filename}\njob_id: {job_id}\n"
            f"Use check_indexing('{job_id}') to confirm."
        )

    return _text(f"Unexpected response: {data}")


async def _check_indexing(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    job_id = args["job_id"]
    try:
        resp = await client.get(f"{BASE_URL}/api/jobs/{job_id}")
        resp.raise_for_status()
        job = resp.json()
    except httpx.HTTPStatusError as e:
        return _text(f"Error: {e.response.status_code}")

    status = job.get("status")
    doc_id = job.get("doc_id")

    if status == "done" and doc_id and job_id in _pending_metadata:
        meta = _pending_metadata.pop(job_id)
        patch: dict = {}
        if "title" in meta:
            patch["title"] = meta["title"]
        if "tags" in meta:
            patch["tags"] = meta["tags"]
        if patch:
            try:
                await client.patch(f"{BASE_URL}/api/documents/{doc_id}", json=patch)
            except Exception:
                pass

    if status == "done":
        return _text(f"✓ Done. doc_id: {doc_id}")
    elif status == "error":
        return _text(f"✕ Indexing error: {job.get('error', 'unknown')}")
    return _text(f"⟳ {status} — not ready yet, check again later.")


async def _list_memories(client: httpx.AsyncClient) -> list[types.TextContent]:
    resp = await client.get(f"{BASE_URL}/api/documents")
    if resp.status_code != 200:
        return _text(f"Error: {resp.status_code}")
    docs = resp.json()
    if not docs:
        return _text("Knowledge base is empty.")
    lines = []
    for d in docs:
        tags = d.get("tags", [])
        tags_str = f" [{', '.join(tags)}]" if tags else ""
        date = (d.get("indexed_at") or "")[:10]
        title = d.get("title") or "—"
        lines.append(f"• {title}{tags_str}\n  id: {d['id']}  |  {d['mime_type']}  |  {date}")
    return _text(f"Documents: {len(docs)}\n\n" + "\n\n".join(lines))


async def _forget(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    doc_id = args["doc_id"]
    resp = await client.delete(f"{BASE_URL}/api/documents/{doc_id}")
    if resp.status_code == 204:
        return _text(f"✓ Deleted (doc_id: {doc_id})")
    elif resp.status_code == 404:
        return _text(f"Document not found: {doc_id}")
    return _text(f"Error: {resp.status_code}")


async def main():
    async with stdio_server() as streams:
        await server.run(*streams, server.create_initialization_options())


if __name__ == "__main__":
    asyncio.run(main())
