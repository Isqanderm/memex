#!/bin/bash
# Memex × Hermes one-command installer
# Usage:
#   OPENAI_API_KEY=sk-... bash install-hermes.sh
#   or:
#   bash install-hermes.sh  (will prompt for key)

set -euo pipefail

REPO_RAW="https://raw.githubusercontent.com/Isqanderm/memex/main"
INSTALL_DIR="${MEMEX_INSTALL_DIR:-/docker/memex}"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
ok()   { echo -e "${GREEN}✓${NC} $*"; }
warn() { echo -e "${YELLOW}⚠${NC}  $*"; }
die()  { echo -e "${RED}✗${NC} $*"; exit 1; }
step() { echo -e "\n${YELLOW}→${NC} $*"; }

echo "╔══════════════════════════════════════╗"
echo "║     Memex × Hermes Installer         ║"
echo "╚══════════════════════════════════════╝"

# ── 1. OpenAI API key ──────────────────────────────────────────────────────
if [ -z "${OPENAI_API_KEY:-}" ]; then
  read -rsp "OpenAI API key (required for embeddings): " OPENAI_API_KEY
  echo
fi
[ -z "${OPENAI_API_KEY:-}" ] && die "OPENAI_API_KEY is required (used for embeddings even if LLM_PROVIDER=claude)"

# ── 2. Auto-detect Hermes ─────────────────────────────────────────────────
step "Detecting Hermes..."
HERMES_CONTAINER=$(docker ps --format '{{.Names}}' | grep -E 'hermes.?agent' | head -1 || true)
[ -z "$HERMES_CONTAINER" ] && die "hermes-agent container not found. Is Hermes running? (docker ps)"
ok "Container: $HERMES_CONTAINER"

HERMES_NETWORK=$(docker inspect "$HERMES_CONTAINER" \
  --format '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' \
  | tr ' ' '\n' | grep -v '^$' | head -1)
[ -z "$HERMES_NETWORK" ] && die "Could not detect Hermes Docker network"
ok "Network: $HERMES_NETWORK"

HERMES_CONFIG=$(docker exec "$HERMES_CONTAINER" \
  bash -c 'echo "${HERMES_CONFIG_FILE:-/opt/data/config.yaml}"' 2>/dev/null || echo "/opt/data/config.yaml")
ok "Config: $HERMES_CONFIG"

# ── 3. Check mcp + httpx in Hermes venv ──────────────────────────────────
step "Checking Hermes Python environment..."
HERMES_PYTHON="/opt/hermes/.venv/bin/python3"
docker exec "$HERMES_CONTAINER" "$HERMES_PYTHON" -c "import mcp, httpx" 2>/dev/null \
  && ok "mcp + httpx available" \
  || die "mcp or httpx not found in $HERMES_PYTHON. Run: docker exec $HERMES_CONTAINER pip install mcp httpx"

# ── 4. Prepare install dir ────────────────────────────────────────────────
step "Setting up Memex in $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
cd "$INSTALL_DIR"

# Skip if already installed
if docker ps --format '{{.Names}}' | grep -q '^memex$'; then
  warn "Container 'memex' already running — skipping compose up"
else
  # Download compose files
  curl -sSf "$REPO_RAW/docker-compose.prod.yml"     -o docker-compose.prod.yml
  curl -sSf "$REPO_RAW/docker-compose.hermes.yml"   -o docker-compose.hermes.yml
  ok "Compose files downloaded"

  # Generate .env if missing
  if [ ! -f .env ]; then
    POSTGRES_PASSWORD=$(openssl rand -hex 16)
    cat > .env <<EOF
OPENAI_API_KEY=${OPENAI_API_KEY}
OPENAI_LLM_API_KEY=${OPENAI_API_KEY}
POSTGRES_PASSWORD=${POSTGRES_PASSWORD}
DATABASE_URL=postgresql+asyncpg://memex:${POSTGRES_PASSWORD}@postgres:5432/memex
LLM_PROVIDER=openai
LLM_MODEL=gpt-4o-mini
UPLOAD_DIR=data/uploads
EOF
    chmod 600 .env
    ok ".env created (postgres password auto-generated)"
  else
    ok ".env already exists — skipping"
  fi

  # Start Memex
  step "Starting Memex containers..."
  HERMES_NETWORK="$HERMES_NETWORK" docker compose \
    -f docker-compose.prod.yml \
    -f docker-compose.hermes.yml \
    up -d
  ok "Containers started"
fi

# ── 5. Wait for Memex to be ready ────────────────────────────────────────
step "Waiting for Memex to be ready..."
for i in $(seq 1 30); do
  if docker exec "$HERMES_CONTAINER" curl -sf http://memex:8000/api/documents >/dev/null 2>&1; then
    ok "Memex is up and reachable from Hermes"
    break
  fi
  [ "$i" -eq 30 ] && die "Memex did not become ready in time. Check: docker logs memex"
  sleep 2
done

# ── 6. Install MCP bridge ─────────────────────────────────────────────────
step "Installing MCP bridge..."
docker exec "$HERMES_CONTAINER" curl -sSf \
  "$REPO_RAW/hermes/memex-bridge.py" -o /opt/data/memex-bridge.py
ok "Bridge installed at /opt/data/memex-bridge.py"

# Test it
docker exec -u hermes "$HERMES_CONTAINER" bash -c \
  'printf "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n" \
   | /opt/hermes/.venv/bin/python3 /opt/data/memex-bridge.py 2>/dev/null' \
  | grep -q '"name":"memex"' && ok "MCP handshake OK" || die "MCP bridge test failed. Check /opt/data/memex-bridge.py"

# ── 7. Install skill ──────────────────────────────────────────────────────
step "Installing Hermes skill..."
docker exec "$HERMES_CONTAINER" mkdir -p /opt/data/skills/memex
docker exec "$HERMES_CONTAINER" curl -sSf \
  "$REPO_RAW/hermes/memex-skill.md" -o /opt/data/skills/memex/SKILL.md
ok "Skill installed at /opt/data/skills/memex/SKILL.md"

# ── 8. Patch config.yaml ──────────────────────────────────────────────────
step "Updating Hermes config ($HERMES_CONFIG)..."
docker exec "$HERMES_CONTAINER" "$HERMES_PYTHON" - "$HERMES_CONFIG" <<'PYEOF'
import sys, yaml

path = sys.argv[1]
with open(path) as f:
    config = yaml.safe_load(f) or {}

if 'mcp_servers' not in config:
    config['mcp_servers'] = {}

if 'memex' in config['mcp_servers']:
    print("memex already in config — skipping")
    sys.exit(0)

config['mcp_servers']['memex'] = {
    'command': '/opt/hermes/.venv/bin/python3',
    'args': ['/opt/data/memex-bridge.py'],
    'env': {'MEMEX_URL': 'http://memex:8000'},
    'timeout': 120,
    'connect_timeout': 60,
}

with open(path, 'w') as f:
    yaml.dump(config, f, default_flow_style=False, allow_unicode=True, sort_keys=False)
print("config.yaml updated")
PYEOF
ok "config.yaml patched"

# ── 9. Restart Hermes ─────────────────────────────────────────────────────
step "Restarting Hermes..."
docker restart "$HERMES_CONTAINER" >/dev/null
ok "Hermes restarting..."
sleep 20

# ── 10. Verify ────────────────────────────────────────────────────────────
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
