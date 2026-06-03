#!/bin/bash
# Memex × OpenClaw one-command installer
#
# Option A (recommended):
#   curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-openclaw.sh -o install-openclaw.sh
#   bash install-openclaw.sh
#
# Option B (one-liner):
#   OPENAI_LLM_API_KEY=sk-... bash <(curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-openclaw.sh)

set -euo pipefail

REPO_RAW="https://raw.githubusercontent.com/Isqanderm/memex/main"

# ── Overridable env vars ──────────────────────────────────────────────────────
OPENCLAW_CONTAINER="${OPENCLAW_CONTAINER:-}"      # override container auto-detect
OPENCLAW_NETWORK="${OPENCLAW_NETWORK:-}"          # override network auto-detect
INSTALL_DIR="${MEMEX_INSTALL_DIR:-/docker/memex}"

# ── Colours ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
ok()   { echo -e "${GREEN}✓${NC} $*"; }
warn() { echo -e "${YELLOW}⚠${NC}  $*"; }
die()  { echo -e "${RED}✗${NC} $*"; exit 1; }
step() { echo -e "\n${YELLOW}→${NC} $*"; }

echo "╔══════════════════════════════════════╗"
echo "║    Memex × OpenClaw Installer        ║"
echo "╚══════════════════════════════════════╝"

# ── 0. Dependency checks ──────────────────────────────────────────────────────
step "Checking dependencies..."

if ! docker info >/dev/null 2>&1; then
  die "Docker is not running or not accessible. Start Docker and try again."
fi
ok "Docker is running"

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

if ! command -v curl >/dev/null 2>&1; then
  die "curl is required but not installed. Install it and try again."
fi
ok "curl available"

# ── 1. LLM API key ───────────────────────────────────────────────────────────
if [ -z "${OPENAI_LLM_API_KEY:-}" ] && [ -z "${ANTHROPIC_API_KEY:-}" ]; then
  read -rsp "OpenAI LLM API key (press Enter to use Anthropic/Claude instead): " OPENAI_LLM_API_KEY
  echo
fi
if [ -z "${OPENAI_LLM_API_KEY:-}" ] && [ -z "${ANTHROPIC_API_KEY:-}" ]; then
  die "Set OPENAI_LLM_API_KEY (OpenAI) or ANTHROPIC_API_KEY (Claude) for the LLM provider"
fi

# ── 2. Auto-detect OpenClaw ───────────────────────────────────────────────────
step "Detecting OpenClaw..."

if [ -z "$OPENCLAW_CONTAINER" ]; then
  OPENCLAW_CONTAINER=$(docker ps --format '{{.Names}}' | grep -E '^openclaw-gateway$' | head -1 || true)
  if [ -z "$OPENCLAW_CONTAINER" ]; then
    OPENCLAW_CONTAINER=$(docker ps --format '{{.Names}}' | grep -E '^openclaw_gateway$' | head -1 || true)
  fi
  if [ -z "$OPENCLAW_CONTAINER" ]; then
    OPENCLAW_CONTAINER=$(docker ps --format '{{.Names}}' | grep -i 'openclaw' | head -1 || true)
  fi
  if [ -z "$OPENCLAW_CONTAINER" ]; then
    die "No OpenClaw container found. Is OpenClaw running? (docker ps)\nHint: set OPENCLAW_CONTAINER=<name> to override."
  fi
fi
ok "Container: $OPENCLAW_CONTAINER"

if ! docker inspect --format '{{.State.Running}}' "$OPENCLAW_CONTAINER" 2>/dev/null | grep -q 'true'; then
  die "Container '$OPENCLAW_CONTAINER' is not running. Start OpenClaw first."
fi

