# Forte lang Specification v0 (Draft)

> **📌 For the implementation-conformant specification, see [forte-lang-v1.md](forte-lang-v1.md).**
> This document is retained as a higher-level draft containing design intent and future plans
> (arbitrary-expression DSP, generics, full unit-type checking, etc.).

Status: Draft v0.1 / 2026-07-02
Corresponding requirements: SRS-LANG-001..008 / Parent: 05-detailed-design.md §1

This document is the minimal specification for the Phase 0 implementation. The syntax will be fixed in v1
after field validation via porting the reference song (0.6). **Bold DECISION** items must be settled before implementation.

---

## 1. Design Principles

1. **Everything is a value** — notes, patterns, progressions, tracks, devices, and songs are all values returned by expressions.
2. **Determinism** — runtime I/O, clocks, and unseeded randomness do not exist. A program's meaning is
   a pure mapping to "an event sequence + a render graph".
3. **Two layers** — the Score layer (declarative, fully expanded at compile time) and the DSP layer (a per-sample
   procedural kernel). Provided as subsets of the same language; only the DSP layer has `process`.
4. **Units are types** — beats, seconds, Hz, dB, and pitch are distinct types. Mixing them with bare numbers is a compile error.
5. **diff-able** — a single canonical form via `forte fmt`. One file, one module.

## 2. Lexical Structure

- Encoding: UTF-8. Identifiers: `[a-zA-Z_][a-zA-Z0-9_]*` (v0 is ASCII only).
- Comments: `//` line, `/* */` block.
- Numeric literals allow unit suffixes: `92bpm`, `4bars`, `1/8beat`, `440Hz`,
  `-0.3dB`, `20ms`, `0.5` (dimensionless).
- Music literals (backquote DSL, §5):
  `beat` … step sequence / `notes` … note sequence / `prog` … chord progression.
- Pitch literals: `C4`, `F#3`, `Bb2` (octaves with middle C = C4).

## 3. Type System (v0 core)

```
Basic     : Bool, Int, Float, String (compile-time only)
Unit-typed: Beats, Bars, Sec, Hz, Db, Bpm, Pitch, Velocity (0..1)
Musical   : Note{pitch, start: Beats, dur: Beats, vel},
          Pattern = List<Note> (with length: Beats),
          Chord, Progression = List<(Chord, Beats)>
Signal    : Audio (channel count is a type parameter: Audio<1>, Audio<2>), Control
Structural: Track, Bus, Song, Section
Devices   : Instrument (Note→Audio), Effect (Audio→Audio), NoteFx (Pattern→Pattern)
Assets    : RecordedAudio (provenance-verified microphone recordings only. §8)
Generic   : List<T>, Map<K,V>, Option<T>, function types (T)->U, record types {a: T, b: U}
```

- Conversions are explicit only: `beats(2.0)`, `sec(1.5)`, `(1/8).beats * 3`, etc.
  The `Beats → Sec` conversion is possible only in a context where tempo is fixed (inside a song).
- **DECISION-T1**: scope of generics (the recommended proposal is to start v0 with only `List<T>` and monomorphized functions,
  deferring user-defined generics to v1).

## 4. Modules and import

```forte
// External dependencies (resolved via [deps] in forte.toml and forte.lock)
import { tr909, Kick }  from "@rhythm/tr909@^2.1"
// Local
import { hook }         from "./sections/hook.forte"
// Recording asset (provenance verification runs at compile time)
import vocalTake        from "../assets/vocal_take3.frec"
```

- Circular imports are an error. Publishing uses the `pub` keyword.
- A module's top level contains declarations only (no expression execution).
- Publishing to the public registry requires source (SRS-PKG-003).

## 5. Score Layer

### 5.1 Song Structure

