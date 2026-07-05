#!/usr/bin/env bash
# Corpus gate: every library and song in the repo must compile, and every
# pattern/example must actually render (build) without errors.
#
# Render scope (issue #19): checking is always repo-wide (cheap), but the
# expensive render step narrows to changed songs when CORPUS_BASE is set to a
# git ref (PRs set origin/main). A change under lib/ or crates/ re-renders
# everything, since any song may depend on it.
# Needs: cargo build --release -p fortelang
set -euo pipefail
cd "$(dirname "$0")/.."

FORTE=target/release/forte
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
fail=0

echo "== corpus: check every .forte =="
for f in ../packages/*/instruments/*.forte ../packages/*/songs/devices/*.forte \
         ../packages/*/blocks/*.forte ../packages/*/songs/*.forte \
         ../packages/*/songs/*/*.forte ../packages/*/package.forte; do
  if ! $FORTE check "$f" > /dev/null 2>&1; then
    echo "   FAIL check: $f" >&2
    $FORTE check "$f" >&2 || true
    fail=1
  fi
done
echo "   OK: all sources compile"

render_all=1
if [ -n "${CORPUS_BASE:-}" ]; then
  changed="$(git diff --name-only "$CORPUS_BASE"...HEAD 2>/dev/null || echo ALL)"
  if [ "$changed" != "ALL" ] && ! grep -qE '^(packages/[^/]+/instruments/|forte/crates/|forte/scripts/)' <<< "$changed"; then
    render_all=0
  fi
fi

echo "== corpus: build patterns/examples (full=$render_all) =="
for f in ../packages/*/blocks/*.forte ../packages/*/songs/*.forte; do
  if [ "$render_all" = 0 ] && ! grep -qx "$f" <<< "${changed:-}"; then
    continue
  fi
  if ! $FORTE build "$f" -o "$SCRATCH/out.wav" > /dev/null 2>&1; then
    echo "   FAIL build: $f" >&2
    $FORTE build "$f" -o "$SCRATCH/out.wav" >&2 || true
    fail=1
  fi
done
echo "   OK: selected patterns/examples render"

exit $fail
