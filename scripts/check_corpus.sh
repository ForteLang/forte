#!/usr/bin/env bash
# Corpus gate: every library and song in the repo must compile, and every
# pattern/example must actually render (build) without errors.
# Needs: cargo build --release -p fortelang
set -euo pipefail
cd "$(dirname "$0")/.."

FORTE=target/release/forte
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
fail=0

echo "== corpus: check every .forte =="
for f in lib/std/*.forte songs/devices/*.forte songs/*.forte \
         songs/patterns/*.forte songs/examples/*.forte; do
  if ! $FORTE check "$f" > /dev/null 2>&1; then
    echo "   FAIL check: $f" >&2
    $FORTE check "$f" >&2 || true
    fail=1
  fi
done
echo "   OK: all sources compile"

echo "== corpus: build every pattern/example =="
for f in songs/patterns/*.forte songs/examples/*.forte; do
  if ! $FORTE build "$f" -o "$SCRATCH/out.wav" > /dev/null 2>&1; then
    echo "   FAIL build: $f" >&2
    $FORTE build "$f" -o "$SCRATCH/out.wav" >&2 || true
    fail=1
  fi
done
echo "   OK: all patterns/examples render"

exit $fail