```forte
pub song "Aozora" {
  tempo 92bpm
  meter 4/4
  key   D maj

  let kick   = beat`x--- x--- x-x- x---`          // 1 bar, 16th-note resolution
  let chords = prog`Dmaj7 | Bm7 | Em7 A7`          // '|' is a bar separator

  section verse = bars(1..16)
  section hook  = bars(17..32)

  track Drums {
    instrument tr909(kick: .deep)
    play kick at verse.repeat()                     // repeat across the whole section
  }

  track Keys {
    instrument juno(voices: 8)
    play arp(chords, style: .updown, rate: 1/8beat) at hook
    automate cutoff from 0.2 to 0.8 over hook       // parameter names are type-checked
  }

  track Vocal {
    audio vocalTake at bars(17)                     // placement of RecordedAudio
    insert comp(ratio: 3.0, threshold: -18dB)
  }

  bus Master {
    insert limiter(ceiling: -0.3dB)
  }
}
```

### 5.2 Semantics

- The `song` block is evaluated at compile time and fully expanded into an **event sequence** (sample-accurate
  notes / automation / clip placement) and a **render graph** (the connections between instrument/effect/bus).
- Control structures (`let` / `fn` / `if` / `for` / `map` / `repeat`) are all compile-time.
  The only thing evaluated at runtime is the DSP layer's `process`.
- Default routing: `track` → implicit `Master`. send/return is a
  `return Space { insert reverb(...) }` block + `send Space 0.35` inside a track
  (**DECISION-S1 resolved — conforms to the v0 implementation**).
- Randomness: only the pure generator returned by `random(seed: 42)`. The seed cannot be omitted.
- Since `song` is a value, a function can return a `Song`, variations can be built with `map`, etc.
  (algorithmic composition goes through this path).

### 5.3 Meaning of Music Literals

- `beat` … `x` = hit, `-` = rest, `X` = accent, whitespace = grouping (no meaning).
  Resolution is inferred from the literal length (one bar divided equally). **DECISION-S2**: non-power-of-two divisions such as triplets.
- `notes` … `notes\`C4:1/4 E4:1/4 G4:1/2\`` (pitch:length).
- `prog` … chord names and `|` (bar separator. Multiple chords within one bar divide the time equally).
  Becomes a `Progression` value, input to the pattern functions `chords(x)` / `arp(x, rate:, style: "up|down|updown")` /
  `bass(x, rate:)`. A bare `prog` sounds as block chords.
  Qualities: (major), m, min, 7, maj7, m7, min7, dim, aug, sus2, sus4.
  **The fact that progressions are first-class values is the foundation of similarity search (SRS-PLY-002)**.
- `section verse = bars(1..8)` defines a named range, referenced with `play x at verse`.

## 6. DSP Layer

```forte
pub device MonoSaw : Instrument {
  param cutoff: Hz = 800Hz in 20Hz..18kHz    // Hub/visualization derives the UI from this declaration
  param res:    Float = 0.3 in 0.0..0.99

  state phase: Float = 0.0
  state svf:   Svf   = Svf.lowpass()

  on note_on(n)  { phase = 0.0 }

  process(out: &mut Frame<1>, ctx: &Ctx) {
    let s = std.osc.saw_blep(&mut phase, ctx.pitch_hz)
    out[0] = svf.run(s, cutoff, res) * ctx.env()
  }
}
```

- `process` is the only runtime code, called once per sample (or per frame).
  No allocation, recursion, or infinite loops (a syntactic subset whose termination can be guaranteed at compile time:
  only bounded `for`).
