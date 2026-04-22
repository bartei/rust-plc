# Playwright UI test environment.
# Expects pre-built artifacts mounted or copied:
#   - editors/vscode/out/webview/ (webview bundle)
#   - target/debug/monitor-test-server (test WS server)
#
# Build from repo root: docker build -f docker/playwright.Dockerfile -t rust-plc-playwright .

# Stage 1: Build the monitor-test-server on glibc
FROM rust:1-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY stdlib/ ./stdlib/
RUN cargo build -p st-monitor --bin monitor-test-server --release 2>&1

# Stage 2: Playwright tests
FROM mcr.microsoft.com/playwright:v1.52.0-noble
WORKDIR /work

# Copy built webview output
COPY editors/vscode/out/webview/ ./editors/vscode/out/webview/

# Copy test files
COPY editors/vscode/test/ui/playwright.config.js ./editors/vscode/test/ui/
COPY editors/vscode/test/ui/monitor-panel.spec.js ./editors/vscode/test/ui/
COPY editors/vscode/test/ui/serve-production.js ./editors/vscode/test/ui/
COPY editors/vscode/test/ui/vscode-api-shim.js ./editors/vscode/test/ui/

# Install Playwright test dependency pinned to match the Docker image
RUN cd editors/vscode/test/ui && npm init -y && npm install @playwright/test@1.52.0

# Copy the glibc-linked binary from the builder
COPY --from=builder /build/target/release/monitor-test-server ./target/debug/monitor-test-server

WORKDIR /work/editors/vscode/test/ui
CMD ["npx", "playwright", "test"]
