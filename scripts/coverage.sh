#!/usr/bin/env bash
# Measures workspace test coverage with proper child-process instrumentation.
#
# This script is the single source of truth for how coverage is collected
# locally and in CI. Two things make it work where a plain `cargo llvm-cov`
# does not:
#
#  1. `cargo llvm-cov show-env` is sourced into the current shell, then
#     `cargo build -p st-cli -p st-target-agent` produces *instrumented*
#     binaries at the standard `target/debug/<bin>` location. Integration
#     tests that spawn these binaries (lsp_integration, api_integration,
#     dap_*_integration, e2e_qemu, ...) then inherit the LLVM_PROFILE_FILE
#     env var and write per-PID profraw files alongside the parent's.
#
#  2. The Modbus/serial RTU integration tests require `socat` and
#     `--test-threads=1`. We hard-fail (rather than silently skip) when
#     `socat` is missing so a CI regression cannot quietly drop those
#     ~1.4k lines of comm-stack coverage.
#
# Usage:
#   ./scripts/coverage.sh                 # lcov.info + summary
#   ./scripts/coverage.sh --html          # also writes HTML
#   ./scripts/coverage.sh --no-comm       # skip comm tests (faster local loop)
#   ./scripts/coverage.sh --fail-under N  # exit non-zero if line % < N
#
# Outputs:
#   target/llvm-cov/lcov.info             # LCOV format, for codecov/coveralls
#   target/llvm-cov/coverage.json         # JSON summary
#   target/llvm-cov/html/index.html       # HTML report (when --html)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

WANT_HTML=0
WANT_COMM=1
FAIL_UNDER=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --html)        WANT_HTML=1; shift ;;
    --no-comm)     WANT_COMM=0; shift ;;
    --fail-under)  FAIL_UNDER="$2"; shift 2 ;;
    *) echo "unknown flag: $1" >&2; exit 2 ;;
  esac
done

# ── Pre-flight checks ─────────────────────────────────────────────────────
if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "ERROR: cargo-llvm-cov is not installed."
  echo "Install with: cargo install cargo-llvm-cov --locked"
  exit 1
fi

if [[ "$WANT_COMM" -eq 1 ]] && ! command -v socat >/dev/null 2>&1; then
  echo "ERROR: socat not found on PATH; the Modbus/serial integration tests need it."
  echo "  - CI: apt-get install -y socat"
  echo "  - Local nix: nix-shell -p socat openssl pkg-config systemdLibs --run './scripts/coverage.sh'"
  echo "  - Skip those tests with: ./scripts/coverage.sh --no-comm"
  exit 1
fi

mkdir -p target/llvm-cov

# ── Coverage collection ──────────────────────────────────────────────────
echo "==> Cleaning previous coverage data"
cargo llvm-cov clean --workspace

# Source the env so child processes inherit instrumentation flags.
# `show-env --export-prefix` (alias `--sh`) prints `export VAR=...` lines.
echo "==> Setting up coverage env (show-env)"
# shellcheck disable=SC1090
source <(cargo llvm-cov show-env --export-prefix)

echo "==> Building instrumented binaries that integration tests spawn"
# These are the binaries spawned by `Command::new()` in integration tests:
#   - st-cli         → spawned by lsp_integration ("st-cli serve") and dap_*
#   - st-target-agent → spawned by api_integration / e2e_qemu / online-update
cargo build -p st-cli -p st-target-agent

echo "==> Running workspace tests (excluding comm RTU)"
cargo test --workspace \
  --exclude st-comm-modbus \
  --exclude st-comm-serial

if [[ "$WANT_COMM" -eq 1 ]]; then
  echo "==> Running comm/serial tests (single-threaded, requires socat)"
  ST_REQUIRE_SOCAT=1 cargo test -p st-comm-serial -p st-comm-modbus -- --test-threads=1
fi

# ── Reports ──────────────────────────────────────────────────────────────
echo "==> Writing LCOV + JSON + summary"
cargo llvm-cov report --summary-only | tee target/llvm-cov/summary.txt
cargo llvm-cov report --lcov --output-path target/llvm-cov/lcov.info
cargo llvm-cov report --json --summary-only --output-path target/llvm-cov/coverage.json

if [[ "$WANT_HTML" -eq 1 ]]; then
  echo "==> Writing HTML report"
  cargo llvm-cov report --html --output-dir target/llvm-cov/html
  echo "    Open: target/llvm-cov/html/index.html"
fi

# ── Optional gate ────────────────────────────────────────────────────────
if [[ -n "$FAIL_UNDER" ]]; then
  line_pct=$(jq -r '.data[0].totals.lines.percent | floor' target/llvm-cov/coverage.json)
  echo "==> Line coverage: ${line_pct}%  (gate: ${FAIL_UNDER}%)"
  if [[ "$line_pct" -lt "$FAIL_UNDER" ]]; then
    echo "ERROR: line coverage ${line_pct}% < gate ${FAIL_UNDER}%"
    exit 1
  fi
fi

echo "==> Done. lcov.info: target/llvm-cov/lcov.info"
