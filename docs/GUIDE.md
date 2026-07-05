# Forte User Guide

A hands-on guide to composing with code. Work through it top to bottom and you'll
experience the whole journey from scratch: write a song, tweak it while listening,
build your own instruments, mix in recordings, then publish it and get forked.

- Precise language reference: [webdaw/spec/forte-lang-v1.md](webdaw/spec/forte-lang-v1.md)
- Design philosophy: [webdaw/01-vision.md](webdaw/01-vision.md)

---

## 0. Setup

What you need: the **Rust toolchain** (rustup). On Linux, audio output requires
`libasound2-dev` (it still works without it — you just get a silent backend).

```bash
git clone <this repository>
cd <repository>
cargo install --path crates/fortelang    # installs the `forte` command
```

Check that it works:

```bash
forte check songs/first-light.forte
# OK: compiled song (6 tracks, tempo 96 bpm, 16 bars)
```

## 0.5 Make sound right away (REPL)

Before creating any files, you can start making noise:

```
$ forte repl
forte> beat`x--- x-x-`                  # loops from the moment you hit Enter
forte> let theme = prog`Am | F | C | G`
forte> arp(theme, rate: 0.25, style: "updown")
forte> :inst polymer(wave: "saw", cutoff: 0.4)   # swap the sound while it plays
forte> :fx delay(time: 0.3, mix: 0.25)
forte> device Bloop : Instrument {      # you can even build instruments in the REPL (multi-line OK)
  ...>   node o = osc(shape: "square")
  ...>   out gain(in: o, mod: adsr())
  ...> }
forte> :inst Bloop()
forte> :track Bass                      # <- layer another track (loop station style)
forte:Bass> :inst polymer(wave: "saw", sub: 0.8)
forte:Bass> bass(theme, rate: 0.5)      # layers on top of the drums already playing
forte:Bass> :vol 0.7
forte:Bass> :undo                       # step back one move
forte:Bass> :save jam.forte             # save as a multi-track song
forte:Bass> :quit
```

`:help` lists every command. A `:save`d song can be picked up right where you left off with `forte play jam.forte`.

## 0.7 Browsing and playing instruments (forte instruments / instrument)

**Browsing**: explore the catalog. Library names double as categories
(drums / percussion / bass / keys / leads / pads / synths / fx, plus the
hardware clones tr808 / tr909 / tb303 / juno60 / sh101 / prophet5 / cr78):

```bash
forte instruments             # all 148 instruments: names, parameters, usage
forte instruments 808         # filter (by name or by library name)
forte instruments acid        # brings up the tb303
```

Every line in the listing can be dropped straight into a song:

```forte
import { Bass303 } from "lib/std/tb303.forte"   // <- the import line the listing gives you
track Acid { instrument Bass303(cutoff: 0.4) … }
```

Curious what a parameter means or how an instrument is built? Open `lib/std/*.forte` —
it's all device DSL code, so you can read it, fork it, and rework it character by character.
For a demo with 10 instruments playing at once, run `forte play songs/std-tour.forte`;
genre-specific usage examples live in `songs/patterns/`.

**Playing**: audition any instrument that catches your ear with your keyboard:

```bash
forte instrument Bass303                  # resolved by name from lib/std
forte instrument "JunoPad(cutoff: 0.5)"   # parameters work too
forte instrument polymer                  # built-ins as well
```

Your keyboard becomes a piano:

```
a w s e d f t g y h u j k o l p ;   =  C C# D D# E F F# G G# A A# B C…
z / x  octave up/down      c / v  velocity up/down      q  quit
1..9   select a knob       - / =  turn it down / up
```

**Knobs**: the instrument's parameters (a device's declared `param`s, or a
built-in's parameter table) show up in the status line. Press a digit to grab
one — `1` for cutoff, `2` for reso on the 303 — then `-`/`=` to turn it while
you play. Steps are 5% of the parameter's declared range, applied live
through the same path automation uses, so what you hear is exactly what
`automate cutoff …` will do in the song.

When you quit, your performance is printed as a `notes` literal quantized to
16th notes — paste it straight into a song. This is your entry point to
"performance is code."

