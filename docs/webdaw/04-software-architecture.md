# Software Architecture Design (SAD) — Forte

Status: Draft v0.1 / 2026-07-02
Upstream document: 03-software-requirements.md (SRS)
Downstream document: 05-detailed-design.md (SDD)

---

## 1. Architecture Decision Records

| ID | Decision | Rationale | Status |
| --- | --- | --- | --- |
| **D-01** | The implementation language for the core engine + compiler is **Rust** (exposed as an API via a C ABI) | Existing dawcore assets (lock-free design, DSP, offline rendering), maturity of the wasm32 toolchain, memory safety. The founder's request was "build in C++ and expose an API," but the requirements (native core + API + WASM) can be met equally well or better with Rust | **Approved 2026-07-02** |
| D-02 | Forte lang is a **custom DSL** (not an embedding in a general-purpose language) | To enforce determinism (SRS-LANG-003) and input restrictions (SRS-REC-001) at the language-specification level. A TS/Python embedding cannot exclude arbitrary I/O | **Approved 2026-07-02** |
| D-03 | The language has 2 layers: a "declarative song-description layer" + a "DSP-description layer" | A simple declarative layer for composers, a low-level layer for instrument developers. Provided as subsets within a single language | Pending approval |
| D-04 | Real-time and offline use the **same render-graph implementation** | SYS-ENG-002. Follows dawcore's bounce.rs approach | Pending approval |
| D-05 | Browser execution is AudioWorklet (sink) + WASM + SAB ring | Industry-standard pattern (00-research §3.1). COOP/COEP deployment required | Pending approval |
| D-06 | The distribution format for third-party instruments/effects is **Forte source** (mandatory for public). Precompiled WASM is private-only | White-box principle (SYS-LNG-004). WAM 2.0 is not adopted (it permits binaries + arbitrary JS UI, contrary to the principle), but its host API design serves as a reference | Pending approval |
| D-07 | The Hub's VCS is **git-compatible** (no custom VCS). The fork constraint is implemented at the authorization layer | Use git's ecosystem (diff/merge/history) as-is. Public clone rejection is achieved via server-side authorization (SRS-HUB-002) | Pending approval |
| D-08 | Recorded assets are content-addressed (SHA-256) in a separate git LFS-like store; the repository holds only references + provenance | The industry rule of not putting binaries in CRDTs/git (00-research §3.5) | Pending approval |
| D-09 | The lineage graph is first-class Hub data (graph DB), managed independently of git history | fork/depends/performed/released cannot be expressed in git | Pending approval |
| D-10 | Editor strategy starts as "VSCode extension primary, web editor (Monaco) secondary" | Targeting intermediate-to-advanced users. Share the LSP to avoid double development | Pending approval |
| D-11 | Determinism conventions: f32, in-house libm, denormal flush, deterministic parallel reduction | Precondition for SYS-ENG-001. Details in SDD §4. **Proven by spike (07-determinism-spike.md): native/wasm bit-identity achieved with libm unification alone** | Pending approval |
| D-12 | The point economy ships only "event collection + lineage aggregation" first, with a ledger design allowing economic rules to be added later | Make no irreversible design before regulatory and game-theoretic study | Pending approval |
| **D-13** | **Forte Studio IS the VS Code extension** — no bespoke desktop shell (the Tauri direction of issue #135 is dropped). DAW surfaces (arrange, beat grid, piano roll, mixer, analyze) are webview panels inside VS Code, reading through `forte viz`/`forte edit --sites` and writing through `forte edit` (the lossless edit layer). Git GUI, GitHub/PR integration, AI assistants (Claude Code et al.), terminals, remote dev and the whole extension ecosystem come from the host instead of being rebuilt | Maintainer direction 2026-07-14: "VS Code already has the git GUI, the GitHub plugin, calling Claude — I want the same things here." Strengthens D-10. Rebuilding commodity IDE infrastructure (we had begun: a hand-rolled git panel in the web editor) is wasted effort and forever behind; what is unique to Forte is the deterministic engine, the language, and the code↔GUI projections — exactly what an extension can own. The web editor remains the zero-install listening/sketch surface and the wasm bit-identity proof | **Approved 2026-07-14** |

## 2. System Decomposition

```
┌─────────────────────────── SS1 Toolchain (Rust) ───────────────────────────┐
│                                                                                  │
│  forte-lang     parser / type checking / module resolution / canonicalization (fmt) / AST→IR │
│  forte-compile  IR → render-graph definition + DSP kernels (native/wasm code generation)   │
│  forte-core     render-graph executor (shared RT/offline), scheduler, mixer      │
│                 ← adapted and reused from existing dawcore engine/dsp/bounce     │
│  forte-pkg      forte.toml / forte.lock / Hub fork API client                    │
│  forte-cli      build / play / fmt / test / publish                              │
│  forte-lsp      LSP server (embedding forte-lang)                                │
│  C ABI: forte_ffi (libforte.so / .dylib / .dll) — usable from ML/external tools  │
└──────────────────────────────────────────────────────────────────────────────────┘

┌──────────── SS2 Editor ────────────┐   ┌──────────────── SS3 Hub ────────────────┐
│ VSCode extension (TS)               │   │ git hosting + authorization layer (public=fork-only) │
│  ├ LSP client                       │   │ lineage graph service (GraphDB)          │
│  ├ playback controls / visualization Webview │   │ asset store (CAS, S3-like + signed URLs) │
│  └ local forte-core (native)        │   │ build farm (deterministic build + verification) │
│ Web editor (Monaco + WASM suite)    │   │ streaming distribution / player          │
│  ├ forte-lsp (wasm)                 │   │ fingerprint matching / moderation        │
│  ├ forte-core (wasm, AudioWorklet)  │   │ event ledger (playback → contribution aggregation) │
│  └ OPFS project store               │   │ accounts / signing key management        │
└───────────────────────────────────────┘   └───────────────────────────────────────────┘
```

## 3. Runtime Architecture (Playback and Recording Paths)

### 3.1 Native (CLI / Inside the VSCode Extension)

```
forte-lang ──AST──► forte-compile ──graph+kernels──► forte-core
                                                        │
editor/CLI ──commands (SPSC ring)──► RT thread (cpal callback)
                                    ◄──garbage return / meters (atomics)
Recording: input callback ──SPSC──► writer thread ──► sequential .frec writes
```

### 3.2 Browser

```
Main thread: Monaco / visualization (Canvas/WebGPU) / transport UI
   │ postMessage (control) / SAB rings (audio, meters)
Worker(compile): forte-lang + forte-compile (wasm) — incremental builds
Worker(asset):   OPFS SyncAccessHandle — persistence of projects/recordings
AudioWorklet:    forte-core (wasm) — every 128 frames, consumes commands
                 from the SAB ring, renders, publishes meters
Recording: AudioWorklet (input tap) ──SAB ring──► Worker(asset) ──► OPFS .frec
```

- Graph swapping (hot reload) is "build the new graph on the Worker side → transfer the
  Box equivalent via the ring → swap on the RT side → return the old graph as garbage"
  (isomorphic to dawcore's existing protocol).
- COOP/COEP required. Third-party assets are restricted to same-origin delivery (own CDN).
  * Because D-06 means there is no plugin loading from arbitrary origins, the cross-origin
  tensions that afflict WAM do not arise.

## 4. Language Architecture (2-Layer Structure, D-03)

| Layer | Target users | Contents | Execution form |
| --- | --- | --- | --- |
| **Score layer** (declarative) | Composers | song/track/pattern/arrangement/mix. Time is in beats. Control flow is limited (map/repeat/conditionals evaluated at compile time) | Fully expanded at compile time → event sequence + graph |
| **DSP layer** (procedural) | Instrument/effect developers | `process(frame)` kernels, state variables, filter/oscillator primitives | Code-generated to native/wasm, executed in RT |

- Both layers are deterministic (SRS-LANG-003). I/O, clocks, and unseeded randomness do not exist in the language.
- The Score layer's property of being "fully expandable at compile time" is the key to determinism
  and build speed. Generative composition (algorithmic composition) can be written as seeded
  compile-time functions in the Score layer.

## 5. Data Architecture

### 5.1 Repository Contents

```
song-repo/
  forte.toml          manifest (name, version, dependencies, license, visibility)
  forte.lock          resolved dependencies (commit + content hash + lineage ID)
  src/*.forte         code (songs, tracks, custom devices)
  assets/*.frec       actual content lives in the CAS, not recording references. Only pointer
                      files (hash + provenance block + signature) are placed here (D-08)
  build.manifest.json latest build proof (output hash + full provenance) — verified at release
```

### 5.2 Lineage Graph (D-09)

- Nodes: `User / Repo / Release / Asset / ModuleVersion`
- Edges: `forked_from / depends_on(version) / performed_on / released_as / recorded_by`
- Invariant: any replication operation on a public Repo always creates a `forked_from` edge (SRS-HUB-002).
- Playback events are tied to a `Release` and contribution is apportioned in batch along the
  dependency closure (D-12).

## 6. Degradation Matrix (Browser)

| Feature | Chromium | Firefox | Safari |
| --- | --- | --- | --- |
| Edit, build, play (WASM+AudioWorklet+SAB) | Yes | Yes | Yes (COOP/COEP required) |
| MIDI input | Yes | Yes | **No (Web MIDI unsupported) → on-screen keyboard only** |
| Microphone recording (constraints off) | Yes | Yes | Partial (EC constraint bug, must specify 44.1kHz) |
| OPFS persistence | Yes | Yes | Partial **7-day deletion → Hub sync made mandatory** |
| Real-folder saving | Yes (FSA) | No, zip DL | No, zip DL |
| Recommended position | Full features | Nearly full | "Try and listen" + sync required |

Since native (CLI + VSCode) is the first-class environment for professional use, browser
disparities are absorbed by positioning "the web as entry point, sharing, and light work."

## 7. Reuse Map from dawcore

| dawcore (existing) | Treatment in Forte |
| --- | --- |
| dsp/ (polyBLEP, SVF, ADSR, delay, FDN reverb, sampler) | **Reuse**: repurposed as DSP kernel implementations for the standard library `@std/*` |
| engine.rs (sample-accurate scheduler, mixer) | **Adapt and reuse**: render-graphification (fixed 3 stages → arbitrary graph) |
| command.rs (SPSC + garbage channel) | **Reuse**: foundation of the hot-reload/control protocol |
| bounce.rs (offline rendering) | **Reuse**: the core of `forte build`. Determinism conventions additionally applied |
| model.rs (index-referenced project model) | **Discard**: the model is replaced by compiler output (IR) |
| dawapp (egui UI) | **Discard** (visualization reference only), per the policy of not building an editing UI |
| tests/ (offline render verification) | **Reuse**: evolve into determinism CI (two-environment hash comparison) |

## 8. Verification Architecture

- **Determinism CI**: build the reference-song corpus on native (Linux x86_64) and wasm (Node)
  and compare SHA-256. Run on every PR (SYS-ENG-001).
- **RT bench**: measure the underrun counter + callback utilization on the reference song
  (SYS-NFR-003).
- **Golden audio tests**: audio regression tests equivalent to dawcore's visual tests
  (output hash pinned; updated only on intended changes).
- **Language tests**: `forte test` (module unit tests: expected event sequences / expected spectra).
- **Hub integration tests**: E2E for the fork constraint (clone rejection) and release
  reproduction verification.
