#!/usr/bin/env bash
# The merge gate, run locally (GitHub Actions is off to stay in the free tier).
# Run this before merging to main — all four jobs the old CI ran:
#
#   scripts/ci_local.sh          # everything
#   scripts/ci_local.sh quick    # tests + clippy + determinism (skip corpus/E2E)
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== 1/4 cargo test + clippy =="
cargo test --release -p dawcore -p fortelang
cargo clippy --release -p dawcore -p fortelang --all-targets -- -D warnings

echo "== 2/4 determinism gate =="
scripts/determinism_test.sh

if [ "${1:-}" = "quick" ]; then
  echo "OK (quick): tests + clippy + determinism"
  exit 0
fi

echo "== 3/4 corpus =="
scripts/check_corpus.sh

echo "== 4/4 web + hub E2E =="
if command -v node >/dev/null 2>&1 && [ -d node_modules/playwright ]; then
  node scripts/web_e2e.mjs
  node scripts/hub_e2e.mjs
else
  echo "skip: playwright なし(node scripts/web_e2e.mjs を手動で)"
fi

echo "OK: 全ゲート通過 — マージしてよし"
