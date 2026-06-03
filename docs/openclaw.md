# OpenClaw Integration

Connect Memex to a running OpenClaw gateway as a persistent memory MCP server.

**What you get:** tools `mcp_memex_context`, `mcp_memex_remember`, `mcp_memex_recall`, `mcp_memex_observe`, `mcp_memex_memories`, `mcp_memex_index_file`, `mcp_memex_check_indexing`, `mcp_memex_list_memories`, `mcp_memex_forget` available to your OpenClaw agent.

**Session protocol:** call `mcp_memex_context` at session start to inject user profile; call `mcp_memex_observe` at session end to extract new facts from the conversation.

## Prerequisites

- [ ] OpenClaw gateway running in Docker (`docker ps | grep openclaw`)
- [ ] Docker Compose **2.24+** — check: `docker compose version`
- [ ] LLM API key — OpenAI (`OPENAI_LLM_API_KEY`) or Anthropic (`ANTHROPIC_API_KEY`)
- [ ] Internet access on the server (to pull the image from GHCR)

## Quick Install (recommended)

```bash
# Download, inspect, then run:
curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-openclaw.sh -o install-openclaw.sh
cat install-openclaw.sh          # inspect before running
OPENAI_LLM_API_KEY=sk-... bash install-openclaw.sh
```

Or as a one-liner:

```bash
OPENAI_LLM_API_KEY=sk-... bash <(curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-openclaw.sh)
```

The script auto-detects your OpenClaw container and network, starts Memex with no external ports, installs the MCP bridge, patches `openclaw.json`, and restarts OpenClaw.

### If auto-detection fails

Override any value with environment variables:

```bash
OPENCLAW_CONTAINER=my-openclaw-gateway \
OPENCLAW_NETWORK=my_openclaw_net \
MEMEX_INSTALL_DIR=/opt/memex \
OPENAI_LLM_API_KEY=sk-... \
bash install-openclaw.sh
```

Find your values:
```bash
docker ps                          # → OPENCLAW_CONTAINER (copy the name)
docker network ls | grep openclaw  # → OPENCLAW_NETWORK (copy the name)
```

Skip to [Verify](#verify) after the script completes.

---

## Manual Setup

Use this if the script fails or you want full control.

### Step 1 — Find your OpenClaw network name

```bash
docker network ls | grep openclaw
# example output:  abc123  openclaw_default  bridge  local
```

Copy the full name — you will need it in Step 3.

### Step 2 — Download and configure

```bash
mkdir -p /docker/memex && cd /docker/memex
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.prod.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.openclaw.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/.env.example
cp .env.example .env
```

Edit `.env` — fill in at minimum:
```
OPENAI_LLM_API_KEY=sk-...
POSTGRES_PASSWORD=<generate with: openssl rand -hex 16>
LLM_PROVIDER=openai
LLM_MODEL=gpt-4o-mini
```

Secure the file:
```bash
chmod 600 .env
```

### Step 3 — Start Memex

```bash
OPENCLAW_NETWORK=openclaw_default \
  docker compose -f docker-compose.prod.yml -f docker-compose.openclaw.yml up -d
```

Memex is now reachable from OpenClaw as `http://memex:8000`. **No external ports are exposed.**

Verify from inside the OpenClaw container:
```bash
docker exec openclaw-gateway curl -s http://memex:8000/api/documents
# expected: [] or a list of documents
```

### Step 4 — Create a Python venv in the config volume

The venv lives inside the mounted config directory so it **persists across container restarts**:

```bash
docker exec openclaw-gateway python3 -m venv /home/node/.openclaw/.venv
docker exec openclaw-gateway /home/node/.openclaw/.venv/bin/pip install mcp httpx -q
```

### Step 5 — Install the MCP bridge

```bash
# Shared implementation
docker exec openclaw-gateway mkdir -p /home/node/.openclaw/shared
docker exec openclaw-gateway curl -sSf \
  https://raw.githubusercontent.com/Isqanderm/memex/main/shared/memex-bridge.py \
  -o /home/node/.openclaw/shared/memex-bridge.py

# OpenClaw wrapper
docker exec openclaw-gateway curl -sSf \
  https://raw.githubusercontent.com/Isqanderm/memex/main/openclaw/memex-bridge.py \
  -o /home/node/.openclaw/memex-bridge.py
```

Test the bridge:
```bash
docker exec openclaw-gateway bash -c '
printf "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n" \
| /home/node/.openclaw/.venv/bin/python3 /home/node/.openclaw/memex-bridge.py
'
# Expected: JSON with "serverInfo": {"name": "memex", ...}
```

### Step 6 — Add to OpenClaw config

Edit `~/.openclaw/openclaw.json` (or `/home/node/.openclaw/openclaw.json` inside the container). Add the `memex` block under `mcp` → `servers`:

```json
{
  "mcp": {
    "servers": {
      "memex": {
        "command": "/home/node/.openclaw/.venv/bin/python3",
        "args": ["/home/node/.openclaw/memex-bridge.py"],
        "env": {
          "MEMEX_URL": "http://memex:8000"
        }
      }
    }
  }
}
```

> **Common mistake:** accidentally creating a new top-level `"mcp"` key when one already exists — merge into the existing `"mcp"` object, don't duplicate it.

### Step 7 — Restart OpenClaw

```bash
docker restart openclaw-gateway
```

Wait 15–30 seconds — OpenClaw connects to MCP servers on startup.

## Verify

```bash
docker exec openclaw-gateway curl -s http://memex:8000/api/documents
# expected: [] or a list of documents
```

## Troubleshooting

| Error | Cause | Fix |
|---|---|---|
| `No OpenClaw container found` | Container name doesn't match `openclaw*` | Set `OPENCLAW_CONTAINER=<actual-name>` |
| `Docker Compose 2.24+ required` | Old Compose version | `apt upgrade docker-compose-plugin` or update manually |
| `python3 not found` | Python not in image | Set `OPENCLAW_IMAGE_PIP_PACKAGES="mcp httpx"` and rebuild |
| `Connection refused` to `memex:8000` | Memex not in OpenClaw network | Verify `OPENCLAW_NETWORK` matches `docker network ls` |
| `ModuleNotFoundError: mcp` | venv not set up | Run: `docker exec openclaw-gateway /home/node/.openclaw/.venv/bin/pip install mcp httpx` |
| `mcp/httpx lost after container restart` | Installed in system Python | Re-run Step 4 — venv in mounted volume persists |
| Tools not visible after restart | Stale session cache | Open a **new** chat session |
| `!reset` syntax error in compose | Docker Compose < 2.24 | Upgrade: `docker compose version` |
| Indexing fails, LLM error | Missing LLM key | Set `OPENAI_LLM_API_KEY` (OpenAI) or `ANTHROPIC_API_KEY` (Claude) |
