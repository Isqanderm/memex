# ── Stage 1: Builder ─────────────────────────────────────────────────────────
FROM --platform=$BUILDPLATFORM rust:1.82-slim AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
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
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    poppler-utils \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/memex ./memex
COPY rust/templates ./templates
COPY rust/static    ./static

RUN mkdir -p data/uploads data/tantivy

EXPOSE 8000

ENTRYPOINT ["./memex"]
