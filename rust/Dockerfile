# ── Stage 1: Builder ─────────────────────────────────────────────────────────
FROM --platform=$BUILDPLATFORM rust:1.85-slim AS builder

# Accept build target (default: native/host architecture)
ARG TARGETARCH

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# For ARM64 cross-compilation from amd64
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    apt-get update && apt-get install -y gcc-aarch64-linux-gnu && \
    rustup target add aarch64-unknown-linux-gnu; \
    fi

WORKDIR /app

# Cache dependencies
COPY rust/Cargo.toml rust/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs

# Build with correct target
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    cargo build --release --target=aarch64-unknown-linux-gnu; \
    else \
    cargo build --release; \
    fi 2>&1 || true
RUN rm -rf src

# Build the actual application
COPY rust/src ./src
RUN touch src/main.rs

RUN if [ "$TARGETARCH" = "arm64" ]; then \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    cargo build --release --target=aarch64-unknown-linux-gnu; \
    else \
    cargo build --release; \
    fi

# Copy binary to a known location regardless of target
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    cp target/aarch64-unknown-linux-gnu/release/memex /app/memex-binary; \
    else \
    cp target/release/memex /app/memex-binary; \
    fi

# ── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    poppler-utils \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/memex-binary ./memex
COPY rust/templates ./templates
COPY rust/static    ./static

RUN mkdir -p data/uploads data/tantivy

EXPOSE 8000

ENTRYPOINT ["./memex"]