`forte play my-song.forte` shows a timeline in the console:
each track's lane (which bar it enters and exits), the moving playhead,
elapsed/total time, loop count, and which tracks are currently sounding.

## 0.8 Blocks — music as reusable parts

The universal composition unit is the **block**: a self-contained piece of
music (multi-track, a few bars) that other blocks place, transpose, window
and loop. A song is just the outermost block. Composition in Forte is:
refine a part inside a block, then let a higher block decide *when* it
plays and *in which key*, and connect it to other blocks.

```forte
// blocks/acid-line.forte — written once, in A minor
block AcidLine {
  key A minor
  track Acid {
    instrument Bass303(cutoff: 0.24, reso: 0.82)
    play notes`A1!:0.25 A1:0.25 A2:0.25 …` at bars(1..4)
    automate cutoff from 0.15 to 0.7 over bars(1..4)
  }
}
```

```forte
// the song only decides WHEN and in WHICH key
import { AcidLine } from "../blocks/acid-line.forte"
import { FourFloor } from "../blocks/four-floor.forte"

song "Block Party" {
  tempo 126bpm
  key A minor
  play FourFloor at bars(5..12)
  play AcidLine  at bars(5..12)                    // loops the 4-bar block
  play AcidLine(key: "D minor") at bars(13..20)    // the answer, a fourth up
  play AcidLine(from: 3, to: 4) at bars(21..24)    // just its second half
}
```

Rules of thumb:

- **The block above always wins** — the root's tempo/key/swing govern the
  render. A block's own `key` is the reference its transposition is
  computed from; its own `tempo` matters only when the block itself is the
  build root.
- **Melody transposes, drums don't** — `notes`/`prog` content follows the
  placement key; `beat` literals never move.
- **Blocks nest** — a block can `play` other blocks; the last top-level
  block in a file is what `forte build`/`play` renders, so a block library
  is always playable on its own.
- Reusable blocks live in `blocks/` — fork one, change its pattern, and
  every song placing it follows.

## 1. Your first song (5 minutes)

Create `my-song.forte`:

```forte
song "My First" {
  tempo 120bpm
  meter 4/4
  key C major

  track Drums {
    instrument sampler(sample: "Kick")
    play beat`x--- x--- x--- x-x-` at bars(1..4)
  }

  track Keys {
    instrument polymer(wave: "square", cutoff: 0.5)
    play notes`C4:1 E4:1 G4:1 [C4 E4 G4]:1` at bars(1..4)
  }
}
```

```bash
forte check my-song.forte    # errors come with line numbers and plain-language messages
forte play  my-song.forte    # starts loop playback
```

**While playback keeps running**, edit the file in your editor and save.
The sound changes without a single dropout (hot reload). This is the core
Forte loop: **listen, and fix it in code.**

To render to a file:

```bash
forte build my-song.forte
# produces my-song.wav and my-song.manifest.json (build proof)
```

The digest in `manifest.json` is proof that "this code produced this sound" —
it comes out identical no matter who builds it, on any machine (browsers included).

Add `--stems` and you also get per-track WAVs (soloed, with send reverb
included), each with its own digest recorded in the manifest —
the raw material for an open-stems release.

To take a whole song with you:

```bash
forte export my-song.forte
# my-song.zip — entry file + imports + recorded takes + build proof + VCS history
```

The zip bundles an `export.manifest.json` (with the render digest), and if the
song lives inside a clean repository, the `.forte/` history objects come along too.
Unpack it anywhere and it builds as-is, with `forte log` giving you the full past.
The zip itself is deterministic — the same source produces a byte-identical zip.
No lock-in.

## 2. Language cheat sheet

