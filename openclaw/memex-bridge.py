"""
OpenClaw wrapper for the Memex MCP bridge.

Drop this file into /home/node/.openclaw/ alongside shared/memex-bridge.py.

Prerequisites (install inside the openclaw-gateway container):
  python3 -m venv /home/node/.openclaw/.venv
  /home/node/.openclaw/.venv/bin/pip install mcp httpx

Add to ~/.openclaw/openclaw.json under "mcp" -> "servers":
  {
    "mcp": {
      "servers": {
        "memex": {
          "command": "/home/node/.openclaw/.venv/bin/python3",
          "args": ["/home/node/.openclaw/memex-bridge.py"],
          "env": { "MEMEX_URL": "http://memex:8000" }
        }
      }
    }
  }

Tools: context, remember, recall, observe, memories,
       index_file, check_indexing, list_memories, forget
"""
import os
import runpy

_dir = os.path.dirname(os.path.abspath(__file__))
runpy.run_path(os.path.join(_dir, "shared", "memex-bridge.py"), run_name="__main__")
