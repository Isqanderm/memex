# Hermes Integration

Connect Memex to a running Hermes agent as a persistent memory MCP server.

**What you get:** tools `mcp_memex_remember`, `mcp_memex_recall`, `mcp_memex_index_file`, `mcp_memex_check_indexing`, `mcp_memex_list_memories`, `mcp_memex_forget` available to your Hermes agent.

## Prerequisites

- Docker Compose **2.24+** — check with `docker compose version`
- Hermes running in a Docker network
- **OpenAI API key** — required for embeddings regardless of which LLM you use. Even if `LLM_PROVIDER=claude`, indexing will fail without `OPENAI_API_KEY`.

## Step 1 — Find your Hermes network name

```bash
docker network ls | grep hermes
# example output:  abc123  hermes_default  bridge  local
```

Copy the network name exactly — you'll need it in Step 3.

## Step 2 — Download and configure

```bash
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.prod.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.hermes.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/.env.example

cp .env.example .env
nano .env   # set OPENAI_API_KEY, POSTGRES_PASSWORD, and optionally ANTHROPIC_API_KEY
```

## Step 3 — Start Memex with Hermes overlay

```bash
HERMES_NETWORK=hermes_default \    # <-- replace with your network name from Step 1
  docker compose \
    -f docker-compose.prod.yml \
    -f docker-compose.hermes.yml \
  up -d
```

If `HERMES_NETWORK` is not set, the overlay will try to join a network named `hermes_default`.
If that network doesn't exist, Compose will error — check `docker network ls` again.

Memex is now reachable from Hermes as `http://memex:8000`. The Web UI and database are **not** exposed to the host.

## Step 4 — Install the MCP bridge

```bash
curl -o /opt/data/memex-bridge.py \
  https://raw.githubusercontent.com/Isqanderm/memex/main/hermes/memex-bridge.py
```

Verify the Hermes Python environment has the required packages:

```bash
docker exec hermes-agent \
  /opt/hermes/.venv/bin/python3 -c "import mcp, httpx; print('OK')"
```

## Step 5 — Install the skill

Hermes expects skills as `skills/<name>/SKILL.md`:

```bash
docker exec hermes-agent mkdir -p /opt/data/skills/memex
curl -o /opt/data/skills/memex/SKILL.md \
  https://raw.githubusercontent.com/Isqanderm/memex/main/hermes/memex-skill.md
```

## Step 6 — Add to Hermes config

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

## Step 7 — Restart Hermes

```bash
docker restart hermes-agent
```

**Wait 10–30 seconds** after restart — Hermes connects to MCP servers on startup and the first
connection takes time. Don't test in an existing chat session; open a new one or use the API
directly to verify tools are loaded:

```bash
docker exec hermes-agent curl -s \
  -H "Authorization: Bearer $API_SERVER_KEY" \
  http://localhost:8642/v1/chat/completions \
  -X POST -H "Content-Type: application/json" \
  -d '{"model":"hermes","messages":[{"role":"user","content":"list all mcp_memex_* tools"}],"max_tokens":200}'
```

## Verify

Test the MCP handshake directly:

```bash
docker exec -u hermes hermes-agent bash -c '
echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}" \
| /opt/hermes/.venv/bin/python3 /opt/data/memex-bridge.py
'
# Expected: JSON with "serverInfo": {"name": "memex", ...}
```

Check logs if something goes wrong:

```bash
# MCP stderr
tail -f /opt/data/logs/mcp-stderr.log

# Hermes connection errors
docker logs hermes-agent 2>&1 | grep memex
```

## Troubleshooting

| Error | Cause | Fix |
|---|---|---|
| `Permission denied: '/opt/data/...'` | Hermes `chown` still running at startup | Use `/opt/hermes/.venv/bin/python3` — it's outside the volume |
| `Connection refused` to `memex:8000` | Memex not in Hermes network | Verify `HERMES_NETWORK` matches `docker network ls` output exactly |
| Tools not visible after restart | Session cache is stale | Open a new chat session or use the API test above |
| `unhandled errors in a TaskGroup` | Bridge crashed | Check `mcp-stderr.log` |
| `ModuleNotFoundError: mcp` | Wrong Python interpreter | Use `/opt/hermes/.venv/bin/python3` |
| Skill not loading | Wrong directory structure | Path must be `/opt/data/skills/memex/SKILL.md`, not a flat `.md` file |
| `mcp_servers` block ignored | Nested under another key | Must be at the top level of `config.yaml` |
| `!reset` syntax error in compose | Docker Compose < 2.24 | Upgrade: `docker compose version` |
| Indexing fails, embedding error | Missing OpenAI key | `OPENAI_API_KEY` is required even when using Claude as LLM |