```forte
// Comments. /* block comments */ work too

song "Name" {
  tempo 96bpm            // required
  meter 6/8              // defaults to 4/4
  key D minor            // optional

  // ---- Patterns are values. Name them with let and reuse them ----
  let kick  = beat`x--- x-x-`             // x=hit X=accent -=rest. One bar, evenly divided
  let melo  = notes`D4:1/2 F4:1/2 [A3 D4]:1 _:1`  // pitch:beats. []=chord _=rest
  let theme = prog`Dm | Bb F | C`         // chord progression. | separates bars

  // ---- Name the structure of your song ----
  section verse = bars(1..8)
  section hook  = bars(9..16)

  // ---- Return track (send destination) ----
  return Space { insert reverb(size: 0.7, decay: 0.6, mix: 1.0) }

  track Bass {
    instrument polymer(wave: "saw", cutoff: 0.3, sub: 0.7)
    insert drive(drive: 0.2)              // inserts apply in the order listed
    volume 0.7
    pan -0.1
    play bass(theme, rate: 0.5) at verse  // generate a bassline from the progression
  }

  track Keys {
    instrument polymer(wave: "tri")
    send Space 0.35                        // post-fader send
    play chords(theme) at verse            // block chords
    play arp(theme, rate: 0.25, style: "updown") at hook  // arpeggio

    // ---- Move the sound over time ----
    automate volume from 0.2 to 0.8 over verse   // fade in (over bars(1..8) works too)
    automate cutoff from 0.2 to 0.9 over hook    // open the filter while playing
    modulate cutoff with lfo(rate: 0.4, amount: 0.5, shape: "tri")  // wobble
    modulate cutoff with steps(seq: "0.2 0.7 0.4 0.9", every: "1/16", amount: 0.5) // 16th-note step sequence
    modulate reso   with random(rate: 0.3, amount: 0.2, smooth: 0.6) // S&H randomness (deterministic)
    modulate cutoff with adsr(a: 0.02, d: 0.4, s: 0.3, amount: 0.5)  // external envelope that opens on each note
    automate delay.mix from 0.0 to 0.5 over hook   // insert parameters can be targeted as `name.param`
  }
}
```

- Bars are **1-based and inclusive on both ends**. Patterns shorter than their
  range loop to fill it.
- `automate` is a linear ramp from the start of a range to its end. The target
  can be volume or any instrument parameter — for your own devices, every
  declared `param` is addressable by name (this is how you get the 303
  cutoff sweep).
- `modulate` plugs a modulator into a parameter: `lfo` (periodic wave),
  `steps` (tempo-synced step sequence via `every: "1/16"`), `random`
  (sample & hold, deterministic), `adsr` (an external note-gate-driven
  envelope). amount ranges -1..1 and stacks on top of any `automate` ramp.
  Multiple modulators can stack too.
