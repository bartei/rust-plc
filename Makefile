.PHONY: help build clippy test test-rust test-modbus test-unit test-ui \
       test-e2e-x86 test-e2e-arm test-all \
       vscode-compile vscode-webview vscode-build \
       build-static build-static-arm docker-test-image clean

TEST_IMAGE     := rust-plc-test
PW_IMAGE       := rust-plc-playwright
DOCKER_RUN     := docker run --rm $(TEST_IMAGE)

# ── Default ──────────────────────────────────────────────────────────

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-28s\033[0m %s\n", $$1, $$2}'

# ── Docker images ────────────────────────────────────────────────────

docker-test-image: ## Build the Rust/Node test Docker image
	docker build -f docker/test.Dockerfile -t $(TEST_IMAGE) .

docker-pw-image: ## Build the Playwright test Docker image
	docker build -f docker/playwright.Dockerfile -t $(PW_IMAGE) .

# ── Build (in Docker) ───────────────────────────────────────────────

build: docker-test-image ## Build all Rust crates
	$(DOCKER_RUN) cargo build --workspace --all-targets

clippy: docker-test-image ## Run clippy with deny warnings
	$(DOCKER_RUN) cargo clippy --workspace --all-targets -- -D warnings

vscode-compile: docker-test-image ## Compile the VS Code extension (TypeScript)
	$(DOCKER_RUN) sh -c "cd editors/vscode && npm install --ignore-scripts && npm run compile"

vscode-webview: docker-test-image ## Build the Preact webview bundle
	$(DOCKER_RUN) sh -c "cd editors/vscode && npm install --ignore-scripts && npm run build:webview"

vscode-build: docker-test-image ## Build the full VS Code extension
	$(DOCKER_RUN) sh -c "cd editors/vscode && npm install --ignore-scripts && npm run compile && npm run build:webview"

build-static: ## Build static musl binaries (x86_64)
	./scripts/build-static.sh x86_64

build-static-arm: ## Build static musl binaries (aarch64)
	./scripts/build-static.sh aarch64

# ── Test ─────────────────────────────────────────────────────────────

test: docker-test-image ## Run clippy + Rust tests + webview unit tests
	$(DOCKER_RUN) sh -c "\
		cargo clippy --workspace --all-targets -- -D warnings && \
		cargo build -p st-cli && \
		cargo test --workspace --exclude st-comm-modbus --exclude st-comm-serial && \
		cd editors/vscode && npm install --ignore-scripts && npm run compile && npm run build:webview && \
		node test/monitor-tree.test.js"

test-rust: docker-test-image ## Run all Rust tests (except modbus/serial)
	$(DOCKER_RUN) sh -c "cargo build -p st-cli && cargo test --workspace --exclude st-comm-modbus --exclude st-comm-serial"

test-modbus: docker-test-image ## Run modbus/serial integration tests (requires socat)
	$(DOCKER_RUN) cargo test -p st-comm-serial -p st-comm-modbus -- --test-threads=1

test-unit: docker-test-image ## Run webview unit tests (monitor-tree)
	$(DOCKER_RUN) sh -c "cd editors/vscode && npm install --ignore-scripts && npm run compile && npm run build:webview && node test/monitor-tree.test.js"

test-ui: docker-pw-image ## Run Playwright UI tests in Docker
	docker run --rm $(PW_IMAGE)

test-e2e-x86: docker-test-image ## Run E2E tests on x86_64 QEMU (requires VM images)
	$(DOCKER_RUN) sh -c "ST_E2E_QEMU=1 cargo test -p st-target-agent --test e2e_qemu -- --test-threads=1"

test-e2e-arm: docker-test-image ## Run E2E tests on aarch64 QEMU (requires VM images)
	$(DOCKER_RUN) sh -c "ST_E2E_QEMU=1 ST_E2E_AARCH64=1 cargo test -p st-target-agent --test e2e_qemu e2e_aarch64 -- --test-threads=1 --nocapture"

test-all: test test-modbus test-ui ## Run everything (except QEMU E2E)

# ── Clean ────────────────────────────────────────────────────────────

clean: ## Remove build artifacts
	cargo clean 2>/dev/null || true
	rm -rf editors/vscode/out
	docker rmi $(TEST_IMAGE) $(PW_IMAGE) 2>/dev/null || true