if [ -z "$OPENCLAW_NETWORK" ]; then
  ALL_NETWORKS=$(docker inspect "$OPENCLAW_CONTAINER" \
    --format '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' \
    | tr ' ' '\n' | grep -v '^$' || true)
  OPENCLAW_NETWORK=$(echo "$ALL_NETWORKS" | grep -i 'openclaw' | head -1 || true)
  if [ -z "$OPENCLAW_NETWORK" ]; then
    OPENCLAW_NETWORK=$(echo "$ALL_NETWORKS" | head -1 || true)
  fi
  if [ -z "$OPENCLAW_NETWORK" ]; then
    die "Could not detect OpenClaw Docker network.\nHint: set OPENCLAW_NETWORK=<name> to override."
  fi
fi
ok "Network: $OPENCLAW_NETWORK"

OPENCLAW_CONFIG=$(docker exec "$OPENCLAW_CONTAINER" \
  bash -c 'echo "${OPENCLAW_CONFIG_FILE:-/home/node/.openclaw/openclaw.json}"' 2>/dev/null \
  || echo "/home/node/.openclaw/openclaw.json")
ok "Config: $OPENCLAW_CONFIG"

OPENCLAW_VENV="/home/node/.openclaw/.venv"
OPENCLAW_PYTHON="$OPENCLAW_VENV/bin/python3"

# ── 3. Set up Python venv in mounted config volume ────────────────────────────
step "Setting up Python venv in OpenClaw config volume..."

if ! docker exec "$OPENCLAW_CONTAINER" python3 --version >/dev/null 2>&1; then
  die "python3 not found in container '$OPENCLAW_CONTAINER'.\nInstall it via OPENCLAW_IMAGE_PIP_PACKAGES or rebuild the image with Python."
fi
ok "python3 available"

if ! docker exec "$OPENCLAW_CONTAINER" test -f "$OPENCLAW_PYTHON" 2>/dev/null; then
  docker exec "$OPENCLAW_CONTAINER" python3 -m venv "$OPENCLAW_VENV"
  ok "venv created at $OPENCLAW_VENV (persists across container restarts)"
else
  ok "venv already exists at $OPENCLAW_VENV"
fi

if ! docker exec "$OPENCLAW_CONTAINER" "$OPENCLAW_PYTHON" -c "import mcp, httpx" 2>/dev/null; then
  warn "mcp or httpx not found in venv — installing..."
  if ! docker exec "$OPENCLAW_CONTAINER" "$OPENCLAW_VENV/bin/pip" install mcp httpx -q; then
    die "Failed to install mcp/httpx. Run manually:\n  docker exec $OPENCLAW_CONTAINER $OPENCLAW_VENV/bin/pip install mcp httpx"
  fi
  docker exec "$OPENCLAW_CONTAINER" "$OPENCLAW_PYTHON" -c "import mcp, httpx" 2>/dev/null \
    || die "mcp/httpx still not importable after install. Check the venv."
fi
ok "mcp + httpx available in venv"

# ── 4. Prepare install dir ────────────────────────────────────────────────────
step "Setting up Memex in $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
cd "$INSTALL_DIR"

if docker ps --format '{{.Names}}' | grep -q '^memex$'; then
  warn "Container 'memex' already running — skipping compose up"
else
  curl -sSf "$REPO_RAW/docker-compose.prod.yml"     -o docker-compose.prod.yml
  curl -sSf "$REPO_RAW/docker-compose.openclaw.yml" -o docker-compose.openclaw.yml
  ok "Compose files downloaded"

  if [ ! -f .env ]; then
    POSTGRES_PASSWORD=$(openssl rand -hex 16)
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
  OPENCLAW_NETWORK="$OPENCLAW_NETWORK" docker compose \
    -f docker-compose.prod.yml \
    -f docker-compose.openclaw.yml \
    up -d
  ok "Containers started"
fi

# ── 5. Wait for Memex to be ready ────────────────────────────────────────────
step "Waiting for Memex to be ready..."
for i in $(seq 1 30); do
  if docker exec "$OPENCLAW_CONTAINER" curl -sf http://memex:8000/api/documents >/dev/null 2>&1; then
    ok "Memex is up and reachable from OpenClaw"
    break
  fi
  [ "$i" -eq 30 ] && die "Memex did not become ready in time. Check: docker logs memex"
  sleep 2
