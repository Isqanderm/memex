# Memex Rust Edition

Lightweight, self-contained variant of Memex built with Rust + SQLite. No PostgreSQL, no Python runtime — a single binary that runs on any Linux machine, including Raspberry Pi.

---

## Python vs Rust — when to choose

| | Python | Rust |
|---|---|---|
| **Database** | PostgreSQL 15 + pgvector | SQLite (built-in) |
| **RAM** | ~2.5 GB idle | ~2.3 GiB after the first query (both models loaded; measured, arm64 build) |
| **Cold start** | ~15 sec | ~20–30 sec (models load) |
| **Docker image** | ~1.8 GB | ~1.8 GB (models baked in) |
| **Installation** | Docker Compose (2 containers) | Single binary or 1-container Docker |
| **Raspberry Pi / VPS 512 MB** | ❌ | ❌ (needs ~3 GB RAM; Raspberry Pi 4/5 with 4 GB+ works) |
| **Hybrid search (pgvector + BM25)** | ✅ (pgvector + tsvector) | ✅ (sqlite-vec + tantivy) |
| **MCP server** | ✅ (stdio) | ✅ (HTTP `/mcp` in-process) |
| **Corpus size** | Unlimited (PostgreSQL scales) | Practical limit ~50k chunks |
| **Multi-user** | ✅ | Single-user only |
| **Formats** | PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB | PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB |

**Choose Rust if:**
- You're deploying on a Raspberry Pi (4 GB+), a small VPS with 3 GB+ RAM, or a home server
- You want zero external dependencies (no PostgreSQL, no Python)
- You want a simple single-container deployment with a smaller storage footprint
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
curl -L https://raw.githubusercontent.com/Isqanderm/memex/main/rust/.env.example -o .env

# 2. Choose an LLM (edit .env; see "Choose an LLM" below)
#    The default .env already targets Ollama on the host (qwen2.5:7b): `ollama pull qwen2.5:7b`

# 3. Start
docker compose -f docker-compose.rust.yml up -d
# → http://localhost:8000
```

#### Choose an LLM

Pick exactly one in `.env`:

1. **Ollama (default)** - local, no key. The template already has `LLM_PROVIDER=openai`, `LLM_BASE_URL=http://host.docker.internal:11434/v1`, `LLM_MODEL=qwen2.5:7b`. Run `ollama pull qwen2.5:7b` first. Compose maps `host.docker.internal` to the host (`extra_hosts: host-gateway`), so it also works on Linux Docker Engine. For Ollama on another machine, put that machine's address in `LLM_BASE_URL`. For the bare binary (Option B) use `http://127.0.0.1:11434/v1`.
2. **OpenAI** - set `LLM_PROVIDER=openai`, `OPENAI_LLM_API_KEY=sk-...`, `LLM_MODEL=gpt-4o-mini`, and remove or comment out `LLM_BASE_URL`.
3. **Claude** - set `LLM_PROVIDER=claude`, `ANTHROPIC_API_KEY=sk-ant-...`, `LLM_MODEL=claude-sonnet-4-6`, and remove or comment out `LLM_BASE_URL`.

Do not set `MODEL_CACHE_DIR` for Docker (the image sets `/app/models`).

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
# Edit .env — choose an LLM (see "Choose an LLM" above).
# For Ollama on this machine set LLM_BASE_URL=http://127.0.0.1:11434/v1
# (host.docker.internal only resolves inside Docker).

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

On first start, the binary downloads ONNX models into `MODEL_CACHE_DIR` (default `data/models`); run `./memex --download-models` to do it ahead of time. Loading both models into memory takes about 20–30 seconds on every start.

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
# SQLite database
DATABASE_PATH=data/memex.db
# Full-text search index
TANTIVY_PATH=data/tantivy
# Uploaded files
UPLOAD_DIR=data/uploads

# Embedding model (downloaded automatically on first run)
# 384-dim, EN+RU
LOCAL_EMBEDDING_MODEL=multilingual-e5-small
EMBEDDING_DIMENSIONS=384

# LLM provider
# openai or claude
LLM_PROVIDER=openai
# or: gpt-4o-mini, claude-sonnet-4-6
LLM_MODEL=qwen2.5:7b
LLM_MAX_TOKENS=2048
LLM_TEMPERATURE=0.1

