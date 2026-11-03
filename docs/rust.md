# Memex Rust Edition

Lightweight, self-contained variant of Memex built with Rust + SQLite. No PostgreSQL, no Python runtime — a single binary that runs on any Linux machine, including Raspberry Pi.

---

## Python vs Rust — when to choose

| | Python | Rust |
|---|---|---|
| **Database** | PostgreSQL 15 + pgvector | SQLite (built-in) |
| **RAM idle** | ~2.5 GB | ~80–120 MB |
| **Cold start** | ~15 sec | ~2–3 sec |
| **Docker image** | ~1.8 GB | ~1.8 GB (models baked in) |
| **Installation** | Docker Compose (2 containers) | Single binary or 1-container Docker |
| **Raspberry Pi / VPS 512 MB** | ❌ | ✅ |
| **Hybrid search (pgvector + BM25)** | ✅ (pgvector + tsvector) | ✅ (sqlite-vec + tantivy) |
| **MCP server** | ✅ (stdio) | ✅ (HTTP `/mcp` in-process) |
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

The Docker image already contains the ONNX models (baked into `/app/models`), so the first start needs no download. Compose binds the port to `127.0.0.1:8000` and mounts a single `./data:/app/data`.

### Option B: Pre-built binary (Linux)

Download the latest binary from [GitHub Releases](https://github.com/Isqanderm/memex/releases) (look for the `rust/v*` tag):

```bash
# Linux x86_64
curl -L https://github.com/Isqanderm/memex/releases/download/rust%2Fv3.0.0-rc.1/memex-linux-amd64 -o memex
chmod +x memex

# Linux ARM64 (Raspberry Pi 4/5)
curl -L https://github.com/Isqanderm/memex/releases/download/rust%2Fv3.0.0-rc.1/memex-linux-arm64 -o memex
chmod +x memex

# Create data directories
mkdir -p data/uploads data/tantivy

# Download .env template
curl -L https://raw.githubusercontent.com/Isqanderm/memex/main/rust/.env.example -o .env
# Edit .env — add your LLM API key (ANTHROPIC_API_KEY or OPENAI_LLM_API_KEY)

# Run
./memex
# → http://localhost:8000
```

> **PDF support** requires `poppler-utils` on the host system:
> ```bash
> # Debian / Ubuntu / Raspberry Pi OS
> sudo apt-get install -y poppler-utils
> ```
> Without it, PDF uploads will fail silently. DOCX, MD, TXT, PPTX, XLSX, EPUB work without any system dependencies.

On first start, the binary downloads ONNX models into `MODEL_CACHE_DIR` (default `data/models`); run `./memex --download-models` to do it ahead of time. Subsequent starts take 2–3 seconds.

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
# LLM_BASE_URL=http://host.docker.internal:11434/v1   # OpenAI-compatible endpoint (key optional)

# Models
# MODEL_CACHE_DIR=data/models       # Docker image uses /app/models
# RERANKER_MODEL=jina-reranker-v2-base-multilingual

# MCP (/mcp)
# MCP_ALLOWED_HOSTS=localhost,127.0.0.1,::1,memex

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

## MCP over HTTP

The MCP server runs inside the main `memex` process: Streamable HTTP at `/mcp` (stateless, JSON responses). There is no separate binary.

```bash
claude mcp add --transport http memex http://localhost:8000/mcp
```

Request body limit is 16 MiB. Requests carrying a browser `Origin` header are rejected with 403 (requests without `Origin`, as sent by MCP clients, pass). The `Host` header must be listed in `MCP_ALLOWED_HOSTS` (default `localhost,127.0.0.1,::1,memex`); add your own hostname there when exposing the server behind a reverse proxy.

### Available tools

| Tool | Description |
|---|---|
| `remember` | Save text as a memory fact (extracted by LLM) |
| `recall` | Semantic search over memories and documents |
| `forget` | Delete a memory by ID |
| `context` | Get user profile summary (static + recent activity) |
| `observe` | Extract facts from a conversation |
| `memories` | List stored memories, optionally filtered by category |
| `index_text` | Index text content as a document (async, returns a job id) |
| `check_indexing` | Check indexing job status by job_id |
| `list_documents` | List all indexed documents |
| `delete_document` | Delete an indexed document by ID |

### Deploying inside mcp-registry

Add memex as a stack component of the registry deployment: no `ports` (it is reachable only on the internal network), auth mode `none`, MCP URL `http://memex:8000/mcp`. The default `MCP_ALLOWED_HOSTS` already includes `memex`.

---

## Local LLM and models

### Any OpenAI-compatible endpoint

Set `LLM_BASE_URL` to use a local or self-hosted model. When it is set, `OPENAI_LLM_API_KEY` is optional. Example for Ollama on the Docker host:

```bash
LLM_PROVIDER=openai
LLM_BASE_URL=http://host.docker.internal:11434/v1
LLM_MODEL=llama3.1
```

### Models and cache

- `MODEL_CACHE_DIR` - where ONNX models are stored. Default `data/models` for bare-binary runs; the Docker image bakes models into `/app/models` (set explicitly by `docker-compose.rust.yml`).
- `RERANKER_MODEL` - `jina-reranker-v2-base-multilingual` (default), `bge-reranker-base`, or `bge-reranker-v2-m3` (~2.3 GB).
- `memex --download-models` downloads both ONNX models (embedding and reranker) into `MODEL_CACHE_DIR` and exits. Use it to pre-warm offline hosts or build images.

---

---

## API

The Rust version exposes the same REST API as the Python version with identical request/response shapes. See [REST API docs](../README.md#rest-api) for usage examples.

---

## Resource requirements

| | Min | Recommended |
|---|---|---|
| **RAM** | 256 MB | 512 MB+ |
| **Storage** | 2 GB (models cache) | 4 GB+ |
| **CPU** | ARMv8 / x86_64 | Any modern core |
| **OS** | Linux | Linux |

Models (~1.2 GB with the default reranker) are cached in `MODEL_CACHE_DIR`: `/app/models` in Docker (baked into the image), `data/models` for the binary.

---

## Versioning

Rust releases are versioned independently from Python:
- Python releases: `v2.x.x` tags
- Rust releases: `rust/v3.x.x` tags
- Docker images: `ghcr.io/isqanderm/memex:rust-3.x.x` and `rust-latest` (built by `docker.yml` on `rust/v*` tags; pre-releases get only the versioned tag)

See [rust/CHANGELOG.md](../rust/CHANGELOG.md) for Rust-specific changes.
