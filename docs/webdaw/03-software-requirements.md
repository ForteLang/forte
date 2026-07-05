# Software Requirements Specification (SRS) — Forte

Status: Draft v0.1 / 2026-07-02
Upstream document: 02-system-requirements.md (SYS)
Downstream documents: 04-software-architecture.md (SAD), 05-detailed-design.md (SDD)

> **Implementation status (as of 2026-07)**: Requirements in this document with an existing v0 implementation —
> LANG (the main parts of 001-008 + fmt + import), PKG (local import and Hub fork; semver not yet),
> CORE (directly coupled to dawcore; dedicated render-graph IR not yet), BLD (build + manifest + digest),
> LSP (diagnostics, completion, hover, formatting), VIS (arrangement overview + playhead; piano roll not yet),
> WEB (001-005: editor, AudioWorklet, OPFS, PWA; degradation verified on Chromium only),
> REC (001-005: .frec provenance, direct PCM capture, calibration; crash recovery and performance-fork GUI not yet),
> HUB (local Hub + HTTP: fork-only, release/verify, similarity search, playback ledger; git compatibility
> and fingerprint matching not yet), PLY (player, lineage page, progression similarity v1), SEC (structural
> validation of provenance only; cryptographic signatures and encryption at rest not yet).
> See the per-item statuses in 06-roadmap.md for details.

Notation: SRS-<component>-<number> [→ traced SYS].
Components: LANG (language processor), PKG (package management), CORE (audio engine),
BLD (build), LSP (editor assistance), VIS (visualization), WEB (web editor/runtime),
REC (recording), HUB (hub), PLY (player), SEC (security).

---

## 1. Language Processor (LANG)

- **SRS-LANG-001** [→SYS-LNG-001] Forte lang has the following first-class concepts:
  `song`, `track`, `pattern` (note sequence), `instrument`, `effect`, `bus`,
  `automation`, `asset` (recording reference), `module` (unit of reuse).
- **SRS-LANG-002** [→SYS-LNG-003] Source is UTF-8 text (extension `.forte`).
  One module per file. A formatter (`forte fmt`) is provided as standard and makes the canonical
  form unique (stabilizing diff/merge).
- **SRS-LANG-003** [→SYS-ENG-001] The language is **deterministic**: it has no runtime randomness,
  clock, or external I/O. Randomness requires an explicit seed (`random(seed: …)`).
- **SRS-LANG-004** [→SYS-LNG-001] Statically typed. Principal types: `Note`, `Pattern`, `Audio`
  (signal), `Control` (control signal), `Time` (unit-carrying beats/seconds), `Pitch`, `Db`,
  `Params`. Confusing units (beats vs. seconds, dB vs. linear) is detected as a type error.
- **SRS-LANG-005** [→SYS-LNG-002] `import` supports external dependencies in the
  `@scope/name@semver` form and local dependencies by relative path.
- **SRS-LANG-006** [→SYS-LNG-004] There is a low-level layer for writing DSP within the language
  (per-sample processing, state variables, filter primitives), which the compiler lowers to both
  native and WASM. The high-level layer (songs, arrangements) is written declaratively.
- **SRS-LANG-007** [→SYS-EDT-002] Incremental compilation: per-module caching, with recompilation
  of one changed module plus reflection into sound within 1 second.
- **SRS-LANG-008** Error messages use vocabulary aimed at musicians
  (e.g., "Track 'Vocal', bar 3: the Pattern length does not match the 4/4 time signature").

## 2. Package Management (PKG)

- **SRS-PKG-001** [→SYS-LNG-002] Manifest `forte.toml` (name, version, dependencies, license,
  visibility) and lock file `forte.lock` (resolved commit hashes of all dependencies).
- **SRS-PKG-002** [→SYS-HUB-002,003] Retrieval of public dependencies goes through the Hub's fork
  API, and the fact of retrieval is recorded in the lineage. Anonymous downloads from the registry
  do not exist.
- **SRS-PKG-003** [→SYS-LNG-004] Source is mandatory for public publication. WASM-only modules
  are usable only in private.
- **SRS-PKG-004** [→SYS-ENG-004] The lock file includes the **content hashes** of dependencies to
  detect tampering and substitution.

## 3. Audio Engine (CORE)

- **SRS-CORE-001** [→SYS-ENG-002] The engine builds a **render graph** from the compiled project
  (nodes = instrument/effect/bus, edges = audio/control) and processes it through a code path
  shared by real-time and offline.
