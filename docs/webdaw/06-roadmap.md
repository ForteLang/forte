# Development Roadmap — Forte

Status: Draft v0.1 / 2026-07-02
Assumptions: 1–3 core developers + founder. Durations are guidelines; decisions are made against the Exit criteria at the end of each phase.

Strategic principles:
1. **The language and engine are the product's heart. Do not over-engineer the Hub before lineage data accumulates.**
2. Each phase must ship exactly one "experience that can be shown externally" (for hiring, gathering allies, and validation).
3. The point economy comes last. **Lineage recording** alone must start from Phase 2 (an asset onto which the economy can be layered later).

---

## Phase 0 — Language Specification and Core Proof (~3 months)

**Goal: technical proof that "a song can be built from code alone."**

| # | Deliverable | Requirements |
| --- | --- | --- |
| 0.1 | Forte lang specification v0 (syntax, types, determinism conventions) + parser/type checker — **🔶 v0 slice implemented (crates/fortelang: lexer/parser/checking, with diagnostic codes). Local `import { X } from "./lib.forte"` (cycle detection, recursive resolution, standalone library validation) also implemented. v1.1 added `automate volume` (linear ramp) + `modulate … with lfo` (parameter modulation) (spec §4.9)** | SRS-LANG-001..006 |
| 0.2 | forte-core: port dawcore's engine/dsp/bounce as a render graph (single-threaded) — 🔶 v0 compiles directly targeting dawcore (graph IR is the next step) | SRS-CORE-001/003/005 |
| 0.3 | `forte build` (WAV + build.manifest.json) and `forte play` (CLI playback) — **✅ `forte check/build/play` implemented. play has file-watch hot reload (recompiled changes applied without stopping playback; on error, the previous version is kept)** | SRS-BLD-001/002 |
| 0.4 | **Determinism CI**: native/wasm output-hash equality gate — **✅ Spike succeeded (07-determinism-spike.md). `scripts/determinism_test.sh` is the prototype of the gate** | SYS-ENG-001 |
| 0.5 | Initial `@std` library (2 synths, EQ, delay, reverb, limiter) — **🔶 8 built-in devices + in-language instrument definition via the `device` syntax (node graph → Grid expansion, demonstrated in `packages/essentials_0.6.0/songs/handmade.forte`). Sound-design expansion: added `noise` (deterministic xorshift) and `shaper` (tanh/clip/fold) to the DSP primitives, making a fully hand-built drum kit possible (`packages/essentials_0.6.0/songs/handmade-kit.forte`). Instrumentalizing recorded takes with `sampler(take: x, root: A3)` — a sound recorded with a microphone becomes a sampler instrument playable chromatically (exactly the white-box principle of SYS-REC-001). **Effect device DSL** (`device X : Effect`, input is `audio.in`, each stereo channel evaluated independently) — fuzz / tremolo / auto-wah can be written in code and inserted as inserts. **Standard instrument library `packages/essentials_0.6.0/instruments/`**: 29 instruments built with the device DSL (drums 10 / bass 5 / keys 5 / pads 4 / leads 5) bundled — all user-space code, so they can be forked and remade. The demo `packages/essentials_0.6.0/songs/std-tour.forte` (10 tracks) is in the determinism gate. **sampler v2**: `start`/`end` (playback-range trim), `loop: "on"` (range looping while the note is held), `reverse: "on"` (reverse playback) — a single recorded take can be sliced into many different instruments (fixed at note-on for determinism; existing songs remain bit-identical). **soundnote**: a `take` slot + `sample()` node in the device DSL — a recorded take becomes a node-graph sound source that can be processed downstream through svf/shaper/adsr. The device holds no take; the user plugs one in, so it can be published/forked as an instrument. **`kit()`**: a drum kit that assigns multiple takes to pads keyed by note names (`kit(C2: kickTake, D2: snareTake)`, played at original speed). All verified bit-identical across native/wasm** | D-06 |
| 0.6 | Port 3 reference songs to code (user-test material) — **✅ 3 songs (`first-light` 4/4, `slow-circles` 6/8, `night-parade` exercising prog/section/send-return/arp). Identical digests across native/wasm** | SYS-LNG-001 acceptance |

**Exit criteria**: two external people with composing experience can write 8 bars relying on the
documentation alone, and the two-environment hashes match.
**Key risk**: determinism (especially numeric equality between wasm/native). → Spike 0.4 in the
first 2 weeks, and if it breaks down, decide early to fall back to "determinism on a single
target (wasm-unified)."