# API keys (set one based on LLM_PROVIDER)
# Only for LLM_PROVIDER=claude:
# ANTHROPIC_API_KEY=sk-ant-...
# Only for api.openai.com:
# OPENAI_LLM_API_KEY=sk-...
# OpenAI-compatible endpoint (Ollama); the key is optional when it is set
LLM_BASE_URL=http://host.docker.internal:11434/v1

# Models
# Docker image uses /app/models
# MODEL_CACHE_DIR=data/models
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
# Memory thresholds: cosine similarity; calibrate with tests/research/memory_eval.py
MEMORY_CONFLICT_SIMILARITY=0.82
MEMORY_RETRIEVAL_SIMILARITY=0.60
# Cross-encoder rerank of memory hits (false: keep the RRF order)
MEMORY_RERANK=true
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

12 tools:

| Tool | Description |
|---|---|
| `remember` | Save text as a memory fact (extracted by LLM, synchronous) |
| `recall` | Search memory facts (hybrid vector + keyword, no LLM), optional `scope`/`category` filter |
| `forget` | Delete a memory by ID |
| `context` | Get user profile summary (static + recent activity) |
| `observe` | Queue fact extraction from a conversation (async, returns a job id) |
| `check_memory_job` | Check an `observe` job by job_id: pending/processing/done (facts extracted)/error |
| `memories` | List stored memories, optionally filtered by category and `scope` |
| `search_documents` | Search indexed documents (vector chunk search, no LLM): `query`, optional `top_k` (default 5, at most 50); one line per chunk with document title, `doc_id`, chunk id and score |
| `index_text` | Index text content as a document (async, returns a job id) |
| `check_indexing` | Check indexing job status by job_id |
| `list_documents` | List all indexed documents |
| `delete_document` | Delete an indexed document by ID |

`remember`, `observe`, `recall` and `memories` take an optional `scope`: writes default to `default`, reads without `scope` cover all scopes. A blank or whitespace-only `scope` counts as not given. Separate projects or agents can keep their facts apart this way. Over HTTP, `GET /api/memory/list?scope=` and `GET /api/memory/context?scope=` take the same filter.

`recall` returns memory facts only: it does not call the LLM and does not search documents. Use `search_documents` for documents and `POST /api/query` for a synthesized answer. Scores in `recall` and `POST /api/memory/search` are raw cross-encoder logits when `MEMORY_RERANK=true` (they can be negative; compare them only within one answer) and RRF scores otherwise.

`observe` does not wait for the LLM. It stores the conversation and returns a job id; a background worker extracts the facts. Over HTTP, `POST /api/memory/observe` answers `202 Accepted` with `{"job_id": "...", "status": "pending"}`, and `GET /api/memory/jobs/{job_id}` returns `{job_id, status, facts_extracted, error}`. Sending the same conversation again in the same scope returns the same job instead of extracting it twice (a job that ended in `error` is retried with a new job; another scope gets its own job). A blank conversation is rejected with 400. When the conversation is already queued, the answer carries that job's current status (`pending`, `processing` or `done`) instead of `pending`. Jobs interrupted by a restart are queued again when the server starts. `remember` stays synchronous.

Extraction is at-least-once: a restart in the middle of a job extracts that conversation again from the start, and in a rolling deploy where the old and new containers overlap, the new one can reset a job the old one is still processing, so it runs twice. Conflict resolution usually merges the duplicate facts, but a few may be stored twice.

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

`host.docker.internal` resolves on Docker Desktop, and on Linux Engine through `extra_hosts: host.docker.internal:host-gateway` (already in `docker-compose.rust.yml`). For Ollama on another host, use that host's address. Outside Docker use `127.0.0.1`.

### Models and cache

- `MODEL_CACHE_DIR` - where ONNX models are stored. Default `data/models` for bare-binary runs; the Docker image bakes models into `/app/models` (set explicitly by `docker-compose.rust.yml`).
- `RERANKER_MODEL` - `jina-reranker-v2-base-multilingual` (default), `bge-reranker-base`, or `bge-reranker-v2-m3` (~2.3 GB).
- `memex --download-models` downloads both ONNX models (embedding and reranker) into `MODEL_CACHE_DIR` and exits. Use it to pre-warm offline hosts or build images.

---

## Evaluating memory quality

