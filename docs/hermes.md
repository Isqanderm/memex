# Hermes Integration

Connect Memex to a running [Hermes](https://github.com/your-hermes-repo) agent as a persistent memory MCP server.

**What you get:** tools `mcp_memex_remember`, `mcp_memex_recall`, `mcp_memex_index_file`, `mcp_memex_check_indexing`, `mcp_memex_list_memories`, `mcp_memex_forget` available to your Hermes agent.

## Prerequisites

- Docker Compose **2.24+** (check: `docker compose version`)
- Hermes running in a Docker network
- OpenAI API key (for embeddings)

## Step 1 — Find your Hermes network name

```bash
docker network ls | grep hermes
# example output: hermes_default
```

## Step 2 — Start Memex with Hermes overlay

```bash
# Download compose files
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.hermes.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/.env.example

# Configure
cp .env.example .env
nano .env   # fill in OPENAI_API_KEY and optionally ANTHROPIC_API_KEY

# Start (replace hermes_default with your actual network name)
HERMES_NETWORK=hermes_default \
  docker compose -f docker-compose.yml -f docker-compose.hermes.yml up -d
```

Memex is now reachable from Hermes as `http://memex:8000`. The Web UI and database are **not** exposed to the host.

## Step 3 — Install the MCP bridge

```bash
curl -o /opt/data/memex-bridge.py \
  https://raw.githubusercontent.com/Isqanderm/memex/main/hermes/memex-bridge.py
```

Verify the Hermes Python environment has the required packages:

```bash
docker exec hermes-agent \
  /opt/hermes/.venv/bin/python3 -c "import mcp, httpx; print('OK')"
```

## Step 4 — Add to Hermes config

Add this block to `~/.hermes/config.yaml` at the **top level** (not nested inside another section):

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

## Step 5 — Install the skill (optional but recommended)

```bash
curl -o /opt/data/skills/memex.md \
  https://raw.githubusercontent.com/Isqanderm/memex/main/hermes/memex-skill.md
```

## Step 6 — Restart Hermes

```bash
docker restart hermes-agent
```

## Verify

Check that the tools are visible:

```bash
# Test MCP handshake directly
docker exec -u hermes hermes-agent bash -c '
echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}" \
| /opt/hermes/.venv/bin/python3 /opt/data/memex-bridge.py
'
# Expected: JSON response with "serverInfo": {"name": "memex", ...}

# Check stderr logs if something goes wrong
tail -f /opt/data/logs/mcp-stderr.log
```

## Troubleshooting

| Error | Cause | Fix |
|---|---|---|
| `Permission denied: '/opt/data/...'` | Hermes `chown` still running at startup | Use `/opt/hermes/.venv/bin/python3` (not in volume) |
| `Connection refused` to `memex:8000` | Memex not in Hermes network | Check `HERMES_NETWORK` value matches `docker network ls` |
| `unhandled errors in a TaskGroup` | Bridge crashed silently | Check `mcp-stderr.log` |
| `ModuleNotFoundError: mcp` | Wrong Python interpreter | Use `/opt/hermes/.venv/bin/python3` |
| Tools not appearing after restart | `mcp_servers` nested under another key | Ensure it's at the top level of `config.yaml` |
| `!reset` syntax error | Docker Compose < 2.24 | Upgrade: `docker compose version` |
