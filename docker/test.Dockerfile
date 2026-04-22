# Rust test environment with all system dependencies.
# Builds from the repo root: docker build -f docker/test.Dockerfile -t rust-plc-test .

FROM rust:1-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libudev-dev \
    libssl-dev \
    socat \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js 22 for webview tests
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add clippy

WORKDIR /work
COPY . .

# Pre-fetch Rust dependencies
RUN cargo fetch
