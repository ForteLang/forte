# System Requirements Specification (SYS) — Forte

Status: Draft v0.1 / 2026-07-02
Upstream document: 01-vision.md
Downstream documents: 03-software-requirements.md (SRS), 04-software-architecture.md (SAD), 05-detailed-design.md (SDD)

This product is not a medical device, but we adopt the process discipline of IEC 62304
(traceability from system requirements → software requirements → architecture → detailed design,
plus risk-based verification). All items are treated as equivalent to safety Class A, but risk
management treats "loss of user works" and "tampering with provenance" as the top-level hazards (§6).

Notation: SYS-<area>-<number>. Every requirement has verifiable acceptance criteria.
Areas: LNG (language), ENG (engine), EDT (editor), HUB (ecosystem), REC (recording/input),
PLY (listening), GOV (governance/licensing), NFR (non-functional).

---

## 1. System Context

```
 Composer ──(VSCode-family editor / web editor)──► Forte toolchain
                                              │  language processor + audio engine
                                              ▼
 Performer/vocalist ──(MIDI and microphone only)──►  Recorded assets (with provenance)
                                              │
                                              ▼  push / fork / release
 Listener ◄──(player / lineage graph)──  Forte Hub (registry + build farm + distribution)
```

The system consists of three subsystems:
- **SS1 Toolchain**: language processor, package manager, audio engine, CLI
- **SS2 Editor**: VSCode extension + web editor (browser), live preview, visualization
- **SS3 Hub**: repository hosting, fork lineage, release/build farm, player, (future) points

## 2. Stakeholder Requirements → System Requirements

### 2.1 Language and Modules (LNG)

- **SYS-LNG-001** It shall be possible to express songs, tracks, instruments, effects, sequences,
  and utilities entirely as source code in a single programming language (Forte lang).
  - Acceptance criteria: one reference song (drums + bass + synth + vocal recording + mastering)
    can be expressed and built with source plus recorded assets only, using no binary-format
    project files whatsoever.
- **SYS-LNG-002** Any module shall be reusable via `import`, with dependencies pinned by a
  manifest plus a lock file (semver compatible).
- **SYS-LNG-003** The language shall not presuppose GUI editing; it shall be **human-readable and
  human-writable text** that supports review, diff, and merge (a git-compatible text format).
  - Acceptance criteria: two branches editing different tracks can be git-merged without conflicts.
- **SYS-LNG-004** Plugin equivalents (instruments/effects) shall be written in Forte lang or be
  providable as modules conforming to a prescribed WASM ABI.
  Publishing to the public registry **requires source disclosure** (white-box principle).

### 2.2 Deterministic Builds (ENG)

- **SYS-ENG-001** From the same commit + lock file + build configuration,
  **bit-identical audio** shall be reproduced (deterministic build).
  - Acceptance criteria: the hashes of outputs built in two different environments
    (x86_64 Linux / browser WASM) match.
- **SYS-ENG-002** The same engine shall handle both real-time playback (preview) and offline
  rendering (build), and the two shall sound identical.
- **SYS-ENG-003** Real-time playback shall run without glitches
  (browser: AudioWorklet 128 frames / within the ~3ms budget. 00-research-report.md §3.1).
- **SYS-ENG-004** Build artifacts shall always be accompanied by a provenance manifest
  (repository + commit + fork lineage for all dependencies, hash + recording provenance for all
  recorded assets).

### 2.3 Editor Experience (EDT)

- **SYS-EDT-001** Coding in VSCode (or a compatible editor) shall be supported as a first-class
  composing experience (LSP: completion, type checking, error display, go-to-definition).
- **SYS-EDT-002** Editing shall be completed entirely in code, with save/evaluate reflected in
  sound (hot reload) within 1 second (incremental build).
- **SYS-EDT-003** **Read-only visualizations** generated from code (piano roll, waveform, mixer,
  lineage) shall be provided. Editing from the visualizations is not possible (code is the single
  source of truth for editing).
- **SYS-EDT-004** The equivalent core experience (edit, build, play) shall work in the browser
  alone (removing the installation barrier; the web editor is Monaco-based).

### 2.4 Ecosystem (HUB)

- **SYS-HUB-001** Repositories can be private / public and support push/pull over a
  git-compatible protocol.
- **SYS-HUB-002** **Public repositories shall be non-clonable; fork is mandatory.**
  Forks are permanently recorded as a lineage graph (who made what from what).
  - Acceptance criteria: raw git clone access to a public repository is rejected; the content can
    only be obtained locally through the Hub's fork operation.
- **SYS-HUB-003** Dependency resolution (package retrieval) shall also be recorded in the fork
  lineage (dependency = lineage).
- **SYS-HUB-004** Releases from tags (deterministic build on the Hub's build farm → verification →
  publication) shall be supported. Releases come in two forms: full mix / open-stems.
- **SYS-HUB-005** Released audio shall not be downloadable/reusable (streaming listening only).
  Obtaining the content shall be possible only by forking.
- **SYS-HUB-006** (Future) Point system: aggregate from the lineage the track record of
  modules/assets being used in others' releases, and recirculate it as usage entitlements.
  The initial release performs **lineage recording only** and introduces no economy.

