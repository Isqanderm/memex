# Memex Rust Edition

Lightweight, self-contained variant of Memex built with Rust + SQLite. No PostgreSQL, no Python runtime — a single binary that runs on any Linux machine, including Raspberry Pi.

---

## Python vs Rust — when to choose

| | Python | Rust |
|---|---|---|
| **Database** | PostgreSQL 15 + pgvector | SQLite (built-in) |
| **RAM idle** | ~2.5 GB | ~80–120 MB |
| **Cold start** | ~15 sec | ~2–3 sec |
| **Docker image** | ~1.8 GB | ~150 MB |
| **Installation** | Docker Compose (2 containers) | Single binary or 1-container Docker |
| **Raspberry Pi / VPS 512 MB** | ❌ | ✅ |
| **Hybrid search (pgvector + BM25)** | ✅ (pgvector + tsvector) | ✅ (sqlite-vec + tantivy) |
| **MCP server** | ✅ | ✅ (`memex-mcp` binary) |
| **Corpus size** | Unlimited (PostgreSQL scales) | Practical limit ~50k chunks |
| **Multi-user** | ✅ | Single-user only |
| **Formats** | PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB | PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB |

**Choose Rust if:**
- You're deploying on a Raspberry Pi, low-RAM VPS, or home server
- You want zero external dependencies (no PostgreSQL, no Python)
- Fast startup matters (e.g. on-demand activation)
- Personal, single-user use case

**Choose Python if:**
- You need full hybrid search with pgvector
- You plan to scale to large document corpora (100k+ chunks)
- You need multi-user support
- You're actively iterating on features

---

## Quick start

### Option A: Docker (recommended)

```bash
# 1. Download config files
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/docker-compose.rust.yml
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/rust/.env.example
cp rust/.env.example .env

# 2. Fill in your LLM API key
#    For Claude:
echo "ANTHROPIC_API_KEY=sk-ant-..." >> .env
#    For OpenAI:
echo "OPENAI_LLM_API_KEY=sk-..." >> .env
echo "LLM_PROVIDER=openai" >> .env
echo "LLM_MODEL=gpt-4o-mini" >> .env

# 3. Start
docker compose -f docker-compose.rust.yml up -d
# → http://localhost:8000
```

First run downloads ONNX embedding (~90 MB) and reranker (~100 MB) models. Subsequent starts use the cache.

### Option B: Pre-built binary (Linux)

