"""
MCP Server для Memex — персистентная память для AI-агентов.

Инструменты:
  remember(content)                 — сохранить текст как воспоминание (извлекает факты)
  recall(query, raw?)               — найти релевантные воспоминания
  context()                         — получить профиль пользователя (static + dynamic)
  observe(conversation)             — извлечь факты из разговора и сохранить
  memories()                        — список всех активных воспоминаний
  index_file(path)                  — проиндексировать файл с диска (async)
  check_indexing(job_id)            — проверить готовность индексации
  list_memories()                   — список всех документов в базе
  forget(doc_id)                    — удалить воспоминание

Подключение в Claude Code — добавить в .claude/settings.json:
{
  "mcpServers": {
    "memex": {
      "command": "python",
      "args": ["mcp_server.py"],
      "cwd": "/path/to/memex"
    }
  }
}
"""
import asyncio
import os
from pathlib import Path

import httpx
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp import types

BASE_URL = os.getenv("MEMEX_URL", "http://localhost:8000")
server = Server("memex")

# In-memory cache: job_id → {title, tags} — used to set metadata after async indexing
_pending_metadata: dict[str, dict] = {}


# ── Tool definitions ─────────────────────────────────────────────────────────

@server.list_tools()
async def list_tools() -> list[types.Tool]:
    return [
        types.Tool(
            name="remember",
            description="Save text as a memory — extracts atomic facts, resolves conflicts with existing memories.",
            inputSchema={
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "Текст для запоминания",
                    },
                },
                "required": ["content"],
            },
        ),
        types.Tool(
            name="recall",
            description=(
                "Найти релевантные воспоминания по запросу. "
                "raw=false (по умолчанию) — LLM синтезирует ответ. "
                "raw=true — возвращает сырые чанки без LLM, быстрее и дешевле."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Вопрос или тема для поиска",
                    },
                    "raw": {
                        "type": "boolean",
                        "description": "true — вернуть сырые чанки; false — LLM-синтез (default)",
                        "default": False,
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Кол-во чанков (только при raw=true, default 5)",
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
            name="context",
            description=(
                "Get the user's current profile as static (stable facts) and dynamic (recent activity). "
                "Call this as the FIRST tool at the start of every session."
            ),
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="observe",
            description=(
                "Extract facts from a conversation history and save to memory. "
                "Call this as the LAST tool at the end of every session, passing the full conversation."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "conversation": {
                        "type": "string",
                        "description": "Full conversation history as text",
                    }
                },
                "required": ["conversation"],
            },
        ),
        types.Tool(
            name="memories",
            description="List all active memories with their content, source, and timestamps.",
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="index_file",
            description=(
                "Проиндексировать файл с диска (PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB). "
                "Индексация асинхронная — используй check_indexing(job_id) для проверки."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Абсолютный путь к файлу",
                    },
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Теги (опционально)",
                    },
                },
                "required": ["path"],
            },
        ),
        types.Tool(
            name="check_indexing",
            description="Проверить статус индексации документа. Возвращает pending/processing/done/error.",
            inputSchema={
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "job_id полученный из remember() или index_file()",
                    },
                },
                "required": ["job_id"],
            },
        ),
        types.Tool(
            name="list_memories",
            description="Показать все документы в базе знаний: id, название, теги, дата.",
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="forget",
            description="Удалить воспоминание из базы знаний по doc_id.",
            inputSchema={
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "ID документа (из list_memories или recall)",
                    },
                },
                "required": ["doc_id"],
            },
        ),
    ]


# ── Tool implementations ─────────────────────────────────────────────────────

@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[types.TextContent]:
    async with httpx.AsyncClient(timeout=60.0) as client:
        if name == "remember":
            return await _remember(client, arguments)
        elif name == "recall":
            return await _recall(client, arguments)
        elif name == "context":
            return await _context(client)
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
        else:
            raise ValueError(f"Unknown tool: {name}")


def _text(s: str) -> list[types.TextContent]:
    return [types.TextContent(type="text", text=s)]


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
    lines.append(f"(Total memories: {data.get('raw_count', 0)})")
    return _text("\n".join(lines))


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
        date = (m.get("created_at") or "")[:10]
        lines.append(f"• {m['content']}{rel}\n  id: {m['id']}  |  {m['source']}  |  {date}")
    return _text(f"Active memories: {len(mems)}\n\n" + "\n\n".join(lines))