### 2.5 Recording and Input Restrictions (REC)

- **SYS-REC-001** Audio input shall be limited to **MIDI input and microphone (line) input only**,
  with no import feature for external audio files (white-box principle).
- **SYS-REC-002** Recorded assets shall automatically receive recording provenance (session ID,
  device information, recording time, recordist, system signature), and audio without provenance
  shall not be able to participate in builds.
- **SYS-REC-003** Recording latency shall be calibrated and compensated placement applied on the
  timeline (loopback calibration; target accuracy ±1ms, 00-research-report.md §3.2).
- **SYS-REC-004** The "performance fork" (fork + adding recorded tracks) on open-stems releases
  shall be supported with a minimal recording GUI (transport + take management).
- **SYS-REC-005** Microphone re-recording of released audio and disguised introduction of
  externally generated audio shall be terms-of-service violations, with a moderation mechanism
  based on reporting + acoustic fingerprint matching
  (the design assumes complete technical prevention is impossible).

### 2.6 Listening (PLY)

- **SYS-PLY-001** Releases shall be listenable in the player on the Hub (public playback without login).
- **SYS-PLY-002** A song's **lineage page**: the modules used, recording participants, fork
  sources/destinations, and derived versions (different vocalists, remixes, rock versions, etc.)
  shall be traversable.
- **SYS-PLY-003** Code-based similarity search (progressions, structure, modules used) shall
  enable discovery of "similar songs" and "creators using the same tools" (may be introduced in stages).

### 2.7 Governance (GOV)

- **SYS-GOV-001** Public submissions shall by default carry the **lineage-preserving license**
  (a custom public license requiring mandatory forking, attribution, and preservation of per-track
  provenance). Implement it on top of current copyright law and subject it to legal review.
- **SYS-GOV-002** The contents of private repositories (unreleased songs) shall be protected
  end-to-end from third parties including the operator (at minimum: encryption at rest + access
  auditing).
- **SYS-GOV-003** Users shall be able to export all of their data (repositories, assets, lineage
  records) in standard formats (lock-in rejection; the lesson of Endlesss).

### 2.8 Non-Functional (NFR)

- **SYS-NFR-001** Local-first: the editor and builds shall work fully offline.
  The Hub is for collaboration and publication and shall not be a mandatory dependency of production.
- **SYS-NFR-002** Supported environments: desktop (CLI + VSCode: Win/macOS/Linux),
  browser (Chromium full-featured; the degradation matrix for Firefox/Safari is defined in the SAD).
- **SYS-NFR-003** Real-time playback: a reference song with 10 tracks + 20 devices at 44.1/48kHz
  shall play with a 0 underrun rate on a mid-range PC (4 cores).
- **SYS-NFR-004** Build performance: a full build of the reference song (3 minutes) at 5x
  real time or faster; an incremental build (one module changed) within 1 second.
- **SYS-NFR-005** The core engine shall be built from a single source for both native
  (server/CLI) and WASM (browser) targets.

## 3. Out of Scope (Explicit)

- Editing via GUI (note editing in a piano roll, etc.) — permanently out of scope (visualization is provided)
- Importing external audio files — permanently out of scope (a premise of the invention)
- Hosting native plugins such as VST/AU — out of scope (contrary to the white-box principle)
- Importing DAW projects (Ableton/Logic, etc.) — out of scope
- Monetary conversion of the point economy — out of initial scope (after regulatory review)

## 4. Assumptions and Dependencies

- Browser execution assumes deployment with COOP/COEP (cross-origin isolation) (SharedArrayBuffer).
- For deterministic builds, the engine's DSP follows implementation conventions that guarantee
  floating-point determinism (SDD §4).
- The trustworthiness of recording provenance depends on client signatures (full tamper
  resistance is a future topic).

## 5. Verification Policy

- Each SYS requirement is traced to one or more software requirements in the SRS, and the
  traceability matrix is placed in the appendix of document 03.
- Determinism (SYS-ENG-001) is continuously verified in CI via two-environment hash comparison.
- Real-time behavior (SYS-ENG-003) is verified with a bench harness that measures underrun counters.

## 6. Risk Management (Top-Level Hazards)

| ID | Hazard | Impact | Mitigation (trace to requirements) |
| --- | --- | --- | --- |
| RSK-01 | Loss of user works (code/recordings) | Fatal (loss of trust) | Local-first (SYS-NFR-001), export (SYS-GOV-003), incremental persistence of recordings (specified in SRS) |
| RSK-02 | Tampering with/forging lineage (bringing in others' audio) | Damage to ecosystem trust | Provenance enforcement (SYS-REC-002), input restrictions (SYS-REC-001), moderation (SYS-REC-005) |
| RSK-03 | Determinism breakage (build reproduction failure) | Collapse of release verification and contribution proof | Determinism conventions + CI verification (SYS-ENG-001) |
| RSK-04 | Leakage of private songs | Legal and trust risk | SYS-GOV-002 |
| RSK-05 | Real-time audio dropouts | Damage to the production experience | SYS-ENG-003 / SYS-NFR-003 |
