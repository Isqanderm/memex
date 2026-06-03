"""
Hermes wrapper for the Memex MCP bridge.

Drop this file into /opt/data/ on your Hermes host alongside shared/memex-bridge.py.

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
import os
import runpy

_dir = os.path.dirname(os.path.abspath(__file__))
runpy.run_path(os.path.join(_dir, "shared", "memex-bridge.py"), run_name="__main__")
