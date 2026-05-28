"""Entry point for MCP server."""
import sys
import asyncio
from pathlib import Path

# Добавляем корень проекта в путь
sys.path.insert(0, str(Path(__file__).parent))

from src.mcp.server import main

if __name__ == "__main__":
    asyncio.run(main())
