#!/usr/bin/env bash
# Build the browser editor: compile forteweb to wasm and place it next to the
# static page. Serve with:  python3 -m http.server -d . 8000  →  /web/
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -q -p forteweb --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/forteweb.wasm web/forte.wasm
echo "web/forte.wasm $(stat -c%s web/forte.wasm) bytes — serve the repo root and open /web/"
