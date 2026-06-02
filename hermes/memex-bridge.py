"""
Standalone MCP bridge for Memex — drop this file into /opt/data/ on your Hermes host.

Prerequisites (already available in the Hermes venv):
  /opt/hermes/.venv/bin/python3 -c "import mcp, httpx; print('OK')"

Add to ~/.hermes/config.yaml:
  mcp_servers:
    memex:
      command: /opt/hermes/.venv/bin/python3
      args:
        - /opt/data/memex-bridge.py
      env:
        MEMEX_URL: http://memex:8000

Tools: context, remember, recall, observe, memories,
       index_file, check_indexing, list_memories, forget
"""
import asyncio
import os
from pathlib import Path

import httpx
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp import types

BASE_URL = os.getenv("MEMEX_URL", "http://memex:8000")
server = Server("memex")


@server.list_tools()
async def list_tools() -> list[types.Tool]:
    return [
        types.Tool(
            name="context",
            description=(
                "Get the user's current profile (stable facts + recent activity). "
                "Call this as the FIRST tool at the start of every session."
            ),
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="remember",
            description=(
                "Save text as a memory. Extracts atomic facts via LLM, resolves conflicts "
                "with existing memories automatically. Returns immediately — no polling needed."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": "Text to remember"},
                },
                "required": ["content"],
            },
        ),
        types.Tool(
            name="recall",
            description=(
                "Search memories and documents by query. "
                "raw=false (default) — LLM answer. raw=true — raw chunks, faster/cheaper."
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
                    "category": {
                        "type": "string",
                        "enum": ["research", "reminder", "thought", "decision", "preference"],
                        "description": "Filter memories by category (optional)",
                    },
                },
                "required": ["query"],
            },
        ),
        types.Tool(
            name="observe",
            description=(
                "Extract facts from a conversation and save to memory. "
                "Call this as the LAST tool at the end of every session."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "conversation": {
                        "type": "string",
                        "description": "Full conversation history as text",
                    },
                },
                "required": ["conversation"],
            },
        ),
        types.Tool(
            name="memories",
            description="List all active memory facts about the user with content, source, and timestamps.",
            inputSchema={"type": "object", "properties": {}},
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
            description="Check file indexing status. Returns: pending / processing / done / error.",
            inputSchema={
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "job_id returned by index_file()",
                    },
                },
                "required": ["job_id"],
            },
        ),
        types.Tool(
            name="list_memories",
            description="List all indexed documents in the knowledge base: id, title, tags, date.",
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="forget",
            description="Delete a memory fact or document by id.",
            inputSchema={
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "Memory id (from memories) or document id (from list_memories)",
                    },
                },
                "required": ["doc_id"],
            },
        ),
    ]


@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[types.TextContent]:
    async with httpx.AsyncClient(timeout=60.0) as client:
        if name == "context":
            return await _context(client)
        elif name == "remember":
            return await _remember(client, arguments)
        elif name == "recall":
            return await _recall(client, arguments)
        elif name == "observe":
            return await _observe(client, arguments)
        elif name == "memories":
            return await _memories(client)
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


async def _context(client: httpx.AsyncClient) -> list[types.TextContent]:
    resp = await client.get(f"{BASE_URL}/api/memory/context")
    if resp.status_code != 200:
        return _text(f"Error: {resp.status_code}")
    data = resp.json()
    lines = []
    if data.get("static"):
        lines.append(f"User profile: {data['static']}")
    if data.get("dynamic"):
        lines.append(f"Recent context: {data['dynamic']}")
    if not lines:
        lines.append("No memories yet.")
    lines.append(f"(Total facts: {data.get('raw_count', 0)})")
    return _text("\n".join(lines))


async def _remember(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    content = args["content"]
    resp = await client.post(
        f"{BASE_URL}/api/memory/remember",
        json={"content": content, "source": "explicit"},
    )
    if resp.status_code != 200:
        return _text(f"Error: {resp.status_code}")
    data = resp.json()
    return _text(
        f"Remembered. Facts extracted: {data['facts_extracted']}, "
        f"memories updated: {data['memories_updated']}"
    )


async def _recall(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    query = args["query"]
    raw = args.get("raw", False)
    category = args.get("category")

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

    payload: dict = {"query": query}
    if category:
        payload["memory_category"] = category
    resp = await client.post(f"{BASE_URL}/api/query", json=payload)
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


async def _observe(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    conversation = args["conversation"]
    resp = await client.post(
        f"{BASE_URL}/api/memory/observe",
        json={"conversation": conversation},
    )
    if resp.status_code != 200:
        return _text(f"Error: {resp.status_code}")
    data = resp.json()
    return _text(
        f"Session observed. Facts extracted: {data['facts_extracted']}, "
        f"memories updated: {data['memories_updated']}"
    )


async def _memories(client: httpx.AsyncClient) -> list[types.TextContent]:
    resp = await client.get(f"{BASE_URL}/api/memory/list")
    if resp.status_code != 200:
        return _text(f"Error: {resp.status_code}")
    mems = resp.json()
    if not mems:
        return _text("No active memories.")
    lines = []
    for m in mems:
        rel = f" [{m['relation']}]" if m.get("relation") else ""
        cat = f" | {m['category']}" if m.get("category") else ""
        proj = f" | {m['project']}" if m.get("project") else ""
        date = (m.get("created_at") or "")[:10]
        lines.append(f"• {m['content']}{rel}\n  id: {m['id']}  |  {m['source']}{cat}{proj}  |  {date}")
    return _text(f"Active memories: {len(mems)}\n\n" + "\n\n".join(lines))


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
    # Try memory endpoint first, fall back to documents
    resp = await client.delete(f"{BASE_URL}/api/memory/{doc_id}")
    if resp.status_code in (200, 204):
        return _text(f"✓ Memory deleted (id: {doc_id})")
    if resp.status_code == 404:
        resp2 = await client.delete(f"{BASE_URL}/api/documents/{doc_id}")
        if resp2.status_code == 204:
            return _text(f"✓ Document deleted (doc_id: {doc_id})")
        return _text(f"Not found: {doc_id}")
    return _text(f"Error: {resp.status_code}")


async def main():
    async with stdio_server() as streams:
        await server.run(*streams, server.create_initialization_options())


if __name__ == "__main__":
    asyncio.run(main())