`tests/research/memory_eval.py` measures extraction and retrieval quality of a running server on 22 rows (11 Russian, 11 English; 4 conflict rows, 5 exact-token rows: an e-mail, two surnames, `MEMEX_URL`, package versions) from `tests/research/fixtures/memory_eval.jsonl`. It calls `remember`, matches extracted facts to the expected ones by token-F1 >= 0.6 (lowercase, whitespace tokens with edge punctuation stripped, so `MEMEX_URL` and `5.2.1` stay single tokens; exact-token rows additionally require the listed tokens verbatim in the fact), runs the row's search queries, and deletes everything in the scope afterwards (`--keep` to skip).

```bash
cd rust && cargo build --release && cd ..
set -a && source rust/.env.example && set +a
export MODEL_CACHE_DIR=$HOME/.cache/fastembed LLM_BASE_URL=http://127.0.0.1:11434/v1 LLM_MODEL=qwen2.5:7b
(cd rust && ./target/release/memex &)          # use a fresh rust/data
uv run python tests/research/memory_eval.py --base-url http://localhost:8000 --scope eval --model-label qwen2.5-7b
```

It prints `facts precision`, `facts recall`, `search hit@1`, `search hit@3`, `hit@k (of matched)` (only queries whose expected fact was extracted), `search precision@3` and `updates detected x/y`, and records its configuration (including `--conflict-sim`/`--retrieval-sim` labels). Unmatched expected facts count as misses in the unconditional `hit@k`. `--self-test` checks the scoring helpers and exits. A run writes its results JSON before cleanup, so a failed cleanup or an aborted run still leaves the measurements (with `error` and `aborted` set). `--search-only` skips `remember` and cleanup and reports no facts precision/recall. Results go to `tests/research/results/<date>T<HHMMSS>-<model-label>.json` (git-ignored). A run takes about 10 minutes with a 7B model. The exit code is always 0.

Baseline (2026-11-03, `qwen2.5:7b` via Ollama, `MEMORY_CONFLICT_SIMILARITY=0.82`, `MEMORY_RETRIEVAL_SIMILARITY=0.60`, `MEMORY_RERANK=true`, per-fact exact-token scoring; code at 6d2bddd + 9331ed5):

| metric | value |
|---|---|
| facts precision | 0.59 |
| facts recall | 0.70 |
| search hit@1 | 0.43 (13/30) |
| search hit@3 | 0.53 (16/30) |
| search hit@1 (of matched) | 0.57 (23 of 30 queries had a matched fact) |
| search hit@3 (of matched) | 0.70 |
| search precision@3 | 0.48 |
| updates detected | 4/4 |

A/B: reranking on vs off (same seed). One `--keep` run seeded the scope (the baseline above), then the same 44 facts were queried with `--search-only --seed-results <seed json>` against a server with `MEMORY_RERANK=true` and again with `MEMORY_RERANK=false` (23 queries had a matched fact):

| `MEMORY_RERANK` | hit@1 | hit@3 | hit@3 (of matched) | precision@3 |
|---|---|---|---|---|
| true (default) | 0.33 | 0.50 | 0.65 | 0.36 |
| false | 0.23 | 0.43 | 0.57 | 0.25 |

Reranking is a gain on this set: hit@1 +0.10, hit@3 +0.07, precision@3 +0.11, so it stays on by default. The set is small (30 queries, one extraction seed), so treat differences of a few queries as noise. The in-run baseline numbers are higher than the search-only ones because in a normal run each row's queries see only the facts of the rows processed so far, while `--search-only` queries the full scope with all distractors; compare only like with like.

Comparability: unconditional numbers between runs with different extraction outputs are not like-for-like (different facts get extracted, so different queries have something to find). To compare search configurations (thresholds, ranking changes, models) seed once with `--keep`, then run `--search-only --seed-results <seed results json>` per server configuration; this reuses the memory ids matched in the seed run. Without `--seed-results`, `--search-only` re-matches against the whole scope and is only comparable with other such runs.

Before reranking and with the older scoring (which split exact tokens on punctuation) the same set gave precision 0.67, recall 0.84, hit@1 0.50, hit@3 0.53; those numbers are not comparable with the table above. Extraction numbers vary between runs of a 7B model (a second run earlier gave 0.64 / 0.78 / updates 3/4). A `--search-only` re-run on the same seeded scope gives hit@1 0.27, hit@3 0.40 (of matched 0.36 / 0.55) and is repeatable, but it matches expected facts against the whole scope instead of the facts the row just extracted, so compare `--search-only` runs only with each other (as in the sweep below). gpt-4o-mini was not run (no OpenAI key available).

