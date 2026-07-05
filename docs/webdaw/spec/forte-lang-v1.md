# Forte lang Specification v1

Status: **Implementation-conformant** (this document precisely describes the language accepted by the implementation in the repository).
The v0 draft (forte-lang-v0.md) is retained as a higher-level document containing design intent and future plans.
Corresponding implementation: `crates/fortelang` (parser/checker/compiler); verification: `cargo test -p fortelang`.

---

## 1. File Structure

```
file := { import } { device | block } [ song ]
```

- A file with a `song` = a **song** (legacy alias: a named root block —
  structurally a song IS a block).
- A file with top-level `block`s (and no song) = a **block library**:
  importable, and directly buildable — `forte build`/`play` use the LAST
  top-level block as the root.
- A file with neither = a **device library** (importable; `forte check`
  instantiates every device with default values and verifies it).
- Evaluation is entirely compile-time. Runtime I/O, clocks, and unseeded randomness do not exist in the language.

## 2. Lexical Structure

- Encoding is UTF-8. Identifiers: `[A-Za-z_@][A-Za-z0-9_@#]*`.
- Comments: `// to end of line`, `/* … */` (multi-line allowed).
- Numbers: `12`, `0.5`; negation is a prefix `-`. **Unit suffixes** are written immediately adjacent to the number:
  e.g. `96bpm` (only `bpm` carries meaning in v1; others are not ignored but are subject to checking).
- Strings: `"…"` (single line).
- Music literals: a backquote `` `…` `` immediately following `beat` / `notes` / `prog` (multi-line allowed).
- Symbols: `{ } ( ) : , / - .. . =`

## 3. Grammar (EBNF, implementation-conformant)

```ebnf
file      = { import } { device | block } [ song ] ;
block     = "block" ident [ ":" ident ] body ;                       (* [: Parent] inherits *)
import    = "import" "{" ident { "," ident } "}" "from" string     (* module *)
          | "import" ident "from" string ;                          (* .frec asset *)
device    = "device" ident [ ":" "Instrument" ] "{" { devItem } "}" ;
devItem   = "param" ident "=" num [ "in" num ".." num ]
          | "node" ident "=" nodeExpr
          | "out" nodeExpr ;
nodeExpr  = ident "(" [ ident ":" nodeArg { "," ident ":" nodeArg } ] ")"
          | "note" "." ident                                        (* freq | gate | vel *)
          | ident ;                                                 (* node name / param name *)
nodeArg   = string | num | nodeExpr ;
song      = "song" string body ;                                    (* legacy alias: a named root block *)
body      = "{" { bodyItem } "}" ;
bodyItem  = "desc" string | "tags" string | "license" string
          | "tempo" num | "swing" num | "meter" num "/" num | "key" ident ident
          | "let" ident "=" musicLit
          | "section" ident "=" "bars" "(" num ".." num ")"
          | track | return | block | place ;
place     = "play" ident [ "(" placeArg { "," placeArg } ")" ] atRef ;
placeArg  = "key" ":" string | "from" ":" num | "to" ":" num ;
track     = "track" ident "{" { trackItem } "}" ;
trackItem = "instrument" call | "insert" call
          | "play" patternExpr atRef
          | "audio" ident atRef
          | "send" ident num
          | "automate" ident "from" num "to" num "over" overRef
          | "modulate" ident "with" call
          | "volume" num | "pan" num ;
overRef   = "bars" "(" num ".." num ")" | ident ;                   (* section name *)
return    = "return" ident "{" { "insert" call | "volume" num | "pan" num } "}" ;
call      = ident [ "(" [ ident ":" ( num | string ) { "," … } ] ")" ] ;
patternExpr = musicLit | ident
            | ident "(" patternExpr { "," ident ":" ( num | string ) } ")" ;
atRef     = "at" ( "bars" "(" num ".." num ")" | ident ) ;
musicLit  = ( "beat" | "notes" | "prog" ) "`" raw "`" ;
num       = [ "-" ] NUMBER [ UNIT ] ;
```

## 4. Semantics

### 4.1 song Header