async def _recall(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    query = args["query"]
    raw = args.get("raw", False)
    category = args.get("category")

    if raw:
        top_k = args.get("top_k", 5)
        resp = await client.post(
            f"{BASE_URL}/api/search/chunks",
            json={"query": query, "top_k": top_k},
        )
        if resp.status_code != 200:
            return _text(f"Ошибка поиска: {resp.status_code}")
        chunks = resp.json().get("chunks", [])
        if not chunks:
            return _text("Ничего не найдено.")
        lines = []
        for i, c in enumerate(chunks, 1):
            name = c.get("filename") or c.get("title") or "—"
            page = f" стр.{c['page']}" if c.get("page") else ""
            section = f" / {c['section']}" if c.get("section") else ""
            lines.append(f"[{i}] {name}{section}{page}\n{c['text']}\n")
        return _text("\n".join(lines))
    else:
        payload: dict = {"query": query}
        if category:
            payload["memory_category"] = category
        resp = await client.post(
            f"{BASE_URL}/api/query",
            json=payload,
        )
        if resp.status_code != 200:
            return _text(f"Ошибка поиска: {resp.status_code}")
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
                name = s.get("filename") or s.get("title") or f"источник {s.get('index', '?')}"
                page = f" · стр. {s['page']}" if s.get("page") else ""
                refs.append(f"  • {name}{page}")
            answer += "\n\nИсточники:\n" + "\n".join(refs)
        return _text(answer)


async def _index_file(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    path = args["path"]
    tags = args.get("tags", [])
    try:
        with open(path, "rb") as f:
            filename = Path(path).name
            resp = await client.post(
                f"{BASE_URL}/api/documents",
                files={"file": (filename, f)},
            )
        resp.raise_for_status()
        data = resp.json()
    except FileNotFoundError:
        return _text(f"Файл не найден: {path}")
    except httpx.HTTPStatusError as e:
        return _text(f"Ошибка сервера: {e.response.status_code}")

    status = data.get("status")
    job_id = data.get("job_id")
    doc_id = data.get("doc_id")

    if status == "already_indexed":
        return _text(f"Уже проиндексирован (doc_id: {doc_id})")

    if status in ("pending", "already_queued") and job_id:
        if tags:
            _pending_metadata[job_id] = {"tags": tags}
        return _text(
            f"Файл принят: {filename}\n"
            f"job_id: {job_id}\n"
            f"Используй check_indexing('{job_id}') для проверки готовности."
        )

    return _text(f"Неожиданный ответ: {data}")


async def _check_indexing(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    job_id = args["job_id"]
    try:
        resp = await client.get(f"{BASE_URL}/api/jobs/{job_id}")
        resp.raise_for_status()
        job = resp.json()
    except httpx.HTTPStatusError as e:
        return _text(f"Ошибка: {e.response.status_code}")

    status = job.get("status")
    doc_id = job.get("doc_id")

    # On first "done", apply cached title/tags
    if status == "done" and doc_id and job_id in _pending_metadata:
        meta = _pending_metadata.pop(job_id)
        patch_body: dict = {}
        if "title" in meta:
            patch_body["title"] = meta["title"]
        if "tags" in meta:
            patch_body["tags"] = meta["tags"]
        if patch_body:
            try:
                await client.patch(
                    f"{BASE_URL}/api/documents/{doc_id}",
                    json=patch_body,
                )
            except Exception:
                pass  # metadata update is best-effort

    if status == "done":
        return _text(f"✓ Готово. doc_id: {doc_id}")
    elif status == "error":
        return _text(f"✕ Ошибка индексации: {job.get('error', 'неизвестно')}")
    else:
        return _text(f"⟳ {status} — ещё не готово, проверь позже.")


async def _list_memories(client: httpx.AsyncClient) -> list[types.TextContent]:
    resp = await client.get(f"{BASE_URL}/api/documents")
    if resp.status_code != 200:
        return _text(f"Ошибка: {resp.status_code}")
    docs = resp.json()
    if not docs:
        return _text("База знаний пуста.")
    lines = []
    for d in docs:
        tags = d.get("tags", [])
        tags_str = f" [{', '.join(tags)}]" if tags else ""
        date = (d.get("indexed_at") or "")[:10]
        title = d.get("title") or "—"
        lines.append(f"• {title}{tags_str}\n  id: {d['id']}  |  {d['mime_type']}  |  {date}")
    return _text(f"Документов в базе: {len(docs)}\n\n" + "\n\n".join(lines))


async def _forget(client: httpx.AsyncClient, args: dict) -> list[types.TextContent]:
    doc_id = args["doc_id"]
    # Try memory endpoint first
    resp = await client.delete(f"{BASE_URL}/api/memory/{doc_id}")
    if resp.status_code in (200, 204):
        return _text(f"✓ Memory deleted (id: {doc_id})")
    if resp.status_code == 404:
        # Fall back to document endpoint
        resp2 = await client.delete(f"{BASE_URL}/api/documents/{doc_id}")
        if resp2.status_code == 204:
            return _text(f"✓ Document deleted (doc_id: {doc_id})")
        return _text(f"Not found: {doc_id}")
    return _text(f"Error: {resp.status_code}")


# ── Entry point ──────────────────────────────────────────────────────────────

async def main():
    async with stdio_server() as streams:
        await server.run(*streams, server.create_initialization_options())
