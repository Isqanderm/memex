# ── Stage 1: Builder ─────────────────────────────────────────────────────────
# Docker buildx + QEMU handles multi-platform transparently —
# each platform variant runs natively, no cross-compilation needed.
FROM rust:slim-trixie AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    g++ \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependencies
COPY rust/Cargo.toml rust/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release 2>&1 || true
RUN rm -rf src

# Build the actual application
COPY rust/src ./src
RUN touch src/main.rs
RUN cargo build --release

# ── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y \
    poppler-utils \
    ca-certificates \
    curl \
    libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/memex ./memex
COPY rust/templates ./templates
COPY rust/static    ./static

# Bake ONNX models into the image so the container starts without network.
# Kept outside /app/data, which compose bind-mounts.
ENV MODEL_CACHE_DIR=/app/models
RUN LLM_PROVIDER=openai LLM_MODEL=warmup LLM_BASE_URL=http://unused \
    ./memex --download-models

RUN mkdir -p data/uploads data/tantivy

EXPOSE 8000

ENTRYPOINT ["./memex"]