| Element | Meaning | Constraints |
| --- | --- | --- |
| `tempo 96bpm` | Tempo | **Required**. 20..400 (E-TIME-003) |
| `swing 0.62` | Delays even-position 16th notes (MPC notation: 0.5 = straight, 0.66 ≈ shuffle, range 0.5..0.8). Applies only to notes on the grid |
| `meter 4/4` | Time signature | Denominator 2/4/8/16 (E-TIME-004). Engine beats = numerator × 4 / denominator |
| `key D minor` | Key | Root C..B (+#/b), scales major/minor/dorian/phrygian/lydian/mixolydian/locrian/harmonicminor/chromatic |

### 4.2 Placement

- Bars are **1-based and inclusive at both ends**: `bars(1..8)` = bars 1 through 8.
- `section verse = bars(1..8)` names a range, referenced with `at verse` (E-MOD-003).
- Clip content loops within the placement range (when the pattern length < the range length).

### 4.3 Music Literals

| Literal | Content | Generation |
| --- | --- | --- |
| `` beat`x--- X.x-` `` | `x` = hit (vel 100), `X` = accent (120), `.` = ghost (55), `-` = rest. Whitespace is visual grouping | The step count divides one bar equally. Length = 60% of a step. Velocity is reflected as gain on all sound sources (100 = unity) |
| `` notes`C4:1/2 [E4 G4]:1 _:1` `` | `pitch:length` (in beats). `[…]` = chord, `_` = rest; lengths are `1` `0.5` `1/2` | Placed sequentially. C4 = MIDI 60 |
| `` notes`C2!:1/4 C2~:1/4 D2:1/2` `` | `!` = accent (vel 120), `~` = tie: holds the gate until the next note. Becomes a slide on mono/glide instruments (303 notation). To use both, write `C2!~` | Ties overlap at 102% of the length |
| `` prog`Em \| C G \| D` `` | `\|` = bar. Multiple chords within one bar divide the time equally | ChordEvent sequence. Playing it bare produces block chords |

Chord qualities: (unmarked = major), `m`, `min`, `7`, `maj7`, `m7`, `min7`, `dim`,
`aug`, `sus2`, `sus4`.

### 4.4 Pattern Functions (progression → performance)

| Function | Arguments | Voicing |
| --- | --- | --- |
| `chords(p)` | — | Holds all chord tones for the chord duration (root oct3, vel 90) |
| `bass(p, rate: 0.5)` | If rate omitted, one note per chord | Root note oct2, vel 100 |
| `arp(p, rate: 0.5, style: "up\|down\|updown")` | rate is 0<r≤1 bar | Cycles through chord tones at oct4, vel 95 |

### 4.5 Device DSL (defining instruments and effects in code)

`device Name : Instrument` (sound source) or `device Name : Effect` (effect,
used with `insert`). `param` is bound at instantiation time (range via `in lo..hi`,
default 0..1). An Instrument's graph is expanded into a per-voice interpreter;
polyphony (8 voices, oldest-steal) and envelope release are handled by the engine.
An Effect's graph is evaluated as the same graph with independent state for each stereo channel.

- An Instrument's signal sources are `note.freq / note.gate / note.vel`.
- An Effect's signal source is **`audio.in`** (the input signal). note.* cannot be used
  (E-GRID-003), and `adsr` requires an explicit gate (E-GRID-001).
- Writing an Effect as an instrument, or an Instrument as an insert, is E-DEV-009.
- **Reserved param name `glide`**: declaring it makes the device mono/legato, with the value as portamento seconds.
  Overlapping (tied) notes do not retrigger; the frequency slides instead (the 303 slide).

| Primitive | Signal inputs (defaults) | Parameters (defaults) |
| --- | --- | --- |
| `osc` | `freq` (note.freq), `mod` (±4oct), `pwm` (pulse width ±0.45) | `shape`: sine/saw/square/tri/pulse; `pw` (base width for pulse, default 0.5) |
| `noise` | — | — (deterministic: per-voice xorshift, reseeded per note) |
| `lfo` | — | `rate` 0..1 (= 0.05..12Hz), `shape`: sine/tri/saw/square |
| `adsr` | `gate` (note.gate) | `a` .05, `d` .3, `s` .6, `r` .25 (normalized) |
| `svf` | `in` (required), `mod` (±4oct) | `cutoff` .65 (= 30..18kHz exponential), `reso` .2 |
| `shaper` | `in` (required), `mod` (added to drive) | `drive` .3, `mode`: tanh/clip/fold |
| `gain` | `in` (required), `mod` (0..2×) | `level` .8 |
| `mix` | `a`, `b` (required) | — |
| `sample` | — (Instrument only: E-GRID-003 in an Effect) | `take` (required: a declared take slot), `start` 0, `end` 1, `loop`: off/on, `reverse`: off/on |

Signal sources: `note.freq` (Hz) / `note.gate` / `note.vel`, declared `node` names
(forward references disallowed, E-GRID-002), and nested calls. A `param` name may be written in a numeric position.

**take slots (soundnote)**: `take voice` at the top of a device declares that "the user
plugs in a recording". `sample(take: voice)` plays that take as the graph's sound source
(reference pitch C4, repitched to the played note, restarting from the beginning on each note-on),
and it can be processed downstream by svf/shaper/gain. Binding happens at the call site:
`instrument MyVox(voice: myTake)` (unbound is E-DEV-002; a non-Ident is E-TYPE-004).
Since the device itself carries no take, verifying a library alone
(`forte check lib.forte`) passes with the slot unbound —
anyone can plug their own recording into a published instrument.

### 4.6 Built-in Devices

| instrument | Parameters |
| --- | --- |
| `sampler(sample: "Kick"\|"Snare"\|"Hat")` | gain, attack, decay, sustain, release, pitch, start, end, loop("off"/"on"), reverse("off"/"on") |
| `sampler(take: <imported recording>, root: A3)` | Same as above. A recorded take becomes an instrument: `root` is the note name (C2..C6) at which the take was performed; playing that note gives the original sound, others are repitched chromatically |
| `sampler(…, start: 0.25, end: 0.6, loop: "on", reverse: "on")` | Sound design: `start`/`end` set the playback range (as a 0..1 fraction), `loop: "on"` loops the range while the note is held (short ranges become sustained tones), `reverse: "on"` plays in reverse. All are fixed at note-on time, preserving determinism |
| `kit(C2: kickTake, D2: snareTake, …)` | gain, attack, decay, sustain, release. Note-name keys assign recorded takes to pads (only an exactly matching pitch sounds; original-speed playback, no repitching). A `beat` literal strikes the lowest-pitched pad |
| `prisma` | wave(sine/saw/square/tri), cutoff, reso, attack, decay, sustain, release, detune, sub, filtenv |
| `mesh()` | Modular sound source with a default patch |

Beyond the built-ins, a standard instrument library `packages/essentials_0.6.0/instruments/` (drums / percussion / bass /
keys / leads / pads / synths / fx, 103 instruments in total) is bundled. These are not a language feature but
user-space code written in the device DSL of §4.5, used via ordinary `import`.

| effect | Parameters |
| --- | --- |
| `filter` | type(lp/hp/bp/notch), cutoff, reso |
| `eq` | low, mid, high |
| `drive` | drive (alias amount) |
| `delay` | time, fdbk (alias feedback), mix |
| `reverb` | size, decay, mix |
| `comp` | thresh, ratio, attack, release, makeup — stereo-linked compressor |
| `chorus` | rate, depth, mix — modulated delay with L/R quadrature phase |
| `pump` | amount, beats — tempo-synced ducking (a deterministic version of sidechain pumping. beats is the number of beats per cycle, default 1)|
| `width` | amount — M/S stereo width (0.5 is unity. Since insert is pre-pan, use on stereo sources)|

All numeric knobs are normalized 0..1 (out of range is E-TYPE-002). volume 0..1, pan -1..1,
send level 0..1.

### 4.7 Recording Assets (.frec)

- `import take from "./take1.frec"` → `audio take at bars(2..3)`.
- **Audio without provenance cannot even be referenced** (E-PROV-001): the header's provenance requires
  `device_class` (microphone / midi-render), `recorded_at`, `by`, `session`,
  and `sig`. The loopback calibration value is bundled as `latency_samples`.
- Layout: `FREC1\n` + u32-le header length + JSON header + f32-le PCM.
  Rate 8k..192k, 1..2ch (stereo plays back as a mono mix).
- Importing external audio (WAV/MP3, etc.) **does not exist at the grammar level**.

### 4.8 Module Resolution

- Paths are relative to the importing file. Resolution is recursive; cycles are E-MOD-007.
- If a name is absent, the library's actual exports are listed (E-MOD-006).
- Environments: CLI/LSP = filesystem, browser = the editor's file map (OPFS + bundled).

### 4.9 Automation and Modulation

Resolution of the target parameter is shared by automate / modulate (case-insensitive):

- `volume` (automate only) / an instrument's parameter name — built-ins
  (prisma / sampler) use the parameter tables; **for user-defined devices, the declared `param`
  is the name as-is**.
- `<insertName>.<parameter>` — refers to an insert effect by the name it was written with:
  `delay.mix`, `Muffle.cutoff` (a user-defined Effect's `param`s are also exposed).
  If multiple inserts share a name, the first one is targeted.

Unknown names produce E-AUTO-001 / E-LFO-001 with a list of "what is available".

- `automate <param> from 0.2 to 0.8 over bars(1..8)` — a linear ramp from the start to the end
  of the range (`over` also accepts a section name). Values are 0..1 (E-TYPE-002).
  For parameters with a lane, the base value is replaced by the lane: before the ramp begins
  it holds `from`, and after it ends it holds `to`. Multiple `automate`s are merged, per target,
  into a single lane in beat order.
- `modulate <param> with <modulator>(…)` — plugs a modulator into a parameter.
  There are 4 kinds (anything else is E-PARSE-021):
  - `lfo(rate: 0.4, amount: 0.5, shape: "tri")` — periodic wave. `rate` 0..1
    (0.05..8.05 Hz, default 0.3), `shape` sine / tri / saw / square
    (default sine).
  - `steps(seq: "0.1 0.6 0.3 0.9", every: "1/16", amount: 0.5)` —
    step sequencer. `seq` is whitespace-separated 0..1 values (E-TYPE-002).
    Writing `every` (1/2, 1/4, 1/8, 1/16. E-TYPE-005) makes it **tempo-synced**:
    1 step = that note value. If omitted, it cycles once per the free-running period of `rate`.
  - `random(rate: 0.4, amount: 0.4, smooth: 0.5)` — sample & hold
    randomness (deterministic: the same source yields the same random sequence). `smooth` 0..1 smoothly
    interpolates between steps. Tempo sync via `every` is also possible.
  - `adsr(a: 0.02, d: 0.4, s: 0.3, r: 0.1)` — a **note-gate-driven** external
    envelope: rises when the track starts sounding and
    releases when it goes silent (a retrofitted filter envelope). Each value 0..1
    (times follow a squared curve up to 3 seconds max). Evaluated at block rate.
  Common: `amount` -1..1 is **required** (E-LFO-003). The wobble rides on the base value
  (the lane value at that moment, if an automate lane exists) with a width of amount,
  saturating to 0..1. `automate` and `modulate` **can be layered** on a single parameter
  (modulation rides on top of the ramp), and multiple `modulate`s can be
  stacked as well.

### 4.9.5 Metadata: desc and tags

Every body (song or block) may carry one-line metadata at the top:

```forte
block AcidLine {
  desc "A 4-bar 303 acid line in A minor; the filter opens while it plays."
  tags "acid, bass, 303, house"
  …
}
```

- `desc` is the piece's own words — `forte play` prints the ROOT block's
  desc above the timeline; catalogs, packages and the browser use it when
  browsing and importing.
- `tags` is a comma-separated keyword list for search.
- `license "CC-BY-NC-SA-4.0"` declares the content license the body is
  published under (packages declare it; catalogs and players display it).
  The repository's package content defaults to CC BY-NC-SA 4.0 — see
  `packages/LICENSE`: forking and remixing the source is free and
  non-commercial; commercially exploiting rendered audio requires the
  rights holder's permission.
- Inheritance: a child's `desc`/`tags` override the parent's when present.
- A root block with a `desc` and no tracks/placements is a valid,
  deliberately silent file — the shape of `packages/<pkg>/package.forte`
  metadata blocks (an EMPTY root without a desc is still E-SONG-003).

### 4.10 Blocks (the universal composition unit)

A `block Name { … }` is a self-contained piece of music with the same body
as a song: tracks, lets, sections, returns, automation — and placements of
other blocks. Composition in Forte IS nesting blocks: refine a part inside a
block, then a higher block decides WHEN it plays, in WHICH key, and connects
it to other blocks. The outermost block you build is "the song".

- **Placement**: `play Groove(key: "E minor", from: 2, to: 5) at bars(9..16)`
  (also `at <section>`). Content loops when the placement span is longer
  than the block (window length rounded up to whole bars).
- **The block above always wins**: the root's `tempo` / `swing` / `meter` /
  `key` govern the entire render. A placed block's own `tempo` is ignored;
  its own `key` is the *reference* its transposition is computed from.
- **Transposition**: the effective key at a placement is the placement's
  `key:` override, or the effective key inherited from above. Melodic
  content (`notes` / `prog` / pattern functions) transposes by the minimal
  signed interval from the block's native key root to the effective root;
  **`beat` literals never transpose** (pads and drums stay put).
- **Windows**: `from`/`to` select bars inside the block (1-based, inclusive).
  A clip cut at the head keeps its loop phase (content is rotated).
- **Flattening**: placements compile away — each placed block's tracks merge
  into the parent as `Block.Track` named tracks; the same block placed twice
  shares tracks (one more set of clips), so mixer/inserts exist once. Sends
  resolve within the block. Nested definitions (`block` inside a body) are
  local and shadow imported names. Cycles are E-BLOCK-002; an unknown block
  lists what exists (E-BLOCK-001); flattening past the engine's 64-track
  limit is E-BLOCK-003; an empty from/to window is E-BLOCK-004.
- **Import**: `import { Groove } from "./blocks/groove.forte"` — importing a
  block also carries the devices of its home module (first definition of a
  name wins).
- **Inheritance**: `block Child : Parent { … }` — the child starts from the
  parent's (recursively resolved) body and overrides, class-style:
  - same-name track: `instrument` replaces; a same-name `insert` has its
    params replaced, a new insert appends; non-empty `play`/`audio` lists
    replace the parent's; `volume`/`pan` override; `automate`/`modulate`
    stack on top; `send`s merge by destination.
  - new tracks and returns append; `let`/`section` override by name; the
    header (`tempo`/`swing`/`meter`/`key`) overrides field by field.
  - chains resolve recursively (`A : B : C`); an unknown parent is
    E-BLOCK-005, an inheritance cycle is E-BLOCK-006.

## 5. The Determinism Contract

1. Same source + same assets → **bit-identical builds** (verified on native x86_64 / wasm32-wasip1 /
   browser wasm. `scripts/determinism_test.sh` is the CI gate).
2. Conditions: fixed f32, transcendental functions from a single libm implementation (dawcore::dmath),
   rendering at 48kHz, 512-sample blocks, 8-beat tail.
3. Build proof: output digest = FNV-1a 64 over the f32 LE bit patterns of all samples
   (v1; the production version will move to SHA-256). Recorded in `build.manifest.json` and the Hub release
   ledger; `forte hub verify` / the browser's Verify performs reproduction matching.

## 6. Canonical Form (fmt)

`forte fmt` is the sole formatter: indentation of brace depth × 2 spaces, no trailing whitespace,
at most 1 blank line, exactly 1 trailing newline. Strings, music literals, and comments are unchanged.
**If the token sequences before and after formatting do not match, application is refused** (E-FMT-001) — a
meaning-changing format is structurally impossible.

## 7. Diagnostics Catalog

| Series | Meaning |
| --- | --- |
| E-LEX-001..005 | Lexical (unclosed string/literal/block comment, invalid character) |
| E-PARSE-001..021 | Syntax (expected vs. actual for each construct, shape of automate/modulate) |
| E-TYPE-001..005 | Values (units, 0..1 range, string/number mix-up, out of choices) |
| E-TIME-001..004 | Time (bar range, rate, tempo, meter) |
| E-SONG-001..004 | Song structure (tempo required, key, no track, no song) |
| E-MOD-001..007 | Name resolution (pattern/section/return/import/cycles) |
| E-DEV-001..009 | Devices (unknown, parameters, built-in samples, collisions, Instrument/Effect mix-up) |
| E-GRID-001..006 | Device DSL (required inputs, forward references, signal/number, unknown primitives) |
| E-PAT-001..003 | Pattern functions (prog required, arguments, nesting) |
| E-BEAT / E-NOTE / E-PROG | Content of each literal |
| E-PROV-001..003 | Recording provenance (required block, .frec only, not imported) |
| E-AUTO-001 | automate (unknown parameter name. Lists what is available) |
| E-LFO-001..003 | modulate (parameter name, no instrument, modulator arguments) |
| E-FMT-001 | The formatter's safety valve |

Messages use musicians' vocabulary, are in Japanese, and carry positions. "What is available" is always listed.

## 8. Differences from the v0 Draft / Unimplemented

- Implemented and finalized from v0: send/return syntax (DECISION-S1), the prog quality set,
  the device DSL in node-graph form (arbitrary-expression `process` is future work).
- Implemented (v1.1): `automate` (volume + all parameters of instrument / insert,
  §4.9), `modulate … with lfo / steps / random / adsr` (including user-defined devices'
  and user-defined Effects' `param`s, and `<insert>.<param>`, §4.9).
- Unimplemented (v2 candidates): user-defined generics, automate pan, macros
  (1 knob → many parameters), automation of modulators themselves
  (`wobble.amount`), song-level shared modulators, triplet beat literals
  (DECISION-S2), first-class expression of section repetition (DECISION-S3),
  full unit-type checking (Hz/dB/ms), explicit `route` routing,
  actual verification of ed25519 signatures. (The effect device DSL is implemented per §4.5)
