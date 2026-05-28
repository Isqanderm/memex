"""
MCP Server для Memex.
Запуск: python mcp_server.py
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
import httpx
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp import types

BASE_URL = "http://localhost:8000"
server = Server("memex")


@server.list_tools()
async def list_tools() -> list[types.Tool]:
    return [
        types.Tool(
            name="add_document",
            description="Добавить документ в Memex для индексации. Принимает абсолютный путь к файлу.",
            inputSchema={
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Абсолютный путь к файлу (PDF, DOCX, MD, TXT)",
                    },
                },
                "required": ["file_path"],
            },
        ),
        types.Tool(
            name="query",
            description="Задать вопрос по проиндексированным документам. Возвращает ответ со ссылками на источники.",
            inputSchema={
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Вопрос на естественном языке (RU или EN)",
                    },
                },
                "required": ["query"],
            },
        ),
    ]


@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[types.TextContent]:
    async with httpx.AsyncClient(timeout=60.0) as client:
        if name == "add_document":
            file_path = arguments["file_path"]
            try:
                with open(file_path, "rb") as f:
                    filename = file_path.split("/")[-1]
                    response = await client.post(
                        f"{BASE_URL}/api/documents",
                        files={"file": (filename, f)},
                    )
                response.raise_for_status()
                data = response.json()
                status = data.get("status", "unknown")
                job_id = data.get("job_id", "")
                doc_id = data.get("doc_id", "")
                if status == "already_indexed":
                    text = f"Документ уже проиндексирован (doc_id: {doc_id})"
                elif status == "already_queued":
                    text = f"Документ уже в очереди индексации (job_id: {job_id})"
                else:
                    text = f"Документ добавлен в очередь. job_id: {job_id}\nПроверь статус: GET /api/jobs/{job_id}"
            except FileNotFoundError:
                text = f"Файл не найден: {file_path}"
            except httpx.HTTPStatusError as e:
                text = f"Ошибка сервера: {e.response.status_code}"
            return [types.TextContent(type="text", text=text)]

        elif name == "query":
            try:
                response = await client.post(
                    f"{BASE_URL}/api/query",
                    json={"query": arguments["query"]},
                )
                response.raise_for_status()
                data = response.json()
                answer = data.get("answer", "")
                sources = data.get("sources", [])
                sources_text = "\n".join(
                    f"[{s['index']}] {s.get('title', 'Unknown')} — {s.get('section', '')} (стр. {s.get('page', '?')})"
                    for s in sources
                )
                text = answer
                if sources_text:
                    text += f"\n\nИсточники:\n{sources_text}"
            except httpx.HTTPStatusError as e:
                text = f"Ошибка сервера: {e.response.status_code}"
            return [types.TextContent(type="text", text=text)]

    raise ValueError(f"Unknown tool: {name}")


async def main():
    async with stdio_server() as streams:
        await server.run(*streams, server.create_initialization_options())


if __name__ == "__main__":
    asyncio.run(main())