done

# ── 6. Install MCP bridge ─────────────────────────────────────────────────────
step "Installing MCP bridge..."
docker exec "$OPENCLAW_CONTAINER" mkdir -p /home/node/.openclaw/shared
docker exec "$OPENCLAW_CONTAINER" curl -sSf \
  "$REPO_RAW/shared/memex-bridge.py" -o /home/node/.openclaw/shared/memex-bridge.py
docker exec "$OPENCLAW_CONTAINER" curl -sSf \
  "$REPO_RAW/openclaw/memex-bridge.py" -o /home/node/.openclaw/memex-bridge.py
ok "Bridge installed at /home/node/.openclaw/memex-bridge.py"

docker exec "$OPENCLAW_CONTAINER" bash -c \
  "printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n' \
   | $OPENCLAW_PYTHON /home/node/.openclaw/memex-bridge.py 2>/dev/null" \
  | grep -q '"name":"memex"' && ok "MCP handshake OK" || die "MCP bridge test failed. Check /home/node/.openclaw/memex-bridge.py"

# ── 7. Patch openclaw.json ────────────────────────────────────────────────────
step "Updating OpenClaw config ($OPENCLAW_CONFIG)..."

print_manual_config() {
  warn "$1 Add the following to $OPENCLAW_CONFIG manually (under \"mcp\" -> \"servers\"):"
  printf '\n  "mcp": {\n    "servers": {\n      "memex": {\n        "command": "%s",\n        "args": ["/home/node/.openclaw/memex-bridge.py"],\n        "env": { "MEMEX_URL": "http://memex:8000" }\n      }\n    }\n  }\n\n' "$OPENCLAW_PYTHON"
}

if ! docker exec "$OPENCLAW_CONTAINER" "$OPENCLAW_PYTHON" - "$OPENCLAW_CONFIG" "$OPENCLAW_PYTHON" <<'PYEOF' 2>/dev/null; then
import sys, json, os

path = sys.argv[1]
python_bin = sys.argv[2]
try:
    with open(path) as f:
        cfg = json.load(f)
except FileNotFoundError:
    cfg = {}
except Exception as e:
    print(f"ERROR reading config: {e}", file=sys.stderr); sys.exit(1)

if "mcp" not in cfg:
    cfg["mcp"] = {}
if "servers" not in cfg["mcp"]:
    cfg["mcp"]["servers"] = {}
if "memex" in cfg["mcp"]["servers"]:
    print("memex already in config — skipping"); sys.exit(0)

cfg["mcp"]["servers"]["memex"] = {
    "command": python_bin,
    "args": ["/home/node/.openclaw/memex-bridge.py"],
    "env": {"MEMEX_URL": "http://memex:8000"},
}

try:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(cfg, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print("openclaw.json updated")
except Exception as e:
    print(f"ERROR writing config: {e}", file=sys.stderr); sys.exit(1)
PYEOF
  print_manual_config "Automatic config patch failed."
else
  ok "openclaw.json patched"
fi

# ── 8. Restart OpenClaw ───────────────────────────────────────────────────────
step "Restarting OpenClaw..."
docker restart "$OPENCLAW_CONTAINER" >/dev/null
ok "OpenClaw restarting..."
sleep 20

# ── 9. Summary ────────────────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}╔══════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  ✅  Memex is live in OpenClaw!       ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════╝${NC}"
echo ""
echo "Installed in: $INSTALL_DIR"
echo "Bridge:       /home/node/.openclaw/memex-bridge.py"
echo "Config:       $OPENCLAW_CONFIG"
echo "Logs:         docker logs memex"
echo ""
echo "Verify (from host):"
echo "  docker exec $OPENCLAW_CONTAINER curl -s http://memex:8000/api/documents"
