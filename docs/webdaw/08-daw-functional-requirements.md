# DAW Functional Requirements (DAW-FR) — disposition of the full DAW surface

Status: Draft v0.1 / 2026-07-14
Upstream documents: 01-vision.md, 03-software-requirements.md (SRS)
Downstream: Forte Studio requirements (issue #135), future SRS revisions

## 0. Purpose and method

The web editor prototype proves the pipeline (edit → diagnose → play →
prove) but covers a tiny fraction of what a DAW user actually does all
day. Before Forte Studio can be specified, the **whole** surface of a
baseline modern DAW must be on the table, and every operation must be
given an explicit disposition in Forte's model — otherwise the GUI gets
designed by vibes and the philosophy leaks.

Method: the capability list below is the union of the baseline (non-
flagship-exclusive) operations of Ableton Live 12, Logic Pro 11,
Cubase 14, REAPER 7, Bitwig Studio 5 and FL Studio 24. Marketing
features unique to one product are omitted; anything two or more of
them treat as table stakes is in.

### Disposition key

| Code | Meaning |
| --- | --- |
| **CODE** | The operation already IS forte source. The mapping is cited. GUI may still surface it, but the language needs nothing. |
| **CODE-GAP** | Belongs in the language and is missing today. Each gap becomes a language issue before or during Studio work. |
| **GUI** | Needs a graphical editing surface in Studio. GUI edits are **projections**: they read the code and write clean, minimal-diff code back. A GUI row always has a code representation underneath. |
| **TOOL** | Ephemeral workspace/audition state (zoom, solo-while-listening, meters, playhead). Deliberately **never serialized into the song file**; at most a per-user workspace file that is gitignored. |
| **NO** | Deliberately not done. The rationale is part of the requirement — these rows define the product as much as the others. |

### The three laws these dispositions follow

1. **Code is the single source of truth** (vision §3). Every persistent
   musical decision must exist as readable, diffable `.forte` source. A
   GUI gesture with no code representation is either TOOL (ephemeral) or
   forbidden.
2. **Determinism is sacred** (SRS-LANG-003, SRS-CORE-003). Anything that
   cannot render bit-identically from source — black-box plugin
   binaries, free-running randomness, wall-clock — is NO, not CODE-GAP.
3. **Provenance over convenience** (vision §3, SRS-REC). External audio
   is not banned; *unattributed* audio is. Recorded/imported material
   enters only as content-hashed assets with lineage (`.frec` /
   `asset`), never as loose files.

---

## 1. PRJ — Project & session management

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-PRJ-01 | create/open/save a project | CODE | A project is a directory of `.forte` files (+ `forte.toml`). Save = file save. Studio opens a folder, like an IDE. |
| DAW-PRJ-02 | autosave & crash recovery | TOOL | Editor buffer journal. Durable history is git; no shadow project format. |
| DAW-PRJ-03 | project templates | CODE | A template is a package you fork (`forte hub fork`). No binary template format. |
| DAW-PRJ-04 | "collect all & save" / media consolidation | CODE | `forte.lock` + content-hashed deps already pin everything; `bounce`/`dig` renders are cache, not media. Nothing to collect. |
| DAW-PRJ-05 | project version snapshots | NO | Proprietary snapshot systems duplicate git badly. Git IS the history; Studio surfaces status/diff/commit (DAW-HIS-02). |
| DAW-PRJ-06 | per-project audio settings (SR/bit depth) | CODE | Build/render settings in source or manifest; digests depend on them, so they must be in code, not app preferences. |
| DAW-PRJ-07 | open several projects side by side | GUI | Multiple workspace windows. |
| DAW-PRJ-08 | import a track/section from another project | CODE | `import` of blocks/devices; `dig()` for the audio of another song. Strictly better than any DAW's track import. |

## 2. TRK — Tracks & routing

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-TRK-01 | create/delete/rename/reorder tracks | GUI | Projection of `track` statements; reorder = reorder source lines. |
| DAW-TRK-02 | track types: instrument, audio, return, group/bus, master | CODE | `track` + device = instrument track; sampler tracks = audio; send-return and buses exist; master = song-level insert chain. |
| DAW-TRK-03 | track folders / visual grouping | TOOL | Pure view state (collapse/color). Musical grouping is a bus (CODE). |
| DAW-TRK-04 | mute/solo a track while listening | TOOL | Audition state, never in the file. "Permanently muted" = the code doesn't play it. |
| DAW-TRK-05 | arm/record-enable | — | Recording subsystem (SRS-REC), out of Studio v1 scope. |
| DAW-TRK-06 | freeze/unfreeze tracks | TOOL | Engine-side render cache (#132). Invisible and automatic; never user-managed files. |
| DAW-TRK-07 | flexible routing (any output → any input) | CODE | Sends, buses, `duck(from:)` keyed sidechain. Cycles rejected at compile time. |
| DAW-TRK-08 | per-track input monitoring | — | SRS-REC scope. |

## 3. ARR — Arrangement & timeline

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-ARR-01 | place a clip at a bar | CODE | `play Block at N`. The GUI drag writes the placement. |
| DAW-ARR-02 | move/copy/duplicate clips | GUI | Rewrites `play` statements; duplicate-with-alias covered by `as` aliases. |
| DAW-ARR-03 | loop a clip over a span | CODE | Placement span > block length loops; shorter truncates. Already semantics, not GUI magic. |
| DAW-ARR-04 | split a clip at the playhead | GUI + CODE-GAP | GUI op; needs a clean code form (a `play` range/offset syntax) so a split is two readable placements, not a duplicated block. |
| DAW-ARR-05 | trim clip start/end | GUI + CODE-GAP | Same placement-range syntax as ARR-04. |
| DAW-ARR-06 | crossfade two adjacent audio clips | CODE-GAP | Sampler-domain crossfade at placement boundaries (declick exists; musical-length crossfades don't). |
| DAW-ARR-07 | section markers with names | CODE | `section` statements; already drive `forte analyze`'s energy arc. |
| DAW-ARR-08 | rearrange song sections wholesale (arranger track) | GUI | Sections/placements are text; the GUI reorders spans and rewrites bar numbers. |
| DAW-ARR-09 | ripple-edit (insert/delete time) | GUI | Batch rewrite of `at` positions and `automate` ranges. Needs care: automate ranges are absolute bars. |
| DAW-ARR-10 | loop braces / punch region for listening | TOOL | Transport state. |
| DAW-ARR-11 | tempo-synced grid snapping | TOOL | View behavior over CODE positions (bars/beats are already the language's coordinates). |

## 4. NOTE — Note editing (piano roll / drum grid)

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-NOTE-01 | draw/erase/move/resize notes on a piano roll | GUI | Projection of `notes` strings. THE hardest projection: edits must write back idiomatic notes syntax, not exploded garbage. |
| DAW-NOTE-02 | step-sequence drums on a 16-grid | GUI | Projection of `beats` (16-token literals map 1:1 to a grid row — this one is easy and should be first). |
| DAW-NOTE-03 | edit velocities per note | CODE + GUI | Velocity syntax exists in patterns; GUI = lane under the roll. |
| DAW-NOTE-04 | quantize notes (destructive) | GUI | A refactoring: rewrites note positions in source. |
| DAW-NOTE-05 | groove/swing (non-destructive) | CODE | `swing` templates (mpc54/58/62/66, shuffle, straight). |
| DAW-NOTE-06 | humanize timing/velocity | CODE | `humanize`/feel — deterministic, seeded. |
| DAW-NOTE-07 | enter/edit chords by name or degree | CODE | Theory stdlib: `prog`, roman-numeral degrees, qualities, slash chords, voiced leading. Stronger than any surveyed DAW. |
| DAW-NOTE-08 | arpeggiator | CODE-GAP | A pattern-transformer in the stdlib (seeded, compile-time), not a realtime MIDI FX. |
| DAW-NOTE-09 | scale highlighting / fold to scale | TOOL | View filter over the song's `key`. |
| DAW-NOTE-10 | ghost notes from other tracks | TOOL | View overlay. |
| DAW-NOTE-11 | randomize/probability per note | CODE-GAP | Only as seeded, compile-time variation (`vary` exists on sampler params; note-level needs a seeded form). Free-running probability is NO (law 2). |
| DAW-NOTE-12 | MPE / per-note expression | NO (v1) | No controller story yet; revisit with SRS-REC hardware work. Determinism is not the blocker. |
| DAW-NOTE-13 | CC / pitch-bend lanes | CODE | `automate`/`modulate` on device params is the general form; per-note pitch events are the sampler/glide vocabulary. |

## 5. AUD — Audio editing & sampling

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-AUD-01 | import an audio file | CODE-GAP | The `asset` concept (SRS-LANG-001): content-hashed, provenance-stamped, in the lock file. Loose WAV drag-in with no lineage is NO (law 3). |
| DAW-AUD-02 | slice audio to a grid / to transients | CODE | `sampler(slices:, choke:)`, `start`/`end`, slice = note. |
| DAW-AUD-03 | reverse, stutter, gate, tape-stop | CODE | Sampler + inserts (`stutter`, `gate`, `tapestop`) — the package's whole aesthetic. |
| DAW-AUD-04 | time-stretch without pitch change | CODE | `stretch` (granular). |
| DAW-AUD-05 | repitch audio (with time) | CODE | `pitch` / `semis` — audio-domain, formant-dragging, as glitch demands. |
| DAW-AUD-06 | bounce/render in place | CODE | `bounce(Block, beats:)` — a first-class language operation, not a menu command. |
| DAW-AUD-07 | sample another finished song | CODE | `dig(path, beats:, skip:, semis:, key:, tempo:)` — no surveyed DAW has this. |
| DAW-AUD-08 | destructive waveform editing (pencil, normalize, silence) | NO | Sources are renders of code; editing rendered audio breaks the render→source chain. Edit the code and re-render. |
| DAW-AUD-09 | comping / take lanes | — | SRS-REC scope (performance forks). |
| DAW-AUD-10 | corrective vocal tuning (Melodyne-style) | CODE-GAP (post-v1) | Only as a deterministic effect over provenance-stamped assets. Far future; explicitly not Studio v1. |
| DAW-AUD-11 | audio-to-MIDI | NO (v1) | Analysis exists (`analyze` chroma/onsets) but auto-transcription invites the "AI generates, human can't read it" failure. Revisit only as an assistive, review-diff workflow. |

## 6. INS — Instruments & sound design

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-INS-01 | play built-in synths (subtractive/FM/etc.) | CODE | `device` definitions, grid nodes (osc, `vcf`, `uni`, `pan`…), prisma. |
| DAW-INS-02 | build/patch your own instrument | CODE | The grid IS a modular; devices are source modules. This is the product's centerpiece, not a feature. |
| DAW-INS-03 | sampler instruments / multisample kits | CODE | `sampler`, `kit(C1:…, wrap:)`. |
| DAW-INS-04 | browse & load presets | CODE + GUI | A preset is a device module you `import` (or fork). GUI browses packages; "save preset" = extract device to file. |
| DAW-INS-05 | macro knobs over many params | CODE-GAP | Named param groups on a device exposing N params as one control; needed for GUI knobs worth turning. |
| DAW-INS-06 | host third-party plugins (VST/AU/CLAP) | **NO** | The cornerstone rejection. Black-box binaries break bit-identical builds, fork lineage and white-boxing (laws 1–3). Instruments/effects exist only as source modules. This is load-bearing for the whole vision; it is why the Hub can promise reproducibility. |
| DAW-INS-07 | microtuning / alternative temperaments | CODE-GAP (post-v1) | Tuning tables in the theory stdlib eventually; not Studio v1. |

## 7. MIX — Mixing console & effects

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-MIX-01 | fader per track, master fader | CODE | `level` (track solo-render LUFS target; song-level 3-pass master target). Declarative — states the goal, not the knob. |
| DAW-MIX-02 | pan per track | CODE | Track-level `pan` statement (wired through compile → engine), plus the `pan` node inside instruments for per-voice fields. |
| DAW-MIX-03 | insert chain per track, reorderable | CODE + GUI | `insert` lines in order; GUI drag reorders lines. |
| DAW-MIX-04 | full effect palette (EQ, comp, reverb, delay…) | CODE | eq, parcomp, glue, limiter, space/reverb, drive, saturate, crush, exciter, vinyl, filter, chorus-class (uni), etc. Gaps become core-library issues as found (that pipeline works — see #123–#134). |
| DAW-MIX-05 | visual EQ curve editing | GUI | Curve is a projection of eq params; drag writes params back. |
| DAW-MIX-06 | sidechain compression/ducking | CODE | `duck(from:, mode:"key", amount:)` + glue sc HPF. |
| DAW-MIX-07 | sends with pre/post options | CODE (partial) | Send-return exists; pre/post fader semantics need pinning once track pan/fader (MIX-02) lands. |
| DAW-MIX-08 | mixer view (all channels side by side) | GUI | Read-write projection of tracks/levels/inserts/sends. |
| DAW-MIX-09 | plugin latency compensation | CODE | Engine invariant (e.g. glue lookahead, oversampler latency are internally matched). User never manages it. |
| DAW-MIX-10 | A/B a mix against a reference | CODE + TOOL | Profiles (`analyze --against`) make the reference a JSON target in the repo — better than ear-only A/B. Instant audio A/B toggle is TOOL. |
| DAW-MIX-11 | loudness/peak/spectrum/correlation meters | TOOL | Live meters in Studio; the committed truth is `forte analyze` (DAW-MTR). |

## 8. AUTO — Automation & modulation

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-AUTO-01 | draw automation over time | CODE + GUI | `automate p from A to B over bars(N..M)`; GUI curve lane writes these. |
| DAW-AUTO-02 | curved/stepped automation shapes | CODE-GAP | Only linear segments exist; needs curve/step/hold shapes to make GUI-drawn curves round-trip. |
| DAW-AUTO-03 | LFO/step/envelope modulators on any param | CODE | `modulate p with lfo/steps(seq:, every:, amount:)`. |
| DAW-AUTO-04 | per-clip (block-local) automation | CODE-GAP | `automate` ranges are absolute song bars; block-local automation that travels with the block is missing and Studio's clip view will need it. |
| DAW-AUTO-05 | record knob movements (touch/latch/write) | NO | Capturing mouse performances as automation is the opaque-project workflow. Composition is written; performance capture belongs to SRS-REC as provenance, not to the song source. |
| DAW-AUTO-06 | mixer snapshots / scene morphing | NO (v1) | Snapshot state outside code violates law 1; if ever, as named param-set modules. |

## 9. TMP — Tempo, meter & groove

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-TMP-01 | set tempo & time signature | CODE | Song header (4/4, 6/8 shipped). |
| DAW-TMP-02 | tempo changes/ramps mid-song (tempo track) | CODE-GAP | Per-section tempo map. Engine supports maps (SRS-CORE-005); the language doesn't yet. |
| DAW-TMP-03 | meter changes mid-song | CODE-GAP | Same shape as TMP-02. |
| DAW-TMP-04 | apply groove templates | CODE | `swing` templates. |
| DAW-TMP-05 | tap tempo | TOOL | Sets the number the user then writes. |
| DAW-TMP-06 | detect tempo of audio | — | Asset-import concern (AUD-01), with `dig(tempo:"match")` as the in-repo precedent. |

## 10. BRS — Browser & library

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-BRS-01 | browse instruments/effects/clips library | GUI | Over imports + installed packages + Hub. Blocks, devices, songs, profiles are all first-class entries. |
| DAW-BRS-02 | search by name/tag/type | GUI | Hub metadata + package manifests. |
| DAW-BRS-03 | audition an item before using it | TOOL | One-click solo render of a block/device demo (cached, deterministic). |
| DAW-BRS-04 | drag item into project | GUI | Writes the `import` + `play`/`insert` lines. |
| DAW-BRS-05 | favorites/collections | TOOL | Per-user workspace state. |
| DAW-BRS-06 | preset hot-swap while playing | GUI | Edit + hot reload (SRS-CORE-006) already gives this. |

## 11. TRN — Transport & monitoring

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-TRN-01 | play/stop/continue from position | TOOL | `forte play` engine; position is transport state. |
| DAW-TRN-02 | loop playback of a region | TOOL | Transport state (ARR-10). |
| DAW-TRN-03 | scrub / audition at position | TOOL | Deterministic render makes scrubbing exact, not approximate. |
| DAW-TRN-04 | metronome / count-in | TOOL | Click on the monitor path only; never in renders. |
| DAW-TRN-05 | playhead follows in all views | TOOL | Studio UI. |

## 12. MTR — Metering & analysis

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-MTR-01 | LUFS/true-peak/crest metering | CODE + TOOL | `forte analyze` (BS.1770, 4x TP, crest) is the committed measurement; Studio panels are its live view. |
| DAW-MTR-02 | spectrum & band-balance view | CODE + TOOL | 5-band shares + chroma from analyze. |
| DAW-MTR-03 | stereo field / correlation view | CODE + TOOL | mid/side + per-track masking overlap from analyze. |
| DAW-MTR-04 | song structure/energy overview | CODE + TOOL | `section` energy arc from analyze — most DAWs don't have this. |
| DAW-MTR-05 | compare mix to genre targets | CODE | Profiles + `--against` deltas; a profile miss is a work order with a number. |

## 13. EXP — Export & publishing

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-EXP-01 | render master to WAV | CODE | `forte build` + manifest + digest (the build proof). |
| DAW-EXP-02 | export stems | CODE-GAP | Engine already solo-renders per track (analyze); needs a `forte build --stems` surface. |
| DAW-EXP-03 | export a region/loop only | CODE | `bounce` of the block; or build with a range (small gap if range export is wanted). |
| DAW-EXP-04 | MP3/FLAC/dither options | CODE-GAP | Encoder flags on build. WAV digest stays the canonical proof; encoded files are derived artifacts. |
| DAW-EXP-05 | batch/queue exports | GUI | Studio task runner over builds (renders are deterministic and cacheable). |
| DAW-EXP-06 | publish/release a song | CODE | `forte hub publish/release/verify` — release digest = anyone can audit. No DAW has an equivalent. |

## 14. HIS — Undo, history & collaboration

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-HIS-01 | unlimited undo/redo | GUI | Text-editor undo across all views (a GUI gesture = a text edit = undoable). One stack, since everything is one medium. |
| DAW-HIS-02 | browse project history | GUI | Git log/diff/blame surfaced musically ("bars 33–48 changed, Keys track"). |
| DAW-HIS-03 | collaborate on a project | CODE | Git branches/merges + Hub fork lineage. Text merges where DAWs have none. |
| DAW-HIS-04 | real-time co-editing (Google-Docs style) | NO (v1) | Merge-based collaboration first; live CRDT editing only if ever proven necessary. |

## 15. PERF — Live performance / session view

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-PERF-01 | clip-launch grid / scenes | NO (v1) | Studio v1 is a composition tool. Performance is a separate mode with its own provenance story (a performance is a fork/`.frec`, SRS-REC); bolting a launcher on now would fork the product's identity. |
| DAW-PERF-02 | follow actions / generative clip chaining | CODE-GAP (post-v1) | Only as seeded, compile-time sequence generators; free-running randomness is NO. |
| DAW-PERF-03 | DJ-style crossfader | NO (v1) | Same reasoning as PERF-01. |

## 16. HW — Hardware & external world

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-HW-01 | audio interface I/O selection | TOOL | Monitor path only; renders never depend on the device (determinism). |
| DAW-HW-02 | play a MIDI keyboard into a device | — (v1: audition only) | Live audition without recording is TOOL; recording performances is SRS-REC. |
| DAW-HW-03 | control surfaces / MIDI mapping | NO (v1) | Revisit after macro params (INS-05) exist to map onto. |
| DAW-HW-04 | sync to external clock (Link/MTC) | NO (v1) | Performance-mode concern. |
| DAW-HW-05 | ReWire/inter-app audio | NO | Black-box audio exchange breaks provenance (law 3). `dig`/assets are the sanctioned imports. |

## 17. UIX — General editor UX

| ID | A DAW lets you… | Disposition | Forte mapping / rationale |
| --- | --- | --- | --- |
| DAW-UIX-01 | zoom/scroll/track heights/colors | TOOL | Workspace file, gitignored. |
| DAW-UIX-02 | key commands, customizable | TOOL | Studio settings. |
| DAW-UIX-03 | dark/light, layout panes | TOOL | Studio settings. |
| DAW-UIX-04 | video track for scoring | NO | Out of domain. |
| DAW-UIX-05 | notation/score view | NO (v1) | The code is the score. A read-only engraving view could exist someday; editable notation never (two writable representations violate law 1). |
| DAW-UIX-06 | integrated help/manual | GUI | Language reference + device docs in-app (they're markdown in the repo). |
| DAW-UIX-07 | Japanese/English UI | GUI | CLI stays Japanese (repo rule); Studio ships ja + en. |

---

## 18. Summary of dispositions

**The rejections that define the product (NO):** third-party plugin
hosting (INS-06 — the cornerstone), destructive audio editing (AUD-08),
automation performance-capture (AUTO-05), proprietary snapshots
(PRJ-05), mixer scenes outside code (AUTO-06), editable notation
(UIX-05), inter-app audio (HW-05), video (UIX-04). Each is a direct
consequence of the three laws, not a resource decision.

**Deferred, not rejected (NO v1):** MPE (NOTE-12), audio-to-MIDI
(AUD-11), real-time co-editing (HIS-04), performance/session mode
(PERF-01..03), control surfaces & sync (HW-03/04), vocal tuning
(AUD-10), microtuning (INS-07).

**Language gaps to file as issues (CODE-GAP):**

| Gap | Rows | Shape |
| --- | --- | --- |
| Placement range/offset (split & trim) | ARR-04/05 | `play B at N from bar X to Y` or similar |
| Musical crossfades at boundaries | ARR-06 | sampler-domain fade pairs |
| Arpeggiator / pattern transformers | NOTE-08 | stdlib, seeded, compile-time |
| Seeded note-level variation | NOTE-11 | `vary`-for-notes with explicit seed |
| Asset import w/ provenance | AUD-01 | `asset` (content hash, lineage) — biggest single gap |
| Macro params on devices | INS-05 | named param groups |
| Per-track pan (and pre/post send semantics) | MIX-02/07 | track statement param |
| Automation curve shapes | AUTO-02 | curve/step/hold segment kinds |
| Block-local automation | AUTO-04 | ranges relative to block, travel with `play` |
| Tempo/meter maps | TMP-02/03 | per-section tempo/meter statements |
| Stems + encode + range export | EXP-02/03/04 | build flags |

**Where Studio's GUI effort actually goes (GUI):** the drum grid
(NOTE-02, easiest 1:1 projection — build first), the piano roll
(NOTE-01, hardest round-trip — spike early), arrangement timeline
(ARR-01..09), mixer view (MIX-08), EQ curves (MIX-05), browser
(BRS-01..06), git surfaces (HIS-01/02), export runner (EXP-05), and the
analyze panels (MTR-01..05) that no other DAW can honestly offer.

## 19. Traceability

- Laws 1–3 trace to vision 01 §3 and SRS-LANG-003/SRS-CORE-003/SRS-REC.
- CODE rows cite implemented language surface (spec/forte-lang-v1.md).
- CODE-GAP rows become numbered issues before implementation starts;
  each issue references its DAW-FR row.
- Studio (issue #135) scopes its phases against the GUI and TOOL rows
  of this document; its P0 CST spike is a prerequisite for every GUI
  row marked "projection".
- **P0 spike status (2026-07)**: the lossless edit layer exists as
  `fortelang::edit` / `forte edit` — span-anchored token splices with a
  re-parse guard, covering set_tempo, set_pattern (drum grid / piano
  roll write-back), move_place / move_play / add_place / remove_place
  (arrangement), set_arg (inspector knobs), set_track / set_send (the
  mixer's volume / level / pan / send write path, MIX-01/02) and
  set_section. Contract
  tests assert byte-identity outside the splice, comment survival and
  idempotence (`crates/fortelang/tests/edit.rs`). The first live GUI
  projection is the web editor's **beat grid** (NOTE-02): `beat`
  literals render as clickable step rows (fw_pattern_sites) and every
  click writes back through fw_edit — verified end-to-end in Chromium
  (grid renders, one-line write-back, cycle returns byte-identical).
  The arrange view is writable too (ARR-01/02): dragging a clip snaps
  to bars and re-places its play statement via `move_at_line` (clips
  already know their source lines), with a dashed drop ghost while
  dragging — drag + drag-back verified in the same E2E run.