- Targets include instrument parameters and also **insert effect
  parameters**, addressed as `insertName.parameter` like `delay.mix`
  (a custom Effect's `param`s work the same way).
- All knob-style values are **normalized to 0..1** (volume and cutoff alike).
  Only pan is -1..1.
- Built-in instruments: `sampler(sample: "Kick"/"Snare"/"Hat")`, `polymer(…)`, `grid()`.
  Effects: `filter, eq, drive, delay, reverb`. Misspell a parameter name and
  Forte lists the valid ones — no memorization required.
- **Standard instrument library (lib/std)**: 29 instruments built in the device
  DSL ship with Forte. Import them like
  `import { Kick909, Clap } from "../lib/std/drums.forte"` (paths are relative
  to the song file). drums 10 / bass 5 / keys 5 / pads 4 / leads 5 — all code,
  so if you don't like one, fork it and rework it character by character.
  The full 10-track demo is `songs/std-tour.forte`.

Formatting: `forte fmt my-song.forte` (guaranteed to never change meaning).

## 3. Building your own instruments (device)

Synths are code too. Write them at the top of the file (before the song):

```forte
device MyLead : Instrument {
  param cutoff = 0.6 in 0.0..1.0          // a parameter the caller can tweak

  node o   = osc(shape: "saw")             // omit freq and it tracks the played note
  node env = adsr(a: 0.03, d: 0.25, s: 0.6, r: 0.3)
  node vib = lfo(rate: 0.3, shape: "sine")
  node f   = svf(in: o, cutoff: cutoff, reso: 0.3, mod: vib)
  out gain(in: f, mod: env, level: 0.9)
}

song "..." {
  track Lead {
    instrument MyLead(cutoff: 0.75)        // bound with range checking
    ...
  }
}
```

There are 8 primitives: `osc / noise / lfo / adsr / svf / shaper / gain / mix`.
Wire signals using `note.freq / note.gate / note.vel`, node names, and nested
calls. Polyphony (8 voices) is handled by the engine.

- `noise()` — white noise, the raw material for snares and hats. Deterministic
  (the same source builds to the same bits), so use it with confidence.
- `osc(mod: …)` — pitch modulation (±4 octaves). Feed it an envelope for an
  808-kick pitch drop, or an LFO for vibrato.
- `shaper(in: x, drive: 0.5, mode: "tanh"|"clip"|"fold")` — waveshaper.
  tanh is fat, clip is hard, fold folds harmonics back for a metallic edge.

For a **complete hand-built drum kit**, see `songs/handmade-kit.forte`
(Kick = sine+tanh, Snare = noise+SVF+body resonance, Hat = noise+clip.
No built-in samples — every character of the sound is code).

**Effects can be custom too** (`: Effect`). The input signal is `audio.in`:

```forte
device Fuzz : Effect {
  param amount = 0.6 in 0.0..1.0
  node crushed = shaper(in: audio.in, drive: amount, mode: "fold")
  node dry     = gain(in: audio.in, level: 0.3)
  out mix(a: crushed, b: dry)          // parallel wet + dry
}

track Keys {
  instrument polymer(wave: "tri")
  insert Fuzz(amount: 0.7)             // used via insert (not valid as an instrument)
}
```

Plug an LFO into a `gain`'s mod for tremolo, or into an `svf`'s mod for auto-wah.
In stereo, the left and right channels pass through the same graph with
independent state.

## 4. Splitting into libraries and importing

Move instruments into their own file and every song can use them (and later,
they become the unit that gets forked on the Hub):

```forte
// devices/mylib.forte — a file with no song = a device library
device MyLead : Instrument { ... }
device MyBass : Instrument { ... }
```

```forte
// my-song.forte
import { MyLead, MyBass } from "./devices/mylib.forte"
```

Validate a library on its own: `forte check devices/mylib.forte`.

## 5. The browser editor

The same language, the same sound (bit-identical), with nothing to install:

```bash
forte browser                        # serves and opens the browser (--port 9000 --no-open available)
forte web build                      # rebuild the wasm after changing the engine
```

| UI | What it does |
| --- | --- |
| Song selector / New / Del | Your songs plus the demos, stored in OPFS (in-browser storage). **Edits auto-save** and survive closing the tab |
| ▶ Play / ■ Stop | AudioWorklet playback. Edits apply without stopping playback |
| ● Rec | Mic recording → saved as a provenance-stamped `.frec` (see below) |
| ⏱ Calib | Loopback calibration: plays a chirp, catches it with the mic, and measures actual round-trip latency |
| 🎹 Perform | Performance mode: MIDI keyboard or PC keys (A–K = white keys, W/E/T/Y/U = black keys). On stop, your performance is transcribed into `notes` code |
| Build digest | Computes the build proof in the browser. Matches the CLI's value exactly |
| ⇪ Publish | Registers this song (with its imported libraries and recorded takes) on the hub. If it came from a fork, forked_from is recorded automatically. `?api=` points to the hub server (default 127.0.0.1:9377) |
| History panel | **The repository lives in the browser too**: Commit snapshots all local files, `diff` shows the difference between a commit and your current work in musical terms ("tempo: 96 → 132 bpm"), and Restore returns to that commit's state. Stored in OPFS in the same object format as the CLI (SHA-256) |
| Arrangement view at the bottom | Read-only visualization (code is the single source of truth for editing) |

Once opened, it works **fully offline** (PWA). Chromium-based browsers recommended.

## 6. Recording (vocals, live performance)

Forte has no "load an audio file." The only ways sound gets in are the
**microphone (and MIDI)**, and every recording is stamped with provenance
(when, who, which device). In the browser editor:

1. (Recommended) Run ⏱ Calib once — the measured latency is recorded into every take after that
2. ● Rec → play/sing → ■ stop recording
3. `assets/take-1.frec` is saved and the status bar shows the import line
4. Drop it into the song:

```forte
import take from "./assets/take-1.frec"
song "..." {
  track Voice {
    audio take at bars(5..8)      // no instrument needed. Add effects via insert
    insert reverb(mix: 0.2)
  }
}
```

When you stop recording, Forte asks **"insert this into the song?"** — accept and
the `import` line and a `track Voice_… { audio … }` block are appended
automatically. It's the text version of dragging a take onto the timeline.

A `.frec` without provenance (audio brought in from elsewhere) is a compile
error, E-PROV-001. That's by design, not a bug — it's a core Forte principle.

### Turning a recording into an instrument (take sampler)

Beyond placing a recording on the timeline, you can **turn the recording itself
into an instrument**:

```forte
import voice from "./take1.frec"

track Choir {
  instrument sampler(take: voice, root: A3)   // root: A3 if the take was sung at A3
  play notes`A3:1 C4:1 E4:1` at bars(1..4)    // harmonies get repitched chromatically
}
```

Set `root` to the pitch the take was performed at (C2..C6). Play that note and
you get the original; anything else, the sampler repitches. Your voice,
beatboxing, humming — anything the mic can capture is synth material.
ADSR parameters like attack/decay work too.

You can even **slice one take into different instruments**:

```forte
instrument sampler(take: voice, start: 0.25, end: 0.6)   // cut out just the sweet spot
instrument sampler(take: voice, end: 0.1, loop: "on")    // loop the first 10% -> instant pad
instrument sampler(take: voice, reverse: "on")           // reverse playback -> riser
```

`start`/`end` set the playback range (as 0..1 fractions), `loop: "on"` loops
that range while the note is held (a short range becomes a sustained tone), and
`reverse: "on"` plays backwards. Everything is fixed at note-on, so rendering
stays deterministic.

### Building a drum kit from recordings (kit)

Assign multiple takes to keys and your beatboxing becomes a kit:

```forte
import kickTake from "./kick.frec"
import snareTake from "./snare.frec"

track Drums {
  instrument kit(C2: kickTake, D2: snareTake, gain: 0.9)
  play notes`C2:1/2 D2:1/2 C2:1/2 D2:1/2` at bars(1..8)
}
```

Each pad plays **at original speed** (no repitching). A `beat` literal triggers
the lowest pad. gain / attack / decay / sustain / release all apply.

### Processing recordings inside a device (soundnote)

The deepest form of sound design: make a take the **sound source of a device's
node graph** and process it downstream through filters and shapers.

```forte
device VoxKeys : Instrument {
  take voice                                  // a slot the caller fills with a recording
  param cutoff = 0.55 in 0.0..1.0

  node s   = sample(take: voice, loop: "on", end: 0.3)
  node f   = svf(in: s, cutoff: cutoff, reso: 0.25)
  node env = adsr(a: 0.005, d: 0.3, s: 0.6, r: 0.2)
  out gain(in: f, mod: env, level: 0.9)
}

track Keys {
  instrument VoxKeys(voice: myTake, cutoff: 0.6)   // the recording is bound here
  play notes`C4:1 E4:1 G4:2` at bars(1..8)
}
```

`take voice` declares "the caller supplies a recording." The device itself
holds no take, so **you can publish it to the Hub and anyone can fork it as an
instrument**, each person plugging in their own recording. `sample()` is
repitched to follow the played note (the take's reference pitch is C4), and
start/end/loop/reverse work just like the sampler.

## 7. Writing in Forte Studio (VSCode)

```bash
cd editor/vscode-forte
npm install && npm run compile
# Open this folder in VSCode and press F5 (extension development host)
```

Set `forte.path` to the absolute path of `forte` (`~/.cargo/bin/forte`) and you get:
- Errors underlined in red as you type (plus completion, hover, and format-on-save)
- **Forte: Play (hot reload)** / **Build** / **Stop** from the command palette
- **Forte: REPL** opens a jam terminal; in a `.forte` file, press
  **Shift+Enter** — the selection (or current line) is sent to the REPL and
  plays immediately
- **Forte: Show Arrangement** — a read-only arrangement view opens alongside
  and **refreshes on every save**

The ♪ icon in the activity bar is the **Forte Studio** sidebar:

- **History** — the song's commit list. ✓ to commit (running `forte init` on
  the spot if there's no repository yet), plus per-commit **diff** (the
  difference against the working tree opens alongside, in musical terms) and
  **Restore** (checkout). Merging is in the command palette:
  **Forte: Merge Branch…**
- **Hub** — the hub's songs/libraries (with fork lineage ⑂).
  **▶ Listen** (plays straight from the store's source — audition without
  forking), **Fork…** (pick a folder, fork with full history, open it right
  away), and right-click for **View lineage** / **Verify release**.
  The **Publish** button in the toolbar registers the current file on the hub
  (set the hub location via the `forte.hub` setting; default is
  FORTE_HUB / ./.forte-hub)

## 7.5 Version control — give your song a history

Turn your song's folder into a repository and you can experiment without fear
of wrecking a sketch.

```bash
cd my-song/
forte init                        # creates .forte/ (the equivalent of git's .git)
forte commit -m "first sketch"    # records all *.forte and *.frec (recordings)
forte status                      # what you changed
forte log                         # history
```

**Diffs speak music** — that's Forte's selling point. Instead of line numbers,
it compares compiled models:

```
$ forte diff
~ song.forte
    tempo: 108 → 116 bpm
    track Keys: Polymer wave: square → saw
    track Hats: bars 13..16: placement removed
~ handmade.forte (changes the sound via import)
    track Lead: Poly Grid patch (node graph) changed
```

- Changes that are only comments or formatting report "models are identical."
- If you edit only an instrument library (an import target), the diff also
  shows up on **the side of every song that listens to it**.
- Try alternate ideas on branches: `forte branch idea && forte checkout idea`.
  Return with `forte checkout main`. Reach past versions via hashes from
  `forte log`: `forte checkout 3cc5a7e9` — play them on the spot and compare
  by ear (checkout is refused for safety while you have uncommitted changes).
- Compare branches with `forte diff main idea`.
- Merge with `forte merge idea`. Edits in different places combine
  automatically (file-level, then line-level three-way merge). If both sides
  changed the same line, the file keeps `<<<<<<<` markers; fix it and
  `forte commit` to create a resolution commit (recording both parents).
- **Merge results are compile-verified.** Even when lines merge cleanly, the
  result can be musically broken (e.g., one side renamed a section while the
  other still references the old name) — Forte warns "⚠ does not compile."
  That's a safety net no text VCS can offer.

## 8. Hub — publish, fork, release

```bash
export FORTE_HUB=~/.forte-hub        # where it lives (defaults to ./.forte-hub)

forte hub publish my-song.forte      # snapshots the song with its imported libraries.
#   If the song is inside a VCS repository (forte init'd and clean), the history is pushed too
forte hub list
forte hub fork mylib ./work/mylib    # * the ONLY way to get things. There is no download command
#   -> if history was published, the whole .forte repository comes down with it:
#      a "fork mylib v1" commit lands on top of the original author's commits,
#      and your commits from then on continue that line (lineage lives in the history itself)
#   -> forte diff <original author's commit> HEAD reads as "what I changed from the original"
#   -> modify it and publish --as newname, and "forked from mylib v1 @ commit" is recorded automatically

forte hub release my-song            # deterministic build -> digest recorded in the ledger
forte hub verify  my-song            # anyone can re-verify (tampering shows as MISMATCH)
forte hub lineage my-song            # lineage: fork sources/descendants, releases, verification count
forte hub similar my-song            # songs with the same chord progression (found even in a different key)
```

Song pages have per-track **M / S buttons** so you can pull parts in and out
while listening (mute the vocals for karaoke, solo the bass to learn it by
ear — a way of listening that digs into lineage).

**A performance fork makes a full loop in the browser**: listen on the hub
page → Fork (into the editor with a lineage stamp) → ● Rec your vocals →
insert → ⇪ Publish. The published fork carries `forked_from` and the recorded
takes, and anyone can rebuild it reproducibly.

Song pages list the **instruments used** (with links to their defining
libraries), and library pages list **the songs that use each instrument** —
lineage is traceable from instruments and songs alike. In listings, click an
author's name to filter down to their work.

The hub's front page shows the **fork family tree** — whose song spawned whose
remix, at a glance, with release / play-count badges, each node clickable
through to its song page.

Digging through lineage in the browser:

```bash
forte hub serve                      # API: http://127.0.0.1:9377
# -> open http://localhost:8000/web/hub.html
```

On a song page you can **▶ Listen** (played from source on the spot),
**Verify in browser** (reproduce and check a release's digest in your own tab),
and **Fork → into the editor** (the fork is recorded in the ledger and the
files land in the editor).

### Using it together, option 1: GitHub as your hub (recommended)

No server needed. **A hub is just a git repository**, so create one empty
repository on GitHub and it becomes a hub for your whole group:

```bash
# 1. Create an empty repository on github.com (e.g., you/forte-hub)
# 2. Then just pass it to --hub
forte hub publish my-song.forte --hub github:you/forte-hub   # pushes with history
forte hub fork handmade ./my-take --hub github:you/forte-hub # forks with history
forte hub list --hub github:you/forte-hub
forte hub serve --hub github:you/forte-hub  # dig through lineage in the browser
```

`github:you/forte-hub` is shorthand for `https://github.com/you/forte-hub.git`.
Prefer SSH? Pass `git@github.com:you/forte-hub.git` as-is (GitLab, a bare repo
on a NAS — anywhere git can talk becomes a hub).

- **Authentication**: your everyday git credentials (SSH keys / gh auth) are used as-is
- **Author name**: `git config user.name` (≈ your GitHub name)
- **Concurrent publishes**: pushes are compare-and-swap; if someone beats you
  to it, Forte syncs and replays automatically. Two people publishing at once
  both get in
- **The ledger is versioned too**: `git log` shows every publish / fork in order

release / verify / lineage all take `--hub github:…` the same way.

### Using it together, option 2: your own server (with authentication)

For a serious public hub where "the only way to get things is fork" must be
structurally enforced, there's also an authenticated HTTP server.
Point `--hub` at the URL of a hub running `forte hub serve`:

```bash
# Participants: register a name and receive a token (shown only once)
forte hub signup shusuke --hub http://host:9377
export FORTE_HUB_TOKEN=<the token you received>

forte hub publish my-song.forte --hub http://host:9377   # pushes with history
forte hub fork handmade ./my-take --hub http://host:9377 # forks with history
forte hub list --hub http://host:9377
```

Once anyone signs up on a hub, publishing **requires a token**, and the author
name is derived from the token (the author field in the body is ignored — no
impersonation). Pushed history objects have their content hashes verified
server-side before being stored, so the store stays content-addressed no
matter who pushes. Tokens are stored only as SHA-256 hashes on the server.
v1 speaks plain HTTP, so put a TLS reverse proxy in front before exposing it
to the internet (see the README).

## 9. Troubleshooting

| Symptom | Fix |
| --- | --- |
| No sound from `forte play` | If the startup log says `audio: no output device — silent backend`, it's an audio device problem. On Linux, `apt install libasound2-dev` and rebuild |
| How to read errors | `line:col [E-XXX-nnn] message`. Messages enumerate the valid options. The error-code scheme is in spec v1 §7 |
| No sound in the browser | The first ▶ Play needs a user gesture (browser autoplay restrictions). Safari is restrictive; Chromium recommended |
| wasm build fails | `rustup target add wasm32-unknown-unknown` |
| Want to verify determinism yourself | `rustup target add wasm32-wasip1`, then `scripts/determinism_test.sh` (requires Node 20+) |
| Full test suite | `cargo test -p dawcore -p fortelang` / for browser E2E, `npm i playwright` then `node scripts/web_e2e.mjs` |

## 10. FAQ

**Q. I want to load my own WAVs or sample packs.**
A. You can't (by design). Forte structurally excludes "audio of unknown
origin" to build a world where every sound's provenance can be traced. For
drums, use the `sampler` built-ins or synthesize with a `device`; for vocals
and live performance, record through the mic.

**Q. I want to edit notes in a GUI.**
A. No (by design). Code is the single source of truth for editing. Instead,
visualization (the arrangement view) is provided read-only. Diffs are
readable, merges work, and forks are possible precisely because it's text.

**Q. Same code, but doesn't the sound differ across environments?**
A. It doesn't. That's the deterministic build, and it's the foundation of
release verification (`hub verify`) and proof of contribution. Check it via
the digest in `build.manifest.json`.
