# Hermes Integration

Connect Memex to a running Hermes agent as a persistent memory MCP server.

**What you get:** tools `mcp_memex_remember`, `mcp_memex_recall`, `mcp_memex_index_file`, `mcp_memex_check_indexing`, `mcp_memex_list_memories`, `mcp_memex_forget` available to your Hermes agent.

## Prerequisites

- [ ] Hermes agent running in Docker (`docker ps | grep hermes`)
- [ ] Docker Compose **2.24+** — check: `docker compose version`
- [ ] OpenAI API key — required for embeddings even if you use Claude as your LLM
- [ ] Internet access on the server (to pull the image from GHCR)

## Quick Install (recommended)

```bash
# Download, inspect, then run:
curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-hermes.sh -o install-hermes.sh
cat install-hermes.sh          # inspect before running
OPENAI_API_KEY=sk-... bash install-hermes.sh
```

Or as a one-liner (requires bash with process substitution):

```bash
OPENAI_API_KEY=sk-... bash <(curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-hermes.sh)
```

The script auto-detects your Hermes container and network, starts Memex with no external ports, installs the MCP bridge and skill, patches `config.yaml`, and restarts Hermes.

### If auto-detection fails

Override any value with environment variables:

```bash
HERMES_CONTAINER=my-hermes \
HERMES_NETWORK=my_hermes_net \
MEMEX_INSTALL_DIR=/opt/memex \
OPENAI_API_KEY=sk-... \
bash install-hermes.sh
```

Find your values:
```bash
docker ps                        # → HERMES_CONTAINER (copy the name)
docker network ls | grep hermes  # → HERMES_NETWORK (copy the name)
```

Skip to [Verify](#verify) after the script completes.

---

## Manual Setup

Use this if the script fails or you want full control.

### Step 1 — Find your Hermes network name

```bash
docker network ls | grep hermes
# example output:  abc123  hermes-pioneer_default  bridge  local
```

Copy the full name — you will need it in Step 3.

### Step 2 — Download and configure

```bash
mkdir -p /docker/memex && cd /docker/memex
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.prod.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.hermes.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/.env.example
cp .env.example .env
```

Edit `.env` — fill in at minimum:
```
OPENAI_API_KEY=sk-...
OPENAI_LLM_API_KEY=sk-...   # same key
POSTGRES_PASSWORD=<generate with: openssl rand -hex 16>
```

Secure the file:
```bash
chmod 600 .env
```

### Step 3 — Start Memex

```bash
HERMES_NETWORK=hermes-pioneer_default \
  docker compose -f docker-compose.prod.yml -f docker-compose.hermes.yml up -d
```

Memex is now reachable from Hermes as `http://memex:8000`. **No external ports are exposed.**

Verify from inside the Hermes container:
```bash
docker exec hermes-agent curl -s http://memex:8000/api/documents
# expected: [] or a list of documents
```

### Step 4 — Install the MCP bridge

```bash
docker exec hermes-agent curl -sSf \
  https://raw.githubusercontent.com/Isqanderm/memex/main/hermes/memex-bridge.py \
  -o /opt/data/memex-bridge.py
```

Verify the Hermes Python environment:
```bash
docker exec hermes-agent /opt/hermes/.venv/bin/python3 -c "import mcp, httpx; print('OK')"
# if not OK:
docker exec hermes-agent /opt/hermes/.venv/bin/pip install mcp httpx -q
```

### Step 5 — Install the skill

```bash
docker exec hermes-agent mkdir -p /opt/data/skills/memex
docker exec hermes-agent curl -sSf \
  https://raw.githubusercontent.com/Isqanderm/memex/main/hermes/memex/SKILL.md \
  -o /opt/data/skills/memex/SKILL.md
```

### Step 6 — Add to Hermes config

Edit `~/.hermes/config.yaml` (or `/opt/data/config.yaml` inside the container). Add the `memex` block at the **top level** under `mcp_servers:`:

```yaml
mcp_servers:
  memex:
    command: /opt/hermes/.venv/bin/python3
    args:
      - /opt/data/memex-bridge.py
    env:
      MEMEX_URL: http://memex:8000
    timeout: 120
    connect_timeout: 60
```

> **Common mistake:** accidentally nesting `memex:` inside another section (e.g. `web:`) due to wrong indentation. Double-check the YAML level.

### Step 7 — Restart Hermes

```bash
docker restart hermes-agent
```

Wait 15–30 seconds — Hermes connects to MCP servers on startup.

## Verify

Test the MCP handshake:
```bash
docker exec -u hermes hermes-agent bash -c '
echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}" \
| /opt/hermes/.venv/bin/python3 /opt/data/memex-bridge.py
'
# Expected: JSON with "serverInfo": {"name": "memex", ...}
```

Test that Hermes sees the tools (needs `API_SERVER_KEY` env var):
```bash
docker exec hermes-agent curl -s \
  -H "Authorization: Bearer $API_SERVER_KEY" \
  http://localhost:8642/v1/chat/completions \
  -X POST -H "Content-Type: application/json" \
  -d '{"model":"hermes","messages":[{"role":"user","content":"list all mcp_memex_* tools"}],"max_tokens":200}'
```

## Troubleshooting

| Error | Cause | Fix |
|---|---|---|
| `No Hermes container found` | Container name doesn't match `hermes*` | Set `HERMES_CONTAINER=<actual-name>` |
| `Docker Compose 2.24+ required` | Old Compose version | `apt upgrade docker-compose-plugin` or update manually |
| `Permission denied: '/opt/data/...'` | Hermes `chown` running at startup | Use `/opt/hermes/.venv/bin/python3` (it's outside the volume) |
| `Connection refused` to `memex:8000` | Memex not in Hermes network | Verify `HERMES_NETWORK` matches `docker network ls` |
| `ModuleNotFoundError: mcp` | Wrong Python interpreter | Use `/opt/hermes/.venv/bin/python3` |
| Tools not visible after restart | Stale session cache | Open a **new** chat session or use the API test above |
| `mcp_servers` block ignored | Nested under another key | Must be at top level of `config.yaml` |
| `!reset` syntax error in compose | Docker Compose < 2.24 | Upgrade: `docker compose version` |
| Indexing fails, embedding error | Missing OpenAI key | `OPENAI_API_KEY` is required even when using Claude as LLM |
| `PyYAML not found` (installer) | PyYAML missing from Hermes venv | Installer prints the YAML snippet — add it manually |
