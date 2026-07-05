# Software Detailed Design (SDD) — Forte

Status: Draft v0.1 / 2026-07-02
Upstream document: 04-software-architecture.md (SAD)
This document elaborates the implementation targets of Phases 0–2 (see the roadmap). Later phases will be added as supplements.

---

## 1. Forte lang Language Sketch

The syntax is not a finalized specification; it is a reference example that conveys the intent of the language design (to be specified in Phase 0).

```forte
// song.forte — a song is code
import { tr909 }        from "@rhythm/tr909@^2.1"        // fork lineage is retained in forte.lock
import { juno }         from "@keys/juno-strings@1.0"
import { tape, limiter} from "@fx/std-master@^3"
import { section }      from "@arrange/pop-skeleton@0.4"  // the song skeleton is also a module
import vocalTake        from "../assets/vocal_take3.frec" // only recordings with provenance can be referenced

song "Aozora" {
  tempo 92
  meter 4/4
  key   D maj

  // patterns are data and values
  let kick  = beat`x--- x--- x-x- x---`
  let chord = prog`Dmaj7 | Bm7 | Em7 A7`   // progressions are first-class values → subject to similarity search

  track Drums {
    instrument tr909(kick: .deep, hat: .tight)
    play kick at bars(1..32)
    automate hat.decay from 0.2 to 0.6 over bars(17..24)
  }

  track Keys {
    instrument juno(voices: 8)
    play arp(chord, style: .updown, rate: 1/8) at section.verse
  }

  track Vocal {
    audio vocalTake                 // anything other than MIDI- and microphone-derived assets is a type error
    insert tape(drive: 0.25)
  }

  bus Master {
    insert limiter(ceiling: -0.3dB)
  }
}
```

```forte
// DSP layer example — instruments are written in a subset of the same language (public publication requires source)
device MonoSaw : Instrument {
  param cutoff: Hz = 800.0 in 20.0..18_000.0
  state phase: f32 = 0.0
  state svf:   Svf = Svf::lowpass()

  on note(n: Note) { phase = 0.0 }

  process(frame: &mut Frame, ctx: &Ctx) {
    let s = saw_blep(&mut phase, ctx.pitch_hz)
    frame.mono( svf.run(s, cutoff, q: 0.7) * ctx.env() )
  }
}
```

### 1.1 Key Design Points

- **Time types**: `bars/beats` (beats) and `sec` (seconds) are distinct types. Mixing requires explicit conversion only (SRS-LANG-004).
- **Compile-time expansion**: The Score layer's control structures (repeat/map/if) are evaluated
  at compile time and fully expanded into event sequences and the render graph. The only
  arbitrary code at runtime is the DSP layer's `process` (the linchpin of determinism and build speed).
- **Randomness**: `random(seed:)` only. Seeds are pinned in forte.lock and included in build reproduction.
- **Asset references**: `import x from "*.frec"` has type `RecordedAudio`, and provenance-block
  verification (signature + hash) runs at compile time. Verification failure is an error (SRS-REC-003).
- **Canonical form**: `forte fmt` defines the only formatting. The AST canonical form is the
  foundation for similarity search (progression extraction).

## 2. Compiler Pipeline

```
.forte ──parse──► AST ──resolve(imports, forte.lock)──► type checking
   ──lower──► IR (event sequence + graph definition + DSP kernels)
   ──codegen──► native: cranelift or LLVM / wasm: wasm32 module
   ──cache──► per-module compilation cache (content-hash keyed)
```

- Incremental builds: only re-lower/codegen the changed module and its downstream dependents.
  From the diff of the event sequences, identify "the tracks/regions that changed" and instruct
  the engine to swap only those parts (SRS-LANG-007).
- Codegen of DSP kernels starts in Phase 0 as an interpreter (a Rust-implemented operator tree)
  and may migrate to JIT/ahead-of-time compilation in Phase 2 (the determinism conventions
  require identical numeric paths in both).

## 3. forte-core (Engine) Details

### 3.1 Render Graph

```rust
struct Graph {
    nodes: Vec<Node>,          // Source(instrument) / Fx / Bus / Meter / Sink
    edges: Vec<(NodeId, PortId, NodeId, PortId)>, // audio / control
    order: Vec<NodeId>,        // topological order (determined and pinned by the compiler)
}
```

- Execution order is determined by the compiler and baked into the graph (no runtime sorting =
  order determinism).
- Node processing is in 128-sample blocks. Sample-accurate events (notes, automation points) are
  delivered to nodes with intra-block offsets (the dawcore approach).
- Hot swap: only the differing nodes between old and new graphs are replaced. Replaced nodes get
  a 10ms equal-power crossfade. State (filter history, voices) is transferred when the NodeId
  matches and the type is the same (SRS-CORE-006).

### 3.2 Thread/Memory Discipline (Following dawcore)

- RT thread (cpal callback / AudioWorklet process): no allocation, locking, or syscalls.
- Control: SPSC ring (native: ringbuf crate / web: an in-house wait-free ring over SAB, with the
  same layout as ringbuf.js). Hot messages are Copy; structures are Box-transferred with garbage
  return.
- Readout: meters/playback position/underrun counts are published via atomics.

### 3.3 Floating-Point Determinism Conventions (D-11)

1. The sample type is f32 only. Intermediate accumulators may be f64 (but usage sites are pinned
   by convention).
