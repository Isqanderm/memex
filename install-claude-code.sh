#!/bin/bash
# Memex × Claude Code one-command installer
#
# Usage:
#   bash <(curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-claude-code.sh)
#
# Or download first:
#   curl -sSL https://raw.githubusercontent.com/Isqanderm/memex/main/install-claude-code.sh -o install-claude-code.sh
#   bash install-claude-code.sh

set -euo pipefail

REPO_RAW="https://raw.githubusercontent.com/Isqanderm/memex/main"
INSTALL_DIR="${MEMEX_INSTALL_DIR:-$HOME/.local/share/memex}"
BRIDGE_PATH="${MEMEX_BRIDGE_PATH:-$INSTALL_DIR/memex-bridge.py}"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
ok()   { echo -e "${GREEN}✓${NC} $*"; }
warn() { echo -e "${YELLOW}⚠${NC}  $*"; }
die()  { echo -e "${RED}✗${NC} $*"; exit 1; }
step() { echo -e "\n${YELLOW}→${NC} $*"; }

echo "╔══════════════════════════════════════╗"
echo "║   Memex × Claude Code Installer      ║"
echo "╚══════════════════════════════════════╝"

# ── 0. Dependency checks ──────────────────────────────────────────────────────
step "Checking dependencies..."

if ! docker info >/dev/null 2>&1; then
  die "Docker is not running. Start Docker and try again."
fi
ok "Docker is running"

if ! command -v python3 >/dev/null 2>&1; then
  die "python3 is required but not found."
fi
PYTHON=$(command -v python3)
ok "Python: $PYTHON"

if ! command -v curl >/dev/null 2>&1; then
  die "curl is required but not found."
fi
ok "curl available"

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

# ── 1. Check mcp + httpx ──────────────────────────────────────────────────────
step "Checking Python packages..."
if ! "$PYTHON" -c "import mcp, httpx" 2>/dev/null; then
  warn "mcp or httpx not found — installing..."
  "$PYTHON" -m pip install mcp httpx -q \
    || die "Failed to install mcp/httpx. Run: pip install mcp httpx"
  "$PYTHON" -c "import mcp, httpx" 2>/dev/null \
    || die "mcp/httpx still not importable after install."
fi
ok "mcp + httpx available"

# ── 2. OpenAI API key ─────────────────────────────────────────────────────────
if [ -z "${OPENAI_API_KEY:-}" ]; then
  read -rsp "OpenAI API key (required for embeddings): " OPENAI_API_KEY
  echo
fi
[ -z "${OPENAI_API_KEY:-}" ] && die "OPENAI_API_KEY is required (used for embeddings even if LLM_PROVIDER=claude)"

# ── 3. Start Memex ────────────────────────────────────────────────────────────
step "Setting up Memex in $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
cd "$INSTALL_DIR"

if docker ps --format '{{.Names}}' | grep -q '^memex$'; then
  warn "Container 'memex' already running — skipping compose up"
else
  curl -sSf "$REPO_RAW/docker-compose.prod.yml" -o docker-compose.prod.yml
  ok "docker-compose.prod.yml downloaded"

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

  step "Starting Memex..."
  docker compose -f docker-compose.prod.yml up -d
  ok "Containers started"
fi

# ── 4. Wait for Memex ─────────────────────────────────────────────────────────
step "Waiting for Memex to be ready..."
for i in $(seq 1 30); do
  if curl -sf http://localhost:8000/api/documents >/dev/null 2>&1; then
    ok "Memex is up at http://localhost:8000"
    break
  fi
  [ "$i" -eq 30 ] && die "Memex did not become ready in time. Check: docker logs memex"
  sleep 2
done

# ── 5. Install MCP bridge ─────────────────────────────────────────────────────
step "Installing MCP bridge..."
mkdir -p "$(dirname "$BRIDGE_PATH")"
curl -sSf "$REPO_RAW/claude-code/memex-bridge.py" -o "$BRIDGE_PATH"
ok "Bridge installed at $BRIDGE_PATH"

# Test handshake
"$PYTHON" - <<PYEOF
import subprocess, json, sys
result = subprocess.run(
    ["$PYTHON", "$BRIDGE_PATH"],
    input='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}\n',
    capture_output=True, text=True, timeout=10
)
lines = [l for l in result.stdout.splitlines() if l.strip()]
if lines:
    data = json.loads(lines[0])
    if data.get("result", {}).get("serverInfo", {}).get("name") == "memex":
        print("MCP handshake OK")
        sys.exit(0)
print("MCP bridge test failed", file=sys.stderr)
sys.exit(1)
PYEOF
ok "MCP handshake OK"

# ── 6. Patch .claude/settings.json ───────────────────────────────────────────
step "Configuring Claude Code..."

# Find settings.json — prefer project-local, fall back to global
find_settings() {
  if [ -f ".claude/settings.json" ]; then
    echo ".claude/settings.json"
  elif [ -f "$HOME/.claude/settings.json" ]; then
    echo "$HOME/.claude/settings.json"
  else
    echo "$HOME/.claude/settings.json"
    mkdir -p "$HOME/.claude"
    echo '{}' > "$HOME/.claude/settings.json"
  fi
}

SETTINGS_FILE=$(find_settings)
ok "Settings file: $SETTINGS_FILE"

"$PYTHON" - "$SETTINGS_FILE" "$BRIDGE_PATH" <<'PYEOF'
import sys, json
from pathlib import Path

settings_path = Path(sys.argv[1])
bridge_path = sys.argv[2]

try:
    config = json.loads(settings_path.read_text()) if settings_path.exists() else {}
except json.JSONDecodeError:
    config = {}

if "mcpServers" not in config:
    config["mcpServers"] = {}

if "memex" in config["mcpServers"]:
    print("memex already in mcpServers — skipping")
    sys.exit(0)

config["mcpServers"]["memex"] = {
    "command": sys.executable,
    "args": [bridge_path],
    "env": {"MEMEX_URL": "http://localhost:8000"}
}

settings_path.write_text(json.dumps(config, indent=2))
print(f"Added memex to {settings_path}")
PYEOF
ok "Claude Code configured"

# ── Done ──────────────────────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}╔══════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  ✅  Memex is live in Claude Code!   ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════╝${NC}"
echo ""
echo "Memex:    http://localhost:8000"
echo "Bridge:   $BRIDGE_PATH"
echo "Settings: $SETTINGS_FILE"
echo ""
echo "Restart Claude Code to load the MCP server."
echo "Then try: \"remember this\" or \"recall what I know about...\""
