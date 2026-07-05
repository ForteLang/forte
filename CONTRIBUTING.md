# Contributing to Forte

Issues and PRs are welcome. This document covers development setup and the
rules for getting a PR merged — above all, the **determinism gate**.

## Development setup

You need:

- Rust (stable) + wasm targets
- Node.js >= 20 (runs the wasm side of the determinism gate, and the E2E tests)
- On Linux: ALSA headers (`forte play` depends on cpal)

```bash
# Linux
sudo apt install libasound2-dev

rustup target add wasm32-wasip1 wasm32-unknown-unknown

cargo build --release -p fortelang   # the `forte` CLI → target/release/forte
scripts/build_web.sh                 # browser editor (web/forte.wasm)

# for the E2E tests
npm i playwright
npx playwright install chromium
```

## Testing

Before opening a PR, run the merge gate locally (GitHub Actions is
intentionally off — the gate runs on the maintainer's machine before every
merge):

```bash
scripts/ci_local.sh          # the full gate
scripts/ci_local.sh quick    # tests + clippy + determinism only

# or piece by piece:
cargo test -p dawcore -p fortelang     # engine + language + hub + REPL
scripts/determinism_test.sh            # native/wasm bit-identity gate
node scripts/web_e2e.mjs               # browser E2E (playwright + chromium)
node scripts/hub_e2e.mjs               # hub E2E
scripts/check_corpus.sh                # every instrument & song renders
```

## The determinism gate — the most important rule here

Forte's promise is that the same commit renders **bit-identical audio** on
native, wasm, and in the browser. To keep that promise:

- For **changes that shouldn't affect the sound** (refactors, UI, docs), the
  build digests of the reference songs must not move by a single bit.
  `scripts/determinism_test.sh` is the gate.
- For **changes that intentionally affect the sound** (DSP fixes, new nodes,
  engine changes), explain **why the digests change** in the PR description,
  and update the expected digests embedded in the E2E scripts
  (`NATIVE_DIGEST` etc. in `scripts/web_e2e.mjs`) in the same PR.
- Be careful with floating point: use `libm` for math functions (platform libc
  differences shift bits), never depend on HashMap iteration order, and don't
  introduce `fast-math`-style optimizations.
- If you need randomness, use a deterministic PRNG only (seeded xorshift or
  similar). Never let `Date.now()`, OS randomness, or thread timing feed the
  audio path.

## PR rules

- One PR = one topic, kept small. Discuss large design changes in an issue
  first.
- Behavior changes come with tests (Rust unit tests or E2E).
- `scripts/ci_local.sh` must pass (run locally; GitHub Actions is off).
- Changes to the language also update the relevant part of
  `docs/webdaw/spec/`. The spec and docs are part of the product.
- Commit messages: first line says what and why. Japanese or English both
  welcome.

## Filing issues

- Templates are provided for bug reports and feature requests.
- Tasks from the roadmap (`docs/webdaw/06-roadmap.md`) are usually already
  filed — search existing issues first.

## Code of conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
