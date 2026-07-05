# Determinism Spike Results (Phase 0.4)

Date: 2026-07-02 / Result: **Success — bit-identical rendering achieved on native and wasm32**
Requirements addressed: SYS-ENG-001, SRS-CORE-003 (D-11)

## Method

The existing dawcore demo project (synth + sampler + effects + modulators +
a 20-second arrangement without metro noise) was rendered offline with the same
engine (the same path as `bounce`), and digests of the f32 bit patterns of all
samples (FNV-1a 64) were compared.

- native: x86_64-unknown-linux-gnu (rustc 1.94.1, release)
- wasm: wasm32-wasip1 (same rustc), executed in Node 22's WASI
- Reproduction: `scripts/determinism_test.sh` (verification code: `crates/dawcore/examples/determinism.rs`)

## Results

| Stage | f32 digest (native / wasm) | Match |
| --- | --- | --- |
| Before fix (std float methods) | `a287cd7994449b0a` / `52b1fa18e9084db2` | ✗ |
| After fix (unified libm) | `aa68277c9dbb8161` / `aa68277c9dbb8161` | **✓ bit-identical** |

The reality of the pre-fix mismatch (out of 1,920,000 samples):
- 63.9% bit mismatch, but **maximum absolute difference 1.49e-7 (≈ -136 dBFS, inaudible)**
- After 16-bit quantization, only 0.018% differ by 1 LSB
- Cause: `f32::sin/cos/tan/exp/tanh/powf` resolve to glibc on native and to
  compiler-builtins on wasm, and the implementations differ (the first divergence
  occurred at sample 6)

## Fix

Created `crates/dawcore/src/dmath.rs`, pinning the 6 transcendental functions to the
pure-Rust `libm` crate. Replaced the 18 call sites in the DSP with `crate::dmath::*`.
`sqrt/abs/floor/round/fract/min/max` are IEEE-exact (identical on all targets) and
needed no change. All 13 existing tests pass.

## Conclusions and Implications

1. **SYS-ENG-001 (cross-target determinism) is achievable**. The roadmap's fallback
   plan (degrading to wasm-unified) is unnecessary.
2. Of the D-11 conventions, "a single implementation of transcendental functions" was
   the only actual obstacle. FMA contraction and denormals were not an issue on the
   code paths exercised this time
   (Rust does not contract by default, and wasm fully supports subnormals).
3. A prototype of the determinism CI gate is working as `scripts/determinism_test.sh`.
   In forte-core development, this will become a PR gate (extended to the reference corpus).

## Remaining Risks (Ongoing Monitoring)

- aarch64 (Apple Silicon) native is unverified — Rust's f32 arithmetic is IEEE-compliant
  so a match is expected, but add it to the CI matrix to confirm.
- Future multithreaded rendering will require pinning the addition order (D-11 §5).
- wasm NaN bit patterns are non-canonical (nondeterministic canonicalization) —
  avoided by the convention of never letting NaN flow through the audio path
  (the NaN guards of SDD §7).
- Version updates of the `libm` crate could change the numerics → include the numeric
  implementation's version in the engine version, forte.lock-style (already reflected
  in SRS-BLD-002).