## Phase 1 — Composing Experience (~3 months, cumulative 6 months)

**Goal: establish the experience of "improving a song in VSCode while listening to it."**

| # | Deliverable | Requirements |
| --- | --- | --- |
| 1.1 | forte-lsp (completion, diagnostics, hover) + VSCode extension (playback controls) — **🔶 LSP (diagnostics, completion, hover, formatting) + **Forte Studio** (`editor/vscode-forte`): highlighting, diagnostics, Play/Build/Stop, REPL (Shift+Enter to send), arrangement view, plus sidebar **History** (commit / musical-vocabulary diff / checkout / merge — auto-inits the repository on commit if absent) and **Hub** (list → ▶ direct in-store audition / Fork with history / Publish / verify / lineage). All UI is a thin wrapper over the `forte` CLI (added `forte log --json` / `hub list --json` / `hub entry`)** | SRS-LSP-001/002 |
| 1.2 | Incremental build + hot reload (change → sound within 1 second) | SRS-LANG-007, SRS-CORE-006 |
| 1.3 | Visualization Webview (piano roll/arrangement/meters, read-only + code jump) | SRS-VIS-001/002 |
| 1.4 | MIDI input → pattern recording (transcribing performances as code) — **✅ Browser performance mode (🎹): play with Web MIDI + PC-key keyboard while live monitoring → on stop, transcribed into a `notes` literal with 1/16 quantization, chord grouping, and rest insertion (Rust implementation + unit tests; E2E verifies the generated code compiles)** | SRS-REC-001 |
| 1.5 | Microphone recording v1: `.frec` + provenance + crash recovery + loopback calibration — **🔶 Implemented the `.frec` format + provenance enforcement (missing provenance is compile error E-PROV-001) + `import take from "*.frec"` + the `audio take at …` syntax + engine playback. Browser recording UI implemented (getUserMedia with EC/NS/AGC off → direct AudioWorklet PCM capture → provenance-carrying .frec into OPFS; E2E verified with a fake microphone). Loopback calibration implemented (chirp playback → capture on the same AudioContext clock → sample-accurate round-trip measurement via wasm correlation; the result is recorded in the take's provenance as latency_samples). Crash recovery via sequential OPFS writes (dedicated Worker + SyncAccessHandle, flushed every second) also implemented — even if the tab dies mid-recording, the take is restored on next startup (E2E verified)** | SRS-REC-002..005 |
| 1.6 | `forte test` / `forte fmt` | SRS-LANG-002 |

**Exit criteria**: in a closed trial with 5 composers, each can complete one song including
recording. The qualitative validation of "does composing in code work?" emerges here (the
validation point of the biggest product risk).

## Phase 2 — Hub and Lineage (~4 months, cumulative 10 months)

**Goal: a minimal ecosystem where fork lineage + releases work (closed beta).**

| # | Deliverable | Requirements |
| --- | --- | --- |
| 2.1 | git hosting + authorization layer (**public rejects clone; fork API only**) — **🔶 Local VCS implemented ahead of schedule (`forte init/commit/log/branch/checkout/diff`): a SHA-256 content-addressed object store (blob/tree/commit) held in `.forte/`, tracking `*.forte` + `*.frec` + lineage stamps. `forte diff` is not a line diff but a **semantic diff of the compiled model** ("tempo: 108 → 116 bpm", "track Keys: Polymer wave: square → saw", "bars 13..16: placement removed"). Library edits surface on the importing song's side as "the sound changes." `forte merge` performs a three-way merge (fast-forward / LCS line merge / conflict markers + MERGE_HEAD so the resolution commit records both parents), and additionally **compile-verifies the merge result** — even if the text merges cleanly, it warns if the music is broken. **Repositories in the browser too**: the web editor's History panel (commit / log / musical-vocabulary diff / restore). Object format identical to the CLI (SHA-256 content-addressed, stored in OPFS); semantic diff computed by the wasm compiler's fw_semdiff. Hosting (server-side push/pull) is implemented as the remote hub of 2.2** | SRS-HUB-001/002 |
| 2.2 | Lineage graph DB + public API (fork/depends/performed/released) — **🔶 Implemented ahead of schedule as a local Hub (`forte hub publish/fork/lineage/list`): acquisition is fork-only, provenance stamps go to the fork destination, and re-publish structurally records forked_from. **VCS-integrated**: publish pushes the history with all reachable objects when the repository is clean (recorded in Version.commit); fork comes down with a `.forte` repository, and the fork stamp itself becomes a commit (whose parent is the original author's HEAD) — the lineage is the history itself. forked_from points to the exact commit. **Server-ready**: multiple people can push/pull to a hub run via `forte hub serve` with `--hub http://host:9377`. `forte hub signup` issues tokens (the server stores only SHA-256 hashes); on a hub with registered users, publish requires a token, and the **author is derived from the token** (no impersonation). publish pushes the VCS history objects over HTTP (the server verifies content hashes before storing — the store is content-addressed regardless of who pushes); remote fork brings the history down and commits a lineage stamp — isomorphic to local fork. The HTTP client also uses std only. Integration tests done (a full loop over real HTTP: signup → 401 → publish → fork → re-publish, plus rejection of tampered objects). TLS via a reverse proxy in front. **GitHub backend (the everyday form for individuals to small groups)**: with the definition that a hub is just a git repository, publish/fork/list/release/verify/lineage/serve can all point at `--hub github:you/forte-hub` (/ `git@…:….git` / GitLab / a bare repo on a NAS). Transport is system git, so authentication uses existing git credentials, the author is `git config user.name`, and the ledger's change history also stays in git. Concurrent publishes auto-resolve via push compare-and-swap (reject → sync → replay the operation) — verified through fully offline integration tests against a bare repo up to convergence of concurrent publishes. `serve --hub <git-URL>` serves a synchronized checkout locally, so the browser lineage page works unchanged. Structural fork-only enforcement does not hold on a git host (it degrades to a convention via provenance stamps) — a public hub that needs it is the domain of the authenticated server** | SRS-HUB-003 |
| 2.3 | forte-pkg: dependency resolution via Hub fork + forte.lock | SRS-PKG-001..004 |
| 2.4 | Release pipeline (clean-room deterministic build + hash verification + Opus distribution) — **🔶 Local version implemented (`forte hub release/verify`): a deterministic build from the snapshot records the digest in the ledger, and anyone can verify reproduction (tampering detected as MISMATCH; verification counts shown in the lineage). Distribution (Opus) is the server stage** | SRS-HUB-004/005 |
| 2.5 | Song page (lineage display, code browsing) + web player | SRS-HUB-008, SRS-PLY-001 |
| 2.6 | Playback event ledger (recording only) | SRS-HUB-007 |
| 2.7 | Lineage-preserving license v1 (including legal review) | SYS-GOV-001 |

**Exit criteria**: 30 beta participants, 50 public modules, 10 released songs containing
fork-derived dependencies. The lineage graph can be drawn from real data.

## Phase 3 — Web Editor and Performance Fork (~4 months, cumulative 14 months)

**Goal: "listen → fork → add vocals → release" closes in the browser alone.**

| # | Deliverable | Requirements |
| --- | --- | --- |
| 3.1 | Web editor (Monaco + wasm LSP + AudioWorklet playback + OPFS + PWA offline) — **🔶 Prototype implemented ahead of schedule (`web/` + `crates/forteweb`): as-you-type diagnostics, build-proof digests, AudioWorklet playback + hot reload, read-only arrangement view, OPFS auto-save (multiple songs, persists across reloads), full offline operation via Service Worker (the worklet loads from the SW cache via a blob URL). 7 real-Chromium E2E checks (`scripts/web_e2e.mjs`): browser==native bit-identity, OPFS persistence, and **startup, compile, and playback with the network disconnected** all verified** | SRS-WEB-001..003 |
| 3.2 | open-stems release + **performance fork mode** (minimal recording GUI) — **🔶 "listen → fork → add vocals → publish" completes a full loop in the browser: the hub's POST /api/publish (registers the snapshot after compile verification, CORS-enabled), .frec binaries bundled with publish/fork (clean-room release/verify also holds with takes included), lineage stamp written on browser fork (fixing the provenance gap), record-stop → one-tap import + automatic Voice track addition, the editor's ⇪ Publish. The full loop is E2E-verified (fake-mic recording → insertion → publish → confirm forked_from + bundled takes). **open-stems**: `forte build --stems` (renders per-track WAVs solo with sends included, records per-stem digests in the manifest — determinism-tested) and per-track M/S in Hub audition (fw_set_mute/solo → worklet, toggled during playback, E2E-verified)** | SRS-BLD-003, SRS-REC-006 |
| 3.3 | Safari/Firefox degraded-mode implementation | SRS-WEB-004 |
| 3.4 | Fingerprint matching + report moderation v1 | SRS-HUB-006 |
| 3.5 | Export (zip/git bundle), data portability — **🔶 `forte export <song.forte>`: a self-contained zip (entry + imports + recorded takes + `export.manifest.json` with render digest + when the repository is clean, the `.forte/` history objects + refs + HEAD). Hand-written zip writer (store-only, CRC-32, fixed timestamps) so **the zip itself is byte-for-byte deterministic**. Round-trip test: extract → compile → digest matches the manifest, and `forte log` works on the restored `.forte/` — verified that far** | SRS-WEB-005 |

**Exit criteria**: an event where "10 people add vocal forks to a well-known song's open-stems"
succeeds. This becomes the first proof of the viral pattern (§ listener experience).

## Phase 4 — Discovery and Digging (~3 months, cumulative 17 months)

**Goal: the lineage becomes the listener's experience.**

- 4.1 Lineage-digging UI (fork tree, cross-performer, cross-module "songs using this instrument") — **🔶 Fork family tree implemented ahead of schedule: GET /api/lineage (fork-forest JSON, cycle-safe) + tree display on the hub top page (♪/📚, release and play-count badges, click through to the song page). Multi-generation nesting verified with Rust tests + E2E. **Cross-module**: device names used are recorded at publish (Version.uses); the song page's "instruments used" (linked to the defining library) ⇄ the library page's "songs using this instrument" bidirectionally. **Cross-performer**: click an author name in the list to filter. Remaining: full-text search** |
- 4.2 Similarity search v1 (canonical progression form, modules used, tempo/key) [SRS-PLY-002]
- 4.3 Public launch (open general registration)
- 4.4 Community operations (official module contests, educational content)

## Phase 5 — Economy (timing depends on data)

- 5.1 Point system design (analysis of ledger data → simulation of apportionment rules)
- 5.2 Regulatory review (Payment Services Act, etc. — especially if monetary conversion is involved)
- 5.3 Staged introduction: still recording only → display ("your contribution was listened to X times") → usage-entitlement recirculation → (decision required) monetary conversion

---

## Cross-Cutting Workstreams

- **Determinism CI** (Phase 0 onward, permanent): gate on all PRs.
- **Reference song corpus**: expand each phase; serves as the shared bench for performance/regression/determinism.
- **Security**: from Phase 2, encryption at rest and signing-key management (SRS-SEC-001/002).
- **Documentation**: the language specification and the `@std` reference are treated as part of the product (the primary adoption funnel).

## Key Risk Register

| Risk | Impact | Early validation |
| --- | --- | --- |
| wasm/native numeric determinism fails to hold | Redesign of release verification | Spike Phase 0.4 first |
| The learning curve of "composing in code" is too steep | Collapse of the product premise | Qualitative test at Phase 1 Exit. Invest in visualization and error quality |
| Fork enforcement becomes friction and users retreat into private | Lineage does not accumulate | Measure fork rate in the Phase 2 beta. Make fork UX one click |
| Forged recording provenance | Damaged trust | Complete prevention explicitly impossible. Fingerprint matching + community norms in Phase 3 |
| Failed economy design (monetary-conversion regulation, speculation) | Business risk | Make no irreversible promises before Phase 5 |
| Fast-follow by openDAW and others | Competition | Lineage data and the corpus are the moat. Build the community first |

## Immediate Actions (At Phase 0 Start)

1. ~~Final approval of D-01 (Rust vs C++) and D-02 (custom DSL)~~ ✅ Approved (2026-07-02)
2. ~~Determinism spike: build dawcore's bounce on wasm32 and compare hashes with native~~
   ✅ Succeeded — bit-identical via libm unification (07-determinism-spike.md)
3. Draft language specification v0 (started in spec/forte-lang-v0.md) + hand-port one reference song
4. ~~Decide the project name~~ ✅ Formalized as **Forte** (MIT license. Migrated on 2026-07-04 to the public repository [ForteLang/forte](https://github.com/ForteLang/forte); development continues there)
5. Start implementing the forte-lang parser/type checker (0.1)
