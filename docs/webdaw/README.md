# Forte — Documentation Structure

Design documents for a music production platform where "music is composed in code and contributions are tracked through fork lineage."
Adopts the process discipline of IEC 62304 (traceability from requirements → architecture → detailed design).

| # | Document | Contents |
| --- | --- | --- |
| 00 | [research-report](00-research-report.md) | Web DAW market and technology research (2026-07). Competitors, OSS, platform maturity, AI trends |
| 01 | [vision](01-vision.md) | Product vision: white-boxing music / fork lineage / deterministic builds |
| 02 | [system-requirements](02-system-requirements.md) | System requirements specification (SYS) + risk management |
| 03 | [software-requirements](03-software-requirements.md) | Software requirements specification (SRS) + traceability |
| 04 | [software-architecture](04-software-architecture.md) | Architecture design (SAD) + architecture decision records (ADR) |
| 05 | [detailed-design](05-detailed-design.md) | Detailed design (SDD): language sketch, engine, recording, Hub |
| 06 | [roadmap](06-roadmap.md) | Development roadmap (Phase 0–5) + risk register |
| 07 | [determinism-spike](07-determinism-spike.md) | Phase 0.4 spike results: native/wasm bit-identical rendering achieved |
| 08 | [daw-functional-requirements](08-daw-functional-requirements.md) | The full DAW operation surface (survey of 6 major DAWs), every operation dispositioned: CODE / CODE-GAP / GUI / TOOL / NO — the requirements base for Forte Studio (#135) |
| spec | [forte-lang-v1](spec/forte-lang-v1.md) | **Language specification v1 (implementation-conformant): grammar EBNF, semantics, diagnostics catalog, determinism contract** |
| spec | [forte-lang-v0](spec/forte-lang-v0.md) | Language specification v0 draft (design intent, future concepts) |

## Current State of Implementation

- **`crates/fortelang`** — Language v0 slice: lexer/parser/checking (with diagnostic codes),
  compilation to dawcore, `forte check` / `forte build` (WAV + build.manifest.json) /
  `forte play` (real-time playback plus hot reload that applies changes immediately on save.
  Falls back to a silent backend when no audio device is present).
- **`songs/`** — Four reference songs (`first-light` 4/4, `slow-circles` 6/8,
  `night-parade`: prog/section/send-return, `handmade`: **instruments defined in code
  via the `device` syntax and `import`-ed from the `songs/devices/warm.forte` library** —
  the minimal proof that synths are forkable code and can circulate as modules).
- **`editor/vscode-forte`** — **Forte Studio (ADR D-13: VS Code IS the Studio
  shell)**: syntax highlighting, real-time diagnostics via `forte lsp`, Play
  (hot reload) / Build / Stop, REPL, History (VCS) and Blocks sidebars with
  one-click audition, a **drag-editable arrangement view** (drop = bar-snapped
  `move_at_line` through the lossless edit layer, on the editor's undo stack)
  and a **Beat Grid panel** (`forte edit --sites` → clickable step rows →
  `set_pattern`). Git GUI, GitHub/PRs, AI assistants and terminals come from
  VS Code itself — the extension owns only what is unique to Forte.
- **`web/` + `crates/forteweb`** — Browser editor prototype:
  wasm on the main thread provides as-you-type diagnostics, build proofs, and visualization
  data; wasm inside the AudioWorklet handles playback plus hot reload. Read-only arrange
  view (code is the single source of truth).
  **OPFS auto-save (multiple songs, persists across reloads) + full offline operation via
  Service Worker** = the first proof of local-first (SYS-NFR-001). Build with
  `scripts/build_web.sh`, serve the repository root statically, and open `/web/`.
  E2E is `scripts/web_e2e.mjs`
  (7 checks in real Chromium: **three-way bit-identity across native / wasip1 / browser**,
  OPFS persistence, startup/compile/playback with the network disconnected).
- **`scripts/determinism_test.sh`** — Two-stage determinism gate (engine alone / via forte build).
  Both can be CI-verified as bit-identical across native x86_64 and wasm32-wasip1.
- **`forte edit` (fortelang::edit)** — The Studio P0 lossless-edit spike (#135,
  DAW-FR "GUI projection" rows): structured JSON operations (set_tempo,
  set_pattern, move_place/move_play, add_place/remove_place, set_arg,
  set_section) are applied as **minimal token splices** — the real parser
  supplies `Pos` anchors, the lexer's byte spans locate the exact bytes, and
  everything outside the edited tokens (comments, blank lines, layout) is
  byte-identical by construction. Every result must re-parse or the edit is
  refused. This is the write path a Studio GUI gesture goes through.
- **Local Hub (`forte hub`)** — The first implementation of the fork-lineage registry.
  `publish` (snapshots a song/library including its imports; requires successful compilation),
  `fork` (**the only means of acquisition**; writes the provenance stamp `.forte-lineage.json`),
  `release` (deterministic build of a snapshot → records the digest in the ledger),
  `verify` (reproducibility verification via clean-room rebuild; tampering is detected as MISMATCH),
  `lineage` (ancestor chain + fork list + releases and verification counts), `list`.
  Deterministic, based on sequence numbers. Release digests are identical to the browser's
  build proofs (= anyone can audit from any environment).

## Decision Status

- **D-01 approved (2026-07-02)**: Core in Rust (exposed as an API via C ABI)
- **D-02 approved (2026-07-02)**: Custom DSL
- Open: naming (Forte is provisional), timing for starting legal review of the lineage-preservation license