Retrieval threshold sweep before reranking (history; `--search-only` on 44 seeded facts, `MEMORY_CONFLICT_SIMILARITY=0.82`; the control differs slightly from the old baseline because of whole-scope matching, and the numbers use the older scoring):

| `MEMORY_RETRIEVAL_SIMILARITY` | hit@1 | hit@3 | precision@3 |
|---|---|---|---|
| 0.20 | 0.33 | 0.47 | 0.28 |
| 0.40 | 0.33 | 0.47 | 0.28 |
| 0.60 (control, default) | 0.33 | 0.47 | 0.28 |
| 0.80 | 0.33 | 0.43 | 0.34 |

Lowering the threshold changes nothing: with `multilingual-e5-small` the cosine similarity between a query and even unrelated short facts is well above 0.6, so the vector filter only starts to cut results around 0.8. The default stays at 0.60. The remaining misses come from ranking (the RRF merge of vector and BM25 hits puts generic "User ..." facts above the relevant one), not from the cut-off, so improving hit@3 needs a ranking change rather than a different threshold. Re-run the sweep with `--search-only` after any ranking change.

Calibrating the thresholds: change one variable at a time, restart the server on a fresh `rust/data`, re-run, and compare with the table above.

- `MEMORY_CONFLICT_SIMILARITY` (when a new fact is compared with existing ones for `updates`/`extends`): if `facts precision` drops or good facts get merged, the threshold is too low; if `updates detected` drops, it is too high.
- `MEMORY_RERANK` (`true` default): rerank memory search candidates with the cross-encoder. Compare `true` and `false` with the `--seed-results` A/B above.
- `MEMORY_RETRIEVAL_SIMILARITY` (minimum cosine similarity for search hits): if `hit@3` drops, the threshold may be too high, but see the sweep above: values up to 0.6 made no difference; if unrelated memories show up in results, it is too low.

---

## API

The REST API follows the Python version for documents, `/api/query` and the basic memory endpoints, but it is not identical: `POST /api/memory/observe` is asynchronous (see [MCP over HTTP](#mcp-over-http)), and the Rust version adds `scope` on memory endpoints, `GET /api/memory/jobs/{job_id}` and `POST /api/memory/search`. See [REST API docs](../README.md#rest-api) for the shared endpoints.

### `POST /api/memory/search`

Hybrid memory search (vector + BM25, fused with RRF, then reranked when `MEMORY_RERANK=true`); no LLM call. Every returned fact has its access count incremented.

Request body:

```json
{"query": "where does the user live", "scope": "work", "category": "personal", "top_k": 10}
```

`query` is required; a blank query returns 400. `scope` and `category` are optional filters (a blank `scope` means all scopes). `top_k` defaults to 10 and is clamped to 1..=100.

Response:

```json
{"hits": [{"id": "...", "content": "User lives in Novi Sad", "score": 3.42, "scope": "work",
           "category": "personal", "project": null, "source": "explicit",
           "created_at": "2026-11-03 10:00:00", "access_count": 2}]}
```

`score` is the cross-encoder logit when reranking is on (can be negative) and the RRF score when it is off.

### Known limitations

- Memory search fetches candidates from the whole index and applies the `scope`/`category` filter afterwards. With a filter it fetches at least 200 candidates per side (vector and keyword), so a scope holding a small share of a large memory store can still miss facts that rank below that cut. A filter inside the indexes is planned.

---

## Resource requirements

| | Min | Recommended |
|---|---|---|
| **RAM** | 3 GB | 4 GB+ |
| **Storage** | 2 GB (models cache) | 4 GB+ |
| **CPU** | ARMv8 / x86_64 | Any modern core |
| **OS** | Linux | Linux |

RAM: about 2.3 GiB measured after the first query on the arm64 Docker build (both ONNX models loaded), hence the 3 GB minimum. Measured idle after startup on arm64; a reranked search on a real corpus may use more.

Models (~1.2 GB with the default reranker) are cached in `MODEL_CACHE_DIR`: `/app/models` in Docker (baked into the image), `data/models` for the binary.

---

## Versioning

Rust releases are versioned independently from Python:
- Python releases: `v2.x.x` tags
- Rust releases: `rust/v3.x.x` tags
- Docker images: `ghcr.io/isqanderm/memex:rust-3.x.x` and `rust-latest` (built by `docker.yml` on `rust/v*` tags; pre-releases get only the versioned tag)

See [rust/CHANGELOG.md](../rust/CHANGELOG.md) for Rust-specific changes.
