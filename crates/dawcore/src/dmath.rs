//! Deterministic float math (one implementation on every compilation target).
//!
//! `f32::sin` and friends lower to the platform libm on native builds but to
//! compiler-builtins on wasm32, so the same project renders to different bit
//! patterns per target. Routing every transcendental through the pure-Rust
//! `libm` crate pins a single implementation everywhere, which is what makes
//! offline bounces reproducible across native and wasm (Forte rule D-11).
//!
//! `sqrt`, `abs`, `floor`, `round`, `fract`, `min`/`max` are IEEE-exact and
//! may keep using the std methods.

#[inline(always)]
pub fn sin(x: f32) -> f32 {
    libm::sinf(x)
}

#[inline(always)]
pub fn cos(x: f32) -> f32 {
    libm::cosf(x)
}

#[inline(always)]
pub fn tan(x: f32) -> f32 {
    libm::tanf(x)
}

#[inline(always)]
pub fn exp(x: f32) -> f32 {
    libm::expf(x)
}

#[inline(always)]
pub fn tanh(x: f32) -> f32 {
    libm::tanhf(x)
}

#[inline(always)]
pub fn powf(x: f32, y: f32) -> f32 {
    libm::powf(x, y)
}
