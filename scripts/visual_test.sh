#!/usr/bin/env bash
# Visual regression harness.
#
#   scripts/visual_test.sh update   # (re)capture golden screenshots
#   scripts/visual_test.sh check    # capture and compare against goldens
#
# Each "state" launches the app with env hooks that open a deterministic view
# (no playback, meters idle), captures the screen under Xvfb with software GL,
# and compares pixels with ImageMagick. A state fails when more than
# MAX_DIFF_PCT of pixels differ (beyond a small per-pixel fuzz), which catches
# layout drift — like track-lane misalignment — while tolerating antialiasing.
#
# Requirements: Xvfb, imagemagick (import/compare), a built release binary.

set -uo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-check}"
BIN=target/release/bitwig-clone
GOLDEN=tests/visual/golden
CURRENT=target/visual/current
DIFF=target/visual/diff
DISPLAY_NUM=":97"
SIZE_W=1280
SIZE_H=800
SETTLE_SECS="${VISUAL_SETTLE:-5}"
MAX_DIFF_PCT="0.4"

# state name -> extra env (the app reads these hooks at startup)
declare -A STATES=(
  [arrange]=""
  [launcher]="BITWIG_VIEW=launcher"
  [mix]="BITWIG_VIEW=mix"
  [pianoroll]="BITWIG_EDIT=1"
  [grid]="BITWIG_GRID=1"
  [small]="BITWIG_WINDOW=960x620"
)

command -v Xvfb >/dev/null || { echo "Xvfb not found"; exit 2; }
command -v import >/dev/null || { echo "imagemagick not found"; exit 2; }
[ -x "$BIN" ] || { echo "build first: cargo build --release -p dawapp"; exit 2; }

mkdir -p "$GOLDEN" "$CURRENT" "$DIFF"

export LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe

Xvfb "$DISPLAY_NUM" -screen 0 "${SIZE_W}x${SIZE_H}x24" >/dev/null 2>&1 &
XVFB_PID=$!
trap 'kill $XVFB_PID 2>/dev/null' EXIT
sleep 1
export DISPLAY="$DISPLAY_NUM"

capture() {
  local name="$1" envs="$2" out="$3"
  env $envs "$BIN" >/dev/null 2>&1 &
  local app=$!
  sleep "$SETTLE_SECS"
  import -window root "$out" 2>/dev/null
  kill "$app" 2>/dev/null
  wait "$app" 2>/dev/null
  sleep 0.5
}

fail=0
total_px=$((SIZE_W * SIZE_H))
threshold=$(python3 -c "print(int($total_px * $MAX_DIFF_PCT / 100))")

for name in "${!STATES[@]}"; do
  cur="$CURRENT/$name.png"
  capture "$name" "${STATES[$name]}" "$cur"
  if [ ! -s "$cur" ]; then
    echo "✗ $name: capture failed"
    fail=1
    continue
  fi

  if [ "$MODE" = "update" ]; then
    cp "$cur" "$GOLDEN/$name.png"
    echo "✓ $name: golden updated"
    continue
  fi

  gold="$GOLDEN/$name.png"
  if [ ! -f "$gold" ]; then
    echo "✗ $name: no golden (run 'scripts/visual_test.sh update')"
    fail=1
    continue
  fi
  # AE = absolute error count; fuzz tolerates AA/gamma wiggle per pixel
  diff_px=$(compare -metric AE -fuzz 3% "$gold" "$cur" "$DIFF/$name.png" 2>&1 | grep -oE '^[0-9]+' || echo "$total_px")
  if [ "${diff_px:-$total_px}" -le "$threshold" ]; then
    echo "✓ $name: ${diff_px} px differ (≤ ${threshold})"
  else
    echo "✗ $name: ${diff_px} px differ (> ${threshold}) — see $DIFF/$name.png"
    fail=1
  fi
done

if [ "$MODE" = "check" ]; then
  [ $fail -eq 0 ] && echo "ALL VISUAL TESTS PASSED" || echo "VISUAL REGRESSIONS DETECTED"
fi
exit $fail
