#!/usr/bin/env bash
# Edit→sound latency gate (issue #2 / SRS-LANG-007): a one-line edit must
# reach the ear within 1 second. The live paths (forte play --watch, the
# browser editor) recompile the whole song and hot-swap the running engine;
# the compile is the only non-constant cost, so that is what we budget.
#
# Method: for every album song (the largest content in the repo), run
# `forte check` three times warm and take the best. The budget per song is
# 1000ms MINUS a generous 200ms allowance for the engine swap + process
# overhead — so the gate trips long before a listener could notice.
set -euo pipefail
cd "$(dirname "$0")/.."

FORTE=target/release/forte
[ -x "$FORTE" ] || cargo build --release -p fortelang --bin forte
BUDGET_MS=800
fail=0

songs=$(ls ../packages/*/songs/*.forte)
# warm the page cache so we measure compilation, not disk
for f in $songs; do $FORTE check "$f" > /dev/null 2>&1 || true; done

echo "== latency: edit→compile per song (budget ${BUDGET_MS}ms) =="
for f in $songs; do
  best=999999
  for _ in 1 2 3; do
    s=$(date +%s%N)
    $FORTE check "$f" > /dev/null 2>&1 || true
    e=$(date +%s%N)
    ms=$(( (e - s) / 1000000 ))
    [ "$ms" -lt "$best" ] && best=$ms
  done
  if [ "$best" -gt "$BUDGET_MS" ]; then
    echo "   FAIL ${best}ms  $f"
    fail=1
  else
    echo "   ok   ${best}ms  $f"
  fi
done

if [ "$fail" = 1 ]; then
  echo "NG: 編集→音のレイテンシ予算(${BUDGET_MS}ms)を超えた曲があります"
  exit 1
fi
echo "OK: すべての曲が編集→音 1 秒以内(コンパイルは予算 ${BUDGET_MS}ms 内)"