- **SRS-CORE-002** [→SYS-ENG-003] The real-time path performs no allocation, locking, or system
  calls (following the discipline of the existing dawcore). Changes from UI/control arrive via a
  lock-free SPSC ring, and replaced structures are returned to non-RT threads through a garbage
  channel for deallocation.
- **SRS-CORE-003** [→SYS-ENG-001] Floating-point determinism conventions: f32 only, FMA disabled
  or explicit fma only, transcendental functions implemented in-house (pinned libm), denormals
  explicitly flushed, and parallelization limited to deterministic reductions with fixed
  association order.
- **SRS-CORE-004** [→SYS-NFR-005] Targets: native (cdylib + C ABI) and wasm32 (running inside an
  AudioWorklet). Single Rust source. * For implementation language, see SAD decision D-01.
- **SRS-CORE-005** [→SYS-ENG-002] Supports a sample-accurate scheduler, tempo/time-signature maps,
  looping, and automation (block rate + sample-accurate events).
- **SRS-CORE-006** [→SYS-EDT-002] Hot reload: swapping in a new render graph preserves the
  playback position and active voices to the extent possible, and occurs without click noise
  (crossfade within 10ms).
- **SRS-CORE-007** [→SYS-NFR-003] Performance metrics (callback utilization, underrun counter)
  are exposed at all times.

## 4. Build (BLD)

- **SRS-BLD-001** [→SYS-ENG-001] `forte build` outputs WAV (and Opus) and records the output's
  SHA-256 in `build.manifest.json` as the **build proof**.
- **SRS-BLD-002** [→SYS-ENG-004] `build.manifest.json` includes all dependencies (commit + fork
  lineage ID), all assets (hash + recording provenance), engine version, and build configuration.
- **SRS-BLD-003** [→SYS-HUB-004] Open-stems build: a build profile whose artifacts are per-bus/track
  stems plus the mix definition.
- **SRS-BLD-004** [→SYS-NFR-004] Full build at 5x real time or faster; incremental build within 1 second.

## 5. Editor Assistance (LSP) / Visualization (VIS)

- **SRS-LSP-001** [→SYS-EDT-001] LSP server: completion (modules, parameters, note names),
  type diagnostics, go-to-definition, rename, hover (parameter units and ranges).
- **SRS-LSP-002** [→SYS-EDT-001] VSCode extension: syntax highlighting, LSP connection,
  playback controls (play/stop/loop range), build tasks.
- **SRS-VIS-001** [→SYS-EDT-003] Visualization views (read-only): piano roll, arrangement
  overview, waveform/spectrum, mixer (level meters), render graph, lineage.
  Each view is bidirectionally linked to source locations (click a note → jump to the code line).
- **SRS-VIS-002** [→SYS-EDT-002] Visualization stays in sync with playback and targets 60fps
  (rendering must not affect audio).

## 6. Web Editor / Browser Execution (WEB)

- **SRS-WEB-001** [→SYS-EDT-004] Monaco-based web editor + the same LSP (running as WASM).
- **SRS-WEB-002** [→SYS-ENG-003] Browser playback: AudioWorklet (sink) + WASM engine +
  SharedArrayBuffer ring (ringbuf.js style). COOP/COEP deployment.
- **SRS-WEB-003** [→SYS-NFR-001] Projects and assets are stored in OPFS (Worker +
  SyncAccessHandle). A PWA in which editing, building, and playback are complete offline.
- **SRS-WEB-004** [→SYS-NFR-002] Degradation matrix: because Safari lacks Web MIDI and deletes
  storage after 7 days, an explicit degraded mode of "cloud sync mandatory + no MIDI input"
  is defined.
- **SRS-WEB-005** [→SYS-GOV-003] Provide zip export/import of local projects
  (git bundle compatible).

## 7. Recording (REC)

- **SRS-REC-001** [→SYS-REC-001] Only MIDI (Web MIDI / CoreMIDI, etc.) and microphone/line
  (getUserMedia / native) input devices are enumerated. No UI/API for file drop or audio import
  is implemented.
- **SRS-REC-002** [→SYS-REC-002] Recording captures PCM directly in an AudioWorklet
  (no MediaRecorder), sequentially writing SAB ring → Worker → OPFS/disk.
  Takes up to the moment of a tab/process crash can be recovered.
- **SRS-REC-003** [→SYS-REC-002] Recorded asset format `.frec`: PCM + provenance block
  (session ID, input device type, recording time, recordist ID, client signature).
  An audio reference without a provenance block is a compile error.
- **SRS-REC-004** [→SYS-REC-003] Loopback calibration wizard (measures output → input round-trip
  delay and stores a correction value with ±1ms accuracy). Recordings are placed on the timeline
  using the correction value.