- `state` is duplicated per voice. `param` can be `automate`d from the Score layer.
- Math functions come only from `std.math` (= forte-core's dmath, fixed to libm).
  **The determinism spike (07) has demonstrated native/wasm bit-identity under this convention.**
- The v0 implementation starts as an interpreter (a Rust operator tree) (SDD §2).

### 6.1 v0 Implemented Subset: node-graph devices

Ahead of arbitrary-expression `process`, **devices as declarative node graphs** are implemented in v0
(expanded into a per-voice interpreter = dawcore Grid):

```forte
device WarmLead : Instrument {
  param cutoff = 0.6 in 0.0..1.0       // call site: instrument WarmLead(cutoff: 0.7)
  node o   = osc(shape: "saw")          // when freq is omitted, note.freq
  node env = adsr(a: 0.03, d: 0.25, s: 0.6, r: 0.3)   // when gate is omitted, note.gate
  node f   = svf(in: o, cutoff: cutoff, reso: 0.3, mod: lfo(rate: 0.25))
  out gain(in: f, mod: env, level: 0.9)
}
```

- Primitives: `osc(shape, freq)` / `lfo(rate, shape)` / `adsr(a,d,s,r, gate)` /
  `svf(in, cutoff, reso, mod)` / `gain(in, level, mod)` / `mix(a, b)`.
  Signal sources: `note.freq` / `note.gate` / `note.vel`, declared `node` names, nested calls.
- `param` is **bound at instantiation time** (compile-time constant). The range is checked via `in lo..hi`
  (default 0..1). Runtime automation support lands in forte-core.
- Forward references are disallowed (E-GRID-002). All numeric arguments are normalized 0..1.

## 7. Standard Library `@std` (included in v0)

| Module | Contents (source of the port from dawcore) |
| --- | --- |
| std.osc | polyBLEP saw/square/tri, sine (oscillator.rs) |
| std.env | ADSR (envelope.rs) |
| std.filter | TPT SVF, OnePole (filter.rs) |
| std.fx | delay, FDN reverb, drive, EQ3, limiter (effects.rs) |
| std.inst | Polymer-equivalent reference synth (synth.rs/voice.rs) |
| std.note | arp, transpose, repeat, quantize (NoteFx from device.rs) |
| std.math | sin/cos/tan/exp/tanh/powf (dmath.rs) |
| std.rand | seeded xorshift |

The sampler (sampler.rs) will be ported restricted to `RecordedAudio` only (no external-file
playback. SYS-REC-001).

## 8. Asset References and Provenance

- `import x from "*.frec"` has type `RecordedAudio`. At compile time it verifies
  (a) the existence of the CAS entity, (b) the ed25519 signature, (c) `device_class ∈ {microphone}`.
  Failure is `E-PROV-001`.
- The `.frec` pointer format is defined in SDD §4.1.

## 9. Diagnostics and Errors

- Error code scheme: `E-TYPE-*` (types), `E-TIME-*` (beat/second inconsistencies),
  `E-PROV-*` (provenance), `E-DSP-*` (process constraint violations), `E-MOD-*` (import).
- Messages use musical vocabulary (SRS-LANG-008). Example:
  `E-TIME-002: Track 'Vocal', bar 3: Pattern (worth 3/4 beats) does not match meter 4/4`

## 10. Grammar Sketch (EBNF excerpt)

```
file        := { import } { decl }
import      := "import" ( "{" ident {"," ident} "}" | ident ) "from" string
decl        := ["pub"] ( song | device | fnDecl | letDecl )
song        := "song" string "{" { songItem } "}"
songItem    := tempo | meter | key | letDecl | sectionDecl | track | bus | route
track       := "track" ident "{" { trackItem } "}"
trackItem   := instrument | audioPlace | play | insert | automate
device      := "device" ident ":" deviceKind "{" { param | state | handler | process } "}"
play        := "play" expr "at" expr
automate    := "automate" ident "from" expr "to" expr "over" expr
expr        := literal | musicLit | ident | call | lambda | binop | ...
musicLit    := ("beat"|"notes"|"prog") "`" raw "`"
```

## 11. Not Built in v0 (v1 and later)

- User-defined generics, trait-like abstraction
- Macros / metaprogramming
- Real-time input reflection in the Score layer (live coding) — possible by design, but deferred
- MIDI 2.0, microtuning (the Pitch type leaves room for extension)

## 12. List of Open Decisions

| ID | Content | Deadline |
| --- | --- | --- |
| DECISION-T1 | Scope of generics | Before parser implementation |
| ~~DECISION-S1~~ | ~~send/return routing syntax~~ → **Resolved: `return Name {}` + `send Name level`** | Done |
| DECISION-S2 | beat-literal representation of non-power-of-two divisions (triplets) | When porting the reference song |
| DECISION-S3 | First-class expression of `section` repetition (A-B-A) (plain `section` is implemented) | Same as above |
| DECISION-D1 | Frame granularity of `process` (1 sample vs. small blocks) | When implementing the interpreter |