Download the binary from [GitHub Releases](https://github.com/Isqanderm/memex/releases) and run directly:

```bash
# Linux x86_64
curl -LO https://github.com/Isqanderm/memex/releases/latest/download/memex-linux-amd64
chmod +x memex-linux-amd64

# Linux ARM64 (Raspberry Pi 4/5) — use Docker option instead, binary coming soon

# Create data directories
mkdir -p data/uploads data/tantivy

# Copy and fill .env
curl -O https://raw.githubusercontent.com/Isqanderm/memex/main/rust/.env.example
cp rust/.env.example .env
# edit .env with your API keys

# Run
./memex-linux-amd64   # or memex-linux-arm64 on RPi
# → http://localhost:8000
```

### Option C: Raspberry Pi systemd service

```bash
# Install binary
sudo cp memex-linux-arm64 /usr/local/bin/memex
sudo chmod +x /usr/local/bin/memex

# Create data and config dirs
sudo mkdir -p /opt/memex/data/uploads /opt/memex/data/tantivy
sudo cp .env /opt/memex/.env

# Create systemd service
sudo tee /etc/systemd/system/memex.service << 'EOF'
[Unit]
Description=Memex personal RAG
After=network.target

[Service]
Type=simple
User=pi
WorkingDirectory=/opt/memex
EnvironmentFile=/opt/memex/.env
ExecStart=/usr/local/bin/memex
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now memex
# → http://raspberry-pi-ip:8000
```

---

## Configuration

All settings via environment variables (or `.env` file in working directory):

```bash
# Storage paths
DATABASE_PATH=data/memex.db      # SQLite database
TANTIVY_PATH=data/tantivy         # Full-text search index
UPLOAD_DIR=data/uploads           # Uploaded files

# Embedding model (downloaded automatically on first run)
LOCAL_EMBEDDING_MODEL=multilingual-e5-small   # 384-dim, EN+RU
EMBEDDING_DIMENSIONS=384

# LLM provider
LLM_PROVIDER=claude               # or: openai
LLM_MODEL=claude-sonnet-4-6       # or: gpt-4o-mini
LLM_MAX_TOKENS=2048
LLM_TEMPERATURE=0.1

# API keys (set one based on LLM_PROVIDER)
ANTHROPIC_API_KEY=sk-ant-...
# OPENAI_LLM_API_KEY=sk-...

# Server
HOST=0.0.0.0
PORT=8000

# Retrieval tuning
SEMANTIC_TOP_K=20
BM25_TOP_K=20
RRF_K=60
RERANKER_TOP_N=5
L2_CHUNK_SIZE=512
L1_CHUNK_SIZE=128
L2_CHUNK_OVERLAP=64
```

---

## MCP server for Claude Code (`memex-mcp`)

Each release ships a companion binary `memex-mcp` — a native MCP server for Claude Code. It runs independently of the HTTP server.

### Install

```bash
# Download alongside the main binary
curl -LO https://github.com/Isqanderm/memex/releases/latest/download/memex-linux-amd64
curl -LO https://github.com/Isqanderm/memex/releases/latest/download/memex-mcp-linux-amd64
chmod +x memex-linux-amd64 memex-mcp-linux-amd64
```

### Configure Claude Code

Add to `.claude/settings.json`:

```json
{
  "mcpServers": {
    "memex": {
      "command": "/absolute/path/to/memex-mcp-linux-amd64",
      "env": {
        "DATABASE_PATH": "/absolute/path/to/data/memex.db",
        "TANTIVY_PATH": "/absolute/path/to/data/tantivy",
        "LLM_PROVIDER": "openai",
        "OPENAI_LLM_API_KEY": "sk-...",
        "LLM_MODEL": "gpt-4o-mini"
      }
    }
  }
}
```

`DATABASE_PATH` must point to the same SQLite file used by the HTTP server.

### Available tools

| Tool | Description |
|---|---|
| `remember` | Save text as a memory fact (extracted by LLM) |
| `recall` | Semantic search over memories and documents |
| `context` | Get user profile summary (static + recent activity) |
| `observe` | Extract facts from a conversation |
| `memories` | List stored memories, optionally filtered by category |
| `index_file` | Index a file from disk path |
| `check_indexing` | Check indexing job status by job_id |
| `list_documents` | List all indexed documents |
| `forget` | Delete a memory by ID |

---

## API

The Rust version exposes the same REST API as the Python version. See [REST API docs](../README.md#rest-api) for usage examples.

**Notable differences from Python:**
- `POST /api/search/chunks` — returns array directly (Python wraps in `{"chunks": [...]}`)
- `DELETE /api/memory/:id` — returns `204 No Content` (Python returns `{"status": "deleted"}`)

---

## Resource requirements

| | Min | Recommended |
|---|---|---|
| **RAM** | 256 MB | 512 MB+ |
| **Storage** | 500 MB (models cache) | 2 GB+ |
| **CPU** | ARMv8 / x86_64 | Any modern core |
| **OS** | Linux | Linux |

Model download on first run: ~200 MB total (embedding + reranker). Models are cached to `~/.cache/huggingface` (Docker) or `$HOME/.cache/huggingface` (binary).

---

## Versioning

Rust releases are versioned independently from Python:
- Python releases: `v2.x.x` tags
- Rust releases: `rust/v3.x.x` tags
- Docker images: `ghcr.io/isqanderm/memex:rust-3.x.x` and `rust-latest`

See [rust/CHANGELOG.md](../rust/CHANGELOG.md) for Rust-specific changes.
