#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
export CODEX_INTERNAL_ORIGINATOR_OVERRIDE="codex_cli_rs"

run_step() {
  local label="$1"
  shift
  echo "==> ${label}"
  "$@"
  echo
}

run_step "clippy: codex-core lib" cargo clippy -p codex-core --lib
run_step "clippy: codex-core tests" cargo clippy -p codex-core --tests
run_step "clippy: codex-core all-features tests" cargo clippy -p codex-core --all-features --tests

run_step "clippy: codex-cli lib" cargo clippy -p codex-cli --lib
run_step "clippy: codex-cli tests" cargo clippy -p codex-cli --tests
run_step "clippy: codex-cli all-features tests" cargo clippy -p codex-cli --all-features --tests

run_step "tests: codex-core" cargo test -p codex-core
run_step "tests: codex-cli" cargo test -p codex-cli
