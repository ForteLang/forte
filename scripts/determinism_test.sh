#!/usr/bin/env bash
# Determinism gate (Forte rule D-11 / roadmap Phase 0.4): render the demo
# project on native x86_64 and wasm32-wasip1 and require bit-identical f32
# output. Needs: rustup target wasm32-wasip1, Node >= 20.
set -euo pipefail
cd "$(dirname "$0")/.."

SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

echo "== native =="
cargo run --release -q -p dawcore --example determinism | tee "$SCRATCH/native.txt"

echo "== wasm32-wasip1 =="
cargo build --release -q -p dawcore --example determinism --target wasm32-wasip1
node --no-warnings scripts/run-wasi.mjs \
  target/wasm32-wasip1/release/examples/determinism.wasm "$SCRATCH" \
  | tee "$SCRATCH/wasm.txt"

native_digest=$(grep 'f32 digest' "$SCRATCH/native.txt" | awk '{print $4}')
wasm_digest=$(grep 'f32 digest' "$SCRATCH/wasm.txt" | awk '{print $4}')

if [ "$native_digest" = "$wasm_digest" ]; then
  echo "OK: bit-identical across targets (f32 digest $native_digest)"
else
  echo "FAIL: native=$native_digest wasm=$wasm_digest" >&2
  exit 1
fi
