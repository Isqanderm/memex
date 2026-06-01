# Claude Code Integration

Connect Memex to Claude Code as a persistent memory MCP server.

**What you get:** tools `remember`, `recall`, `index_file`, `check_indexing`, `list_memories`, `forget` available in every Claude Code session.

## Quick Install (one command)

```bash
bash <(curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-claude-code.sh)
```

The script:
1. Starts Memex via Docker on `http://localhost:8000`
2. Installs the MCP bridge to `~/.local/share/memex/memex-bridge.py`
3. Patches `.claude/settings.json` with the MCP server config
4. Tests the MCP handshake

**Restart Claude Code after** to load the MCP server.

---

## Manual Setup

### Prerequisites

- Docker Compose 2.24+ — check with `docker compose version`
- Python 3.10+ with `mcp` and `httpx`: `pip install mcp httpx`
- **OpenAI API key** — required for embeddings regardless of which LLM you use

### Step 1 — Start Memex

```bash
mkdir -p ~/.local/share/memex && cd ~/.local/share/memex
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.prod.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/.env.example
cp .env.example .env   # fill in OPENAI_API_KEY and POSTGRES_PASSWORD
docker compose -f docker-compose.prod.yml up -d
# → http://localhost:8000
```

### Step 2 — Install MCP bridge

```bash
curl -o ~/.local/share/memex/memex-bridge.py \
  https://raw.githubusercontent.com/Isqanderm/memex/main/claude-code/memex-bridge.py
```

### Step 3 — Configure Claude Code

Add to `~/.claude/settings.json` (global) or `.claude/settings.json` (project):

```json
{
  "mcpServers": {
    "memex": {
      "command": "python3",
      "args": ["~/.local/share/memex/memex-bridge.py"],
      "env": {
        "MEMEX_URL": "http://localhost:8000"
      }
    }
  }
}
```

### Step 4 — Restart Claude Code

Restart to load the MCP server. Verify tools are available:

```
/mcp
```

You should see `memex` listed with its tools.

## Usage

```
remember this meeting: discussed Q3 roadmap with team
→ Queued. Use check_indexing('<job_id>') to confirm.

recall what did we discuss about Q3?
→ Answer with sources.

list_memories
→ All documents in the knowledge base.
```

## Troubleshooting

| Error | Cause | Fix |
|---|---|---|
| Tools not visible after restart | Settings not reloaded | Fully quit and relaunch Claude Code |
| `Connection refused` to `localhost:8000` | Memex not running | `docker compose -f docker-compose.prod.yml up -d` |
| `ModuleNotFoundError: mcp` | Missing package | `pip install mcp httpx` |
| `Permission denied` on bridge file | Wrong permissions | `chmod +x ~/.local/share/memex/memex-bridge.py` |
| Indexing error on remember | Missing OpenAI key | Set `OPENAI_API_KEY` in `.env` and restart Memex |
