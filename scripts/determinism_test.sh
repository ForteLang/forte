#!/usr/bin/env bash
# Determinism gate (Forte rule D-11 / roadmap Phase 0.4): the same sources must
# render bit-identical audio on native x86_64 and wasm32-wasip1.
#   Gate 1: dawcore demo project (engine-level)
#   Gate 2: forte build of the reference song (language-level, end to end)
# Needs: rustup target wasm32-wasip1, Node >= 20.
set -euo pipefail
cd "$(dirname "$0")/.."

SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

fail=0

echo "== gate 1: dawcore engine =="
cargo run --release -q -p dawcore --example determinism > "$SCRATCH/native.txt"
cargo build --release -q -p dawcore --example determinism --target wasm32-wasip1
node --no-warnings scripts/run-wasi.mjs \
  target/wasm32-wasip1/release/examples/determinism.wasm \
  "{\"/scratch\":\"$SCRATCH\"}" '["determinism","/scratch/engine.f32"]' > "$SCRATCH/wasm.txt"
n=$(grep 'f32 digest' "$SCRATCH/native.txt" | awk '{print $4}')
w=$(grep 'f32 digest' "$SCRATCH/wasm.txt" | awk '{print $4}')
if [ "$n" = "$w" ]; then
  echo "   OK: engine bit-identical ($n)"
else
  echo "   FAIL: native=$n wasm=$w" >&2
  fail=1
fi

echo "== gate 2: forte build (songs/first-light.forte) =="
cargo run --release -q -p fortelang --bin forte -- \
  build songs/first-light.forte -o "$SCRATCH/native.wav" > "$SCRATCH/forte-native.txt"
cargo build --release -q -p fortelang --bin forte --target wasm32-wasip1
node --no-warnings scripts/run-wasi.mjs \
  target/wasm32-wasip1/release/forte.wasm \
  "{\"/proj\":\".\",\"/scratch\":\"$SCRATCH\"}" \
  '["forte","build","/proj/songs/first-light.forte","-o","/scratch/wasm.wav"]' > "$SCRATCH/forte-wasm.txt"
n=$(grep 'digest' "$SCRATCH/forte-native.txt" | awk '{print $3}')
w=$(grep 'digest' "$SCRATCH/forte-wasm.txt" | awk '{print $3}')
if [ "$n" = "$w" ]; then
  echo "   OK: forte build bit-identical ($n)"
else
  echo "   FAIL: native=$n wasm=$w" >&2
  fail=1
fi

exit $fail