2. `-ffast-math`-style optimizations prohibited. FMA contraction is prohibited on both wasm and
   native (Rust: no contraction by default; `mul_add` only by explicit use = identical on both
   targets).
3. Transcendental functions (sin/exp/log/pow/tanh) must not depend on libm; the in-house
   polynomial-approximation implementation `forte_math` is the sole implementation.
4. Denormals are explicitly flushed in every filter state (additive-offset method or
   quantization). * Do not rely on the CPU's FTZ flag (it does not exist in wasm).
5. Parallel rendering uses per-track work stealing, but **the mix addition order is pinned to the
   graph-baked order** (deterministic reduction). Phases 0–1 start single-threaded.
6. Verification: SHA-256 equality of the reference corpus's native/wasm outputs is a CI gate.

### 3.4 C ABI (forte_ffi)

```c
ForteCtx*  forte_open(const char* project_dir);
int        forte_build(ForteCtx*, const ForteBuildOpts*, ForteBuildResult* out);
int        forte_play_start(ForteCtx*, uint32_t sample_rate);
int        forte_eval(ForteCtx*, const char* changed_file);   // hot reload
void       forte_meter_read(ForteCtx*, ForteMeters* out);
void       forte_close(ForteCtx*);
```

Intended for rendering and feature-extraction use from ML/analysis tools (Python, etc.)
(a founder requirement).

## 4. Recording Subsystem

### 4.1 `.frec` Pointer + CAS Content (D-08)

```json
// assets/vocal_take3.frec (only this pointer goes into the repository)
{
  "hash": "sha256:ab12…",           // the PCM content in the CAS
  "format": {"codec": "pcm_f32le", "rate": 48000, "ch": 1},
  "provenance": {
    "session": "uuid",  "device_class": "microphone",
    "recorded_at": "2026-07-02T04:12:33Z", "recorded_by": "user:shusuke",
    "input_chain": ["gate(-60dB)"],   // monitoring chain applied at recording time (recorded only)
    "sig": "ed25519:…"               // signature by the device-local key (SRS-SEC-002)
  }
}
```

- The compiler checks (a) existence of the hashed content, (b) signature verification, and
  (c) `device_class ∈ {microphone, midi-render}`. Failure is type error `E-PROV-001`.
- Recording writes: [input AudioWorklet tap] → SAB ring (8-second capacity) → the asset Worker
  appends to OPFS every second + updates the recovery journal. After a crash, the take is
  restored from the journal (RSK-01).

### 4.2 Loopback Calibration (SRS-REC-004)

Output a chirp signal, receive it on the input, and detect the peak by cross-correlation. The
median of 5 trials is saved to `calibration.json` (per device pair). At recording placement,
compensate by `recorded_pos - (rtl - output_latency_reported)`. Target ±1ms.

## 5. Hub Details (Phase 2 Target)

### 5.1 Implementing the Fork Constraint (SRS-HUB-002)

- Authorization in front of git smart HTTP: `upload-pack` (clone/fetch) is permitted only for
  (a) owners/collaborators and (b) the fork repositories of users who have already forked.
- `POST /repos/{id}/fork` atomically performs server-side replication + creation of the
  `forked_from` edge + granting read/write to the forker.
- Code browsing in the Web UI is open to everyone for public repos (readable, but taking content
  out only by fork — that is the principle).

### 5.2 Release Pipeline (SRS-HUB-004)

```
tag push → webhook → build farm (container, network cut off, pinned forte-core version)
  → forte build → SHA-256 comparison (matches the submitted build.manifest.json?)
  → match: create Release node + encode for streaming (Opus segments) + register fingerprint
  → mismatch: reject + diff report (a determinism breakage is treated as our bug)
```

### 5.3 Lineage Aggregation (D-12)

- Playback events `(release, listener, duration)` are recorded in an append-only log.
- Daily batch: from the release, apportion contribution points with a decay factor along
  forte.lock's dependency closure + performed edges (the factor is a future governance item;
  initially recording only).

## 6. VSCode Extension / Web Editor

- The extension is implemented in TS. It spawns forte-lsp (native binary). Playback runs in a
  helper process inside the extension host that calls forte_ffi (crash isolation).
- Visualization Webview: renders the compiler-emitted `viz.json` (event sequences, graph, meter
  channels) to Canvas. Click → jump to the corresponding code line via `sourceMap` (SRS-VIS-001).
- Web version: runs the same viz.json/LSP as wasm. Editor state lives in OPFS; Hub sync is done
  with git (isomorphic-git or wasm-git).

## 7. Error/Exception Policy

- RT path: panics prohibited. All nodes have saturation/NaN guards (on NaN detection, bypass the
  node and publish an error event — do not take down the whole song).
- Compiler: errors in musical vocabulary (SRS-LANG-008). Error-code scheme `E-<area>-<number>`.
- Hub: release verification failures and signature mismatches return a complete diff explanation
  to the user (safeguarding trust).

## 8. Open Items (To Be Decided in Phase 0)

1. Final form of the syntax (user testing of the sketch above)
2. Initial execution mode of the DSP layer (interpreter vs. ahead-of-time codegen)
3. wasm-git vs. a custom sync protocol
4. Initial contents of the `@std` standard library (from dawcore: polymer-family synths, SVF, EQ,
   delay, FDN reverb, sampler → but the sampler is restricted to recorded assets only)
5. Design of the canonical form for progressions (`prog`) and the similarity-search index
