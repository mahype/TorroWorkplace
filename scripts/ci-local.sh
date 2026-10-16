#!/usr/bin/env bash
#
# ci-local.sh – runs the checks of .github/workflows/ci.yml on this machine.
#
# Why: GitHub Actions only runs for releases (tags) because every runner minute is billed. Instead the
# checks run here, before a commit (fast) and before a push (complete), via the hooks in .githooks/.
# Linux and Windows are only covered by the release run; this script checks the host system.
#
# Usage:
#   scripts/ci-local.sh           fmt, clippy, design check, tests (same order as ci.yml)
#   scripts/ci-local.sh --quick   fmt and design check only (seconds, used by pre-commit)
#
# Skip once (e.g. a WIP push): git push --no-verify, git commit --no-verify.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }

step "cargo fmt --all --check"
cargo fmt --all --check

step "design check"
scripts/design-check.sh --self-test
scripts/design-check.sh --strict

if [[ "${1:-}" == "--quick" ]]; then
    exit 0
fi

# The hook scripts of the issue workflow (docs/issue-workflow.md) are JavaScript, not Rust.
if command -v node >/dev/null 2>&1; then
    step "node --test scripts/agent/tests"
    node --test scripts/agent/tests/*.test.mjs
fi

step "cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

step "cargo test --workspace"
cargo test --workspace

printf '\n\033[1;32mAll CI checks passed.\033[0m\n'