- **SRS-REC-005** [→SYS-REC-001] getUserMedia is opened with echoCancellation/noiseSuppression/
  autoGainControl all false. In browsers where this does not take effect (known bugs), a warning
  is displayed.
- **SRS-REC-006** [→SYS-REC-004] Performance-fork mode: a minimal GUI that forks an open-stems
  release and only adds recorded tracks (playback + record + take selection + punch-in).

## 8. Hub (HUB)

- **SRS-HUB-001** [→SYS-HUB-001] Git-compatible hosting (smart HTTP). Private repositories allow
  normal git operation.
- **SRS-HUB-002** [→SYS-HUB-002] Public repositories: git clone/fetch is rejected at the
  authorization layer; replication is provided only via the fork API (lineage recording +
  ownership grant).
- **SRS-HUB-003** [→SYS-HUB-002,003] Lineage graph DB: nodes = repository/release/asset/user,
  edges = fork/depends/performed/released. Queryable via a public API.
- **SRS-HUB-004** [→SYS-HUB-004] Release pipeline: tag push → clean-room deterministic build on
  the build farm → hash comparison against the submitted build proof → publish on match.
  A mismatch blocks publication (enforced reproducibility).
- **SRS-HUB-005** [→SYS-HUB-005] Distribution is streaming only (segmentation + signed URLs).
  There is no download API (on the premise that complete copy prevention is impossible,
  complemented by terms of service + detection).
- **SRS-HUB-006** [→SYS-REC-005] Maintain acoustic fingerprints (of all released audio), with a
  matching job against new assets/releases plus a reporting flow.
- **SRS-HUB-007** [→SYS-HUB-006] Implement only the **data foundation** of the point ledger
  first: playback events → contribution aggregation over the lineage (batch). No redemption or
  spending features are implemented (future).
- **SRS-HUB-008** [→SYS-PLY-002] Song page: lineage (fork source/destinations, modules used,
  performers), version list (different vocalists, remixes), code browsing (public).

## 9. Player (PLY)

- **SRS-PLY-001** [→SYS-PLY-001] Web player (playback without login, gain normalization).
- **SRS-PLY-002** [→SYS-PLY-003] Similarity search v1: search by modules used, chord progression
  (canonical form of the progression extracted from the language AST), and tempo/key.
  Embedding-based similarity is v2.

## 10. Security / Privacy (SEC)

- **SRS-SEC-001** [→SYS-GOV-002] Private repositories/assets have encryption at rest and access
  audit logs. Operator viewing is impossible without an explicit consent flow.
- **SRS-SEC-002** [→RSK-02] Client signing keys are generated device-locally and sign provenance
  blocks. Key registration/revocation is managed on the Hub.
- **SRS-SEC-003** WASM modules (third-party instruments) run sandboxed
  (memory isolation, host API limited to audio processing, no file/network access).

## Appendix A: Traceability Matrix (Excerpt)

| SYS | SRS |
| --- | --- |
| SYS-LNG-001 | SRS-LANG-001/004/006 |
| SYS-LNG-003 | SRS-LANG-002 |
| SYS-ENG-001 | SRS-LANG-003, SRS-CORE-003, SRS-BLD-001, SRS-HUB-004 |
| SYS-ENG-002 | SRS-CORE-001/005/006 |
| SYS-ENG-003 | SRS-CORE-002, SRS-WEB-002 |
| SYS-ENG-004 | SRS-BLD-002, SRS-PKG-004 |
| SYS-EDT-001 | SRS-LSP-001/002 |
| SYS-EDT-002 | SRS-LANG-007, SRS-CORE-006, SRS-VIS-002 |
| SYS-EDT-003 | SRS-VIS-001 |
| SYS-EDT-004 | SRS-WEB-001/002/003 |
| SYS-HUB-002 | SRS-HUB-002/003, SRS-PKG-002 |
| SYS-HUB-004 | SRS-HUB-004, SRS-BLD-003 |
| SYS-HUB-005 | SRS-HUB-005 |
| SYS-REC-001 | SRS-REC-001/005 |
| SYS-REC-002 | SRS-REC-002/003, SRS-SEC-002 |
| SYS-REC-003 | SRS-REC-004 |
| SYS-REC-004 | SRS-REC-006 |
| SYS-REC-005 | SRS-HUB-006 |
| SYS-GOV-002 | SRS-SEC-001 |
| SYS-GOV-003 | SRS-WEB-005 |
| SYS-NFR-001 | SRS-WEB-003 |
| SYS-NFR-004 | SRS-BLD-004, SRS-LANG-007 |
| SYS-NFR-005 | SRS-CORE-004 |
