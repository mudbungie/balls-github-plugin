#!/usr/bin/env bash
# Run cargo-tarpaulin and fail if line coverage < 100%.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

if ! command -v cargo-tarpaulin >/dev/null 2>&1; then
    echo "cargo-tarpaulin not found. Install with: cargo install cargo-tarpaulin"
    exit 1
fi

THRESHOLD="${COVERAGE_THRESHOLD:-100}"

echo "Running cargo tarpaulin (threshold ${THRESHOLD}%)..."
cargo tarpaulin \
    --workspace \
    --engine llvm \
    --skip-clean \
    --fail-under "$THRESHOLD" \
    --out Stdout \
    --exclude-files 'target/*' \
    --timeout 120 \
    --color never \
    2>&1 | tail -40
