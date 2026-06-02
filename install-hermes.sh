#!/bin/bash
# Memex × Hermes one-command installer
#
# Option A (recommended):
#   curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-hermes.sh -o install-hermes.sh
#   bash install-hermes.sh
#
# Option B (one-liner):
#   OPENAI_LLM_API_KEY=sk-... bash <(curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-hermes.sh)

set -euo pipefail

REPO_RAW="https://raw.githubusercontent.com/Isqanderm/memex/main"

# ── Overridable env vars ──────────────────────────────────────────────────────
HERMES_CONTAINER="${HERMES_CONTAINER:-}"        # override container auto-detect
HERMES_NETWORK="${HERMES_NETWORK:-}"            # override network auto-detect
INSTALL_DIR="${MEMEX_INSTALL_DIR:-/docker/memex}"

# ── Colours ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
ok()   { echo -e "${GREEN}✓${NC} $*"; }
warn() { echo -e "${YELLOW}⚠${NC}  $*"; }
die()  { echo -e "${RED}✗${NC} $*"; exit 1; }
step() { echo -e "\n${YELLOW}→${NC} $*"; }

echo "╔══════════════════════════════════════╗"
echo "║     Memex × Hermes Installer         ║"
echo "╚══════════════════════════════════════╝"

# ── 0. Dependency checks ──────────────────────────────────────────────────────
step "Checking dependencies..."

# Docker daemon
if ! docker info >/dev/null 2>&1; then
  die "Docker is not running or not accessible. Start Docker and try again."
fi
ok "Docker is running"

# Docker Compose 2.24+
if ! docker compose version >/dev/null 2>&1; then
  die "Docker Compose v2 not found. Install it and try again."
