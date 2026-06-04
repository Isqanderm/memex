#!/usr/bin/env bash
# scripts/run_golden_tests.sh
# Runs the golden test suite against both Python and Rust backends.
# Usage: bash scripts/run_golden_tests.sh [--e2e]
#
# Requirements:
#   - Docker + Docker Compose installed
#   - .env file in project root with LLM keys (for --e2e mode)
#   - uv installed

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
E2E_MARKER=""
if [[ "${1:-}" == "--e2e" ]]; then
  E2E_MARKER="-m 'unit or e2e'"
else
  E2E_MARKER="-m unit"
fi

PYTHON_PORT=8000
RUST_PORT=8001

cleanup() {
  echo "→ Stopping services..."
  docker compose -f "$ROOT/docker-compose.yml" down --timeout 10 2>/dev/null || true
  docker compose -f "$ROOT/docker-compose.rust.yml" down --timeout 10 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Starting Python backend (port $PYTHON_PORT) ==="
docker compose -f "$ROOT/docker-compose.yml" up -d
echo "   Waiting for Python backend to be healthy..."
for i in $(seq 1 30); do
  if curl -sf "http://localhost:$PYTHON_PORT/health" > /dev/null 2>&1; then
    echo "   Python backend ready."
    break
  fi
  sleep 2
done

echo "=== Starting Rust backend (port $RUST_PORT) ==="
# Override port via env — rust docker-compose needs PORT env var
PORT=$RUST_PORT docker compose -f "$ROOT/docker-compose.rust.yml" up -d
echo "   Waiting for Rust backend to be healthy..."
for i in $(seq 1 30); do
  if curl -sf "http://localhost:$RUST_PORT/health" > /dev/null 2>&1; then
    echo "   Rust backend ready."
    break
  fi
  sleep 2
done

PYTHON_RESULT=0
RUST_RESULT=0

echo ""
echo "=== Running golden tests against Python backend ==="
MEMEX_BASE_URL="http://localhost:$PYTHON_PORT" \
  uv run pytest tests/golden/ $E2E_MARKER -v \
  --tb=short --no-header \
  --junitxml="$ROOT/test-results/golden-python.xml" \
  || PYTHON_RESULT=$?

echo ""
echo "=== Running golden tests against Rust backend ==="
MEMEX_BASE_URL="http://localhost:$RUST_PORT" \
  uv run pytest tests/golden/ $E2E_MARKER -v \
  --tb=short --no-header \
  --junitxml="$ROOT/test-results/golden-rust.xml" \
  || RUST_RESULT=$?

echo ""
echo "=== Results ==="
if [ $PYTHON_RESULT -eq 0 ]; then
  echo "  Python backend: PASSED"
else
  echo "  Python backend: FAILED (exit $PYTHON_RESULT)"
fi

if [ $RUST_RESULT -eq 0 ]; then
  echo "  Rust backend:   PASSED"
else
  echo "  Rust backend:   FAILED (exit $RUST_RESULT)"
fi

# Exit non-zero if either failed
[ $PYTHON_RESULT -eq 0 ] && [ $RUST_RESULT -eq 0 ]