fi
COMPOSE_VERSION=$(docker compose version --short 2>/dev/null || docker compose version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
COMPOSE_MAJOR=$(echo "$COMPOSE_VERSION" | cut -d. -f1)
COMPOSE_MINOR=$(echo "$COMPOSE_VERSION" | cut -d. -f2)
if [ "$COMPOSE_MAJOR" -lt 2 ] || { [ "$COMPOSE_MAJOR" -eq 2 ] && [ "$COMPOSE_MINOR" -lt 24 ]; }; then
  die "Docker Compose 2.24+ required (found $COMPOSE_VERSION). Please upgrade."
fi
ok "Docker Compose $COMPOSE_VERSION"

# curl
if ! command -v curl >/dev/null 2>&1; then
  die "curl is required but not installed. Install it and try again."
fi
ok "curl available"

# ── 1. LLM API key ───────────────────────────────────────────────────────────
# Embeddings are local (sentence-transformers) — no OpenAI key needed for them.
if [ -z "${OPENAI_LLM_API_KEY:-}" ]; then
  read -rsp "OpenAI LLM API key (for GPT-4o; press Enter to use Claude instead): " OPENAI_LLM_API_KEY
  echo
fi
if [ -z "${OPENAI_LLM_API_KEY:-}" ] && [ -z "${ANTHROPIC_API_KEY:-}" ]; then
  die "Set OPENAI_LLM_API_KEY (OpenAI) or ANTHROPIC_API_KEY (Claude) for the LLM provider"
fi

# ── 2. Auto-detect Hermes ─────────────────────────────────────────────────────
step "Detecting Hermes..."

if [ -z "$HERMES_CONTAINER" ]; then
  # Try patterns in order of specificity
  HERMES_CONTAINER=$(docker ps --format '{{.Names}}' | grep -E '^hermes-agent$' | head -1 || true)
  if [ -z "$HERMES_CONTAINER" ]; then
    HERMES_CONTAINER=$(docker ps --format '{{.Names}}' | grep -E '^hermes_agent$' | head -1 || true)
  fi
  if [ -z "$HERMES_CONTAINER" ]; then
    HERMES_CONTAINER=$(docker ps --format '{{.Names}}' | grep -i 'hermes' | head -1 || true)
  fi
  if [ -z "$HERMES_CONTAINER" ]; then
    die "No Hermes container found. Is Hermes running? (docker ps)\nHint: set HERMES_CONTAINER=<name> to override."
  fi
fi
ok "Container: $HERMES_CONTAINER"

# Verify the container actually exists and is running
if ! docker inspect --format '{{.State.Running}}' "$HERMES_CONTAINER" 2>/dev/null | grep -q 'true'; then
  die "Container '$HERMES_CONTAINER' is not running. Start Hermes first."
fi

if [ -z "$HERMES_NETWORK" ]; then
  # Prefer a network with "hermes" in the name; fall back to first available
  ALL_NETWORKS=$(docker inspect "$HERMES_CONTAINER" \
    --format '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' \
    | tr ' ' '\n' | grep -v '^$' || true)
  HERMES_NETWORK=$(echo "$ALL_NETWORKS" | grep -i 'hermes' | head -1 || true)
  if [ -z "$HERMES_NETWORK" ]; then
    HERMES_NETWORK=$(echo "$ALL_NETWORKS" | head -1 || true)
  fi
  if [ -z "$HERMES_NETWORK" ]; then
    die "Could not detect Hermes Docker network.\nHint: set HERMES_NETWORK=<name> to override."
  fi
fi
ok "Network: $HERMES_NETWORK"

HERMES_CONFIG=$(docker exec "$HERMES_CONTAINER" \
  bash -c 'echo "${HERMES_CONFIG_FILE:-/opt/data/config.yaml}"' 2>/dev/null || echo "/opt/data/config.yaml")
ok "Config: $HERMES_CONFIG"

# ── 3. Check mcp + httpx in Hermes venv ──────────────────────────────────────
step "Checking Hermes Python environment..."
HERMES_PYTHON="/opt/hermes/.venv/bin/python3"
HERMES_PIP="/opt/hermes/.venv/bin/pip"

if ! docker exec "$HERMES_CONTAINER" "$HERMES_PYTHON" -c "import mcp, httpx" 2>/dev/null; then
  warn "mcp or httpx not found in Hermes venv — installing..."
  if ! docker exec "$HERMES_CONTAINER" "$HERMES_PIP" install mcp httpx -q; then
    die "Failed to install mcp/httpx. Run manually:\n  docker exec $HERMES_CONTAINER $HERMES_PIP install mcp httpx"
  fi
  # Re-verify
  docker exec "$HERMES_CONTAINER" "$HERMES_PYTHON" -c "import mcp, httpx" 2>/dev/null \
    || die "mcp/httpx still not importable after install. Check the Hermes venv."
fi
ok "mcp + httpx available"

# ── 4. Prepare install dir ────────────────────────────────────────────────────
step "Setting up Memex in $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
cd "$INSTALL_DIR"

if docker ps --format '{{.Names}}' | grep -q '^memex$'; then
  warn "Container 'memex' already running — skipping compose up"
else
  curl -sSf "$REPO_RAW/docker-compose.prod.yml"   -o docker-compose.prod.yml
  curl -sSf "$REPO_RAW/docker-compose.hermes.yml" -o docker-compose.hermes.yml
  ok "Compose files downloaded"

  if [ ! -f .env ]; then
    POSTGRES_PASSWORD=$(openssl rand -hex 16)
    # Determine LLM provider from available keys
    if [ -n "${OPENAI_LLM_API_KEY:-}" ]; then
      LLM_PROVIDER_VAL=openai
      LLM_MODEL_VAL=gpt-4o-mini
      LLM_KEY_LINE="OPENAI_LLM_API_KEY=${OPENAI_LLM_API_KEY}"
    else
      LLM_PROVIDER_VAL=claude
      LLM_MODEL_VAL=claude-haiku-4-5
      LLM_KEY_LINE="ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}"
    fi
    cat > .env <<EOF
POSTGRES_PASSWORD=${POSTGRES_PASSWORD}
DATABASE_URL=postgresql+asyncpg://memex:${POSTGRES_PASSWORD}@memex-db:5432/memex
LLM_PROVIDER=${LLM_PROVIDER_VAL}
LLM_MODEL=${LLM_MODEL_VAL}
${LLM_KEY_LINE}
UPLOAD_DIR=data/uploads
EOF
    chmod 600 .env
    ok ".env created (postgres password auto-generated)"
  else
    ok ".env already exists — skipping"
  fi

  step "Starting Memex containers..."
  HERMES_NETWORK="$HERMES_NETWORK" docker compose \
    -f docker-compose.prod.yml \
    -f docker-compose.hermes.yml \
    up -d
  ok "Containers started"
fi

# ── 5. Wait for Memex to be ready ────────────────────────────────────────────
step "Waiting for Memex to be ready..."
for i in $(seq 1 30); do
  if docker exec "$HERMES_CONTAINER" curl -sf http://memex:8000/api/documents >/dev/null 2>&1; then
    ok "Memex is up and reachable from Hermes"
    break
  fi
  [ "$i" -eq 30 ] && die "Memex did not become ready in time. Check: docker logs memex"
  sleep 2
done

# ── 6. Install MCP bridge ─────────────────────────────────────────────────────
step "Installing MCP bridge..."
docker exec "$HERMES_CONTAINER" curl -sSf \
  "$REPO_RAW/hermes/memex-bridge.py" -o /opt/data/memex-bridge.py
ok "Bridge installed at /opt/data/memex-bridge.py"

docker exec -u hermes "$HERMES_CONTAINER" bash -c \
  'printf "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n" \
   | /opt/hermes/.venv/bin/python3 /opt/data/memex-bridge.py 2>/dev/null' \
  | grep -q '"name":"memex"' && ok "MCP handshake OK" || die "MCP bridge test failed. Check /opt/data/memex-bridge.py"

# ── 7. Install skill ──────────────────────────────────────────────────────────
step "Installing Hermes skill..."
docker exec "$HERMES_CONTAINER" mkdir -p /opt/data/skills/memex
docker exec "$HERMES_CONTAINER" curl -sSf \
  "$REPO_RAW/hermes/memex/SKILL.md" -o /opt/data/skills/memex/SKILL.md
ok "Skill installed at /opt/data/skills/memex/SKILL.md"

# ── 8. Patch config.yaml ──────────────────────────────────────────────────────
step "Updating Hermes config ($HERMES_CONFIG)..."

print_manual_config() {
  warn "$1 Add the following to $HERMES_CONFIG manually:"
  printf '\n  mcp_servers:\n    memex:\n      command: /opt/hermes/.venv/bin/python3\n      args: [/opt/data/memex-bridge.py]\n      env:\n        MEMEX_URL: http://memex:8000\n      timeout: 120\n      connect_timeout: 60\n\n'
}

if ! docker exec "$HERMES_CONTAINER" "$HERMES_PYTHON" -c "import yaml" 2>/dev/null; then
  print_manual_config "PyYAML not found in Hermes venv — skipping automatic config patch."
elif ! docker exec "$HERMES_CONTAINER" "$HERMES_PYTHON" - "$HERMES_CONFIG" <<'PYEOF' 2>/dev/null; then
import sys, yaml
path = sys.argv[1]
try:
    cfg = yaml.safe_load(open(path)) or {}
except Exception as e:
    print(f"ERROR reading config: {e}", file=sys.stderr); sys.exit(1)
if 'mcp_servers' not in cfg:
    cfg['mcp_servers'] = {}
if 'memex' in cfg['mcp_servers']:
    print("memex already in config — skipping"); sys.exit(0)
cfg['mcp_servers']['memex'] = {
    'command': '/opt/hermes/.venv/bin/python3',
    'args': ['/opt/data/memex-bridge.py'],
    'env': {'MEMEX_URL': 'http://memex:8000'},
    'timeout': 120, 'connect_timeout': 60,
}
try:
    yaml.dump(cfg, open(path,'w'), default_flow_style=False, allow_unicode=True, sort_keys=False)
    print("config.yaml updated")
except Exception as e:
    print(f"ERROR writing config: {e}", file=sys.stderr); sys.exit(1)
PYEOF
  print_manual_config "Automatic config patch failed."
else
  ok "config.yaml patched"
fi

# ── 9. Restart Hermes ─────────────────────────────────────────────────────────
step "Restarting Hermes..."
docker restart "$HERMES_CONTAINER" >/dev/null
ok "Hermes restarting..."
sleep 20

# ── 10. Verify ────────────────────────────────────────────────────────────────
step "Verifying installation..."
API_KEY=$(docker exec "$HERMES_CONTAINER" env 2>/dev/null | grep API_SERVER_KEY | cut -d= -f2 || true)

if [ -n "$API_KEY" ]; then
  RESULT=$(docker exec "$HERMES_CONTAINER" curl -sf \
    -H "Authorization: Bearer $API_KEY" \
    http://localhost:8642/v1/chat/completions \
    -X POST -H "Content-Type: application/json" \
    -d '{"model":"hermes","messages":[{"role":"user","content":"list all mcp_memex_* tool names, one per line"}],"max_tokens":150,"stream":false}' \
    2>/dev/null | "$HERMES_PYTHON" -c \
    "import json,sys; print(json.load(sys.stdin)['choices'][0]['message']['content'])" 2>/dev/null || true)

  if echo "$RESULT" | grep -q "mcp_memex"; then
    echo ""
    echo -e "${GREEN}╔══════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║  ✅  Memex is live in Hermes!         ║${NC}"
    echo -e "${GREEN}╚══════════════════════════════════════╝${NC}"
    echo "$RESULT"
  else
    warn "Could not verify via API (tools may still be loading)"
    warn "Manual check: docker exec $HERMES_CONTAINER curl -s http://memex:8000/api/documents"
  fi
else
  warn "API_SERVER_KEY not found — skipping auto-verify"
  warn "Manual check: docker logs $HERMES_CONTAINER | grep memex"
fi

echo ""
echo "Installed in: $INSTALL_DIR"
echo "Bridge:       /opt/data/memex-bridge.py"
echo "Skill:        /opt/data/skills/memex/SKILL.md"
echo "Logs:         docker logs memex"
echo "              tail -f /opt/data/logs/mcp-stderr.log"
