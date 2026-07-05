<p align="center">
  <img src="assets/logo.svg" width="112" alt="Forte">
</p>

<h1 align="center">Forte Core</h1>
<p align="center"><b>The engine, the language, the tools.</b></p>
<p align="center">
  Songs, instruments, effects, performances — all readable, hackable, forkable source code.
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
</p>

---

Forte brings music production into the world of open development: **code, fork,
build, release**. A song, its patterns, its chord progressions — and the
instruments themselves — are source code (`.forte`). Builds are deterministic:
the same commit renders **bit-identical audio** on native, wasm, and in the
browser, so anyone can re-verify a release from a browser tab.

```forte
import { WarmLead, SubBass } from "./devices/warm.forte"

song "Handmade" {
  tempo 100bpm
  key G minor
  let line = prog`Gm | Eb | Bb | F`

  track Lead {
    instrument WarmLead(cutoff: 0.7, vib: 0.35)
    insert delay(time: 0.3, fdbk: 0.3, mix: 0.25)
    play arp(line, rate: 0.5, style: "updown") at bars(5..12)
  }
}
```

| Browser editor (musical-vocabulary diff in the History panel) |
| --- |
| ![editor](../docs/images/ui-editor.png) |

## Getting started

```bash
# Linux needs ALSA headers for audio output: sudo apt install libasound2-dev
cargo install --path crates/fortelang   # from forte/   # installs the `forte` command

# tab-completion (recommended — instrument names complete dynamically):
echo 'source <(forte complete bash)' >> ~/.bashrc   # bash
echo 'source <(forte complete zsh)'  >> ~/.zshrc    # zsh

forte repl                              # type a line, hear it immediately
forte check ../packages/essentials_0.6.0/songs/first-light.forte     # validate (errors in musical terms + line numbers)
forte play  ../packages/essentials_0.6.0/songs/first-light.forte     # live playback + console timeline; saves hot-reload
forte instruments play Bass303          # your keyboard becomes a piano (a w s e d ...)
forte browser                           # launch the browser editor
forte build ../packages/essentials_0.6.0/songs/first-light.forte     # WAV + build proof (digest included)
forte build ../packages/essentials_0.6.0/songs/handmade-kit.forte --stems  # per-track WAVs + per-stem digests
forte export ../packages/essentials_0.6.0/songs/first-light.forte    # self-contained zip (song + takes + proof + history)
forte upgrade                           # update the forte command itself
```

The REPL is a loop station:

```
forte> beat`x--- x-x-`                     ← loops instantly
♪ playing (120 bpm, loop 32 beats)
forte> let theme = prog`Am | F | C | G`
forte> arp(theme, rate: 0.25, style: "updown")
♪ playing
forte> :inst prisma(wave: "saw")          ← swap the instrument while it plays
forte> :fx reverb(mix: 0.3)
forte> :save jam.forte                     ← the jam becomes a song file
```

| REPL command | Effect |
| --- | --- |
| (type a pattern) | Loops instantly on the **current track**: `beat` / `notes` / `prog` / `chords()` / `arp()` / `bass()` |
| `:track Bass` / `:tracks` / `:drop Bass` | Layer tracks; subsequent patterns, `:inst`, `:fx` target that track |
| `:vol 0.7` / `:pan -0.3` / `:undo` | Volume/pan of the current track, undo one step |
| `let name = …` / `device … { … }` / `import …` | Add to the session (multi-line OK; errors roll back) |
| `:tempo 140` / `:inst prisma(…)` / `:fx reverb(…)` / `:fx clear` | Change everything without stopping |
| `:show` / `:save jam.forte` / `:stop` / `:quit` / `:help` | Show source / save as a song / stop / quit |

### Blocks — compose with reusable parts

The universal unit of composition is the **block**: a self-contained,
multi-track piece of music. Songs are just the outermost block — they decide
*when* each block plays and *in which key*; melody transposes, drums stay
put, content loops to fill the placement, and blocks nest arbitrarily deep:

```forte
import { AcidLine } from "../../packages/essentials_0.6.0/blocks/acid-line.forte"
import { FourFloor } from "../../packages/essentials_0.6.0/blocks/four-floor.forte"

song "Block Party" {
  tempo 126bpm
  key A minor
  play FourFloor at bars(5..12)
  play AcidLine  at bars(5..12)                  // a 4-bar block, looped
  play AcidLine(key: "D minor") at bars(13..20)  // same line, a fourth up
  play AcidLine(from: 3, to: 4) at bars(21..24)  // just its second half
}
```

Reusable blocks live in `blocks/`; a block library is directly playable
(`forte play ../packages/essentials_0.6.0/blocks/acid-line.forte` renders its last block), so parts are
auditioned exactly like songs.

### Find, audition, and use instruments

Three commands take you from "what's in the box" to notes in a song:

```bash
forte instruments list       # the catalog: 148 devices, params, import lines
forte instruments list 808   # filter by name or library (tr808, juno, bass, ...)
forte instruments play BD808 # audition it: the keyboard becomes a piano
source <(forte complete bash)  # tab-completion: play s<Tab> lists instruments
```

`forte instrument <Name>` resolves any standard instrument by name, takes
parameters (`forte instrument "Bass303(cutoff: 0.5)"`), and maps the keys —
`a w s e d f t g y h u j k ...` is a chromatic run from C, `z`/`x` shift the
octave (z down / x up), `c`/`v` the velocity (c down / v up). The instrument's knobs are live too: press
`1`-`9` to grab a parameter (cutoff, reso, ...) and `-`/`=` to turn it while
you play. When you quit, the jam is printed as a quantized `notes` literal —
the performance is source code you can paste straight into a song. Then wire
it in:

```forte
import { BD808, SD808, CH808 } from "../packages/essentials_0.6.0/instruments/tr808.forte"
track Drums { instrument BD808(decay: 0.7) play beat`x--- x---` at bars(1..8) }
```

`forte play` shows the song as a console timeline — every track's lane, where
it enters and leaves, plus a live playhead with progress, elapsed/total time,
loop count and which tracks are sounding.

### Browser editor

Diagnostics as you type, AudioWorklet playback, OPFS autosave, fully offline PWA:

```bash
forte browser                 # serves web/ and opens the editor
forte browser --port 9000 --no-open
forte web build               # rebuild the wasm after engine changes
```

### Version control for music

Songs get repositories, and diffs speak **music, not line numbers**:

```bash
cd my-song/ && forte init          # create the .forte/ repository
forte commit -m "first sketch"     # snapshot *.forte / *.frec
forte log                          # history
forte branch idea && forte checkout idea   # try another idea
forte diff                         # e.g.  tempo: 108 → 116 bpm
                                   #       track Keys: Polymer wave: square → saw
                                   #       track Hats: bars 13..16: pattern removed
forte checkout main                # switch back for an A/B listen anytime
forte merge idea                   # non-conflicting edits merge automatically;
                                   # the result is compile-checked, and you get a
                                   # warning if the music broke
```

Edit only an instrument library and every song importing it reports
"the sound changes via this import" — possible because everything is code.

### Instruments

The standard library `../packages/essentials_0.6.0/instruments/` ships 148 instruments written in the device DSL —
including faithful classic-hardware recreations: the full 808, 909 and CR-78 drum
machines, the 303 bass (with real accent and slide), Juno-style DCO polys
(PWM + chorus), Prophet-style two-oscillator polys, and SH-101-style monos
with glide. They are plain code: fork one and rewrite it character by
character (demo: `forte play ../packages/essentials_0.6.0/songs/std-tour.forte`). For full arrangements to
learn from, `../packages/essentials_0.6.0/blocks/` holds genre grooves (house, DnB, bossa nova,
afrobeat, trap, …) and `../packages/essentials_0.6.0/songs/` holds complete songs with sections —
every one of them compiles and renders under the merge gate (forte ci).

Recorded takes become instruments too: slice, stretch, and reverse one recording
into many instruments with `sampler(take: voice, start: 0.25, end: 0.6,
loop: "on", reverse: "on")`, turn beatboxing into a drum kit with
`kit(C2: kickTake, D2: snareTake)`, or write devices that use recordings as raw
material via the `take` slot + `sample()` node. Devices don't own takes, so an
instrument can be published and anyone can plug their own recording into it.

### The listening site (GitHub Pages)

`scripts/publish_web.sh` assembles the whole listening experience —
editor, zero-install player, package catalog (static `packages.json`),
and the essentials package with its albums — and pushes it to the
`gh-pages` branch. Enable it once under Settings → Pages (deploy from
branch `gh-pages`) and "click a link → the album plays" is live.
`DRY_RUN=1` builds the site without pushing.

### Packages — distribution is GitHub

Everything is a package: a `forte init` project pushed to GitHub. The CLI
covers both directions —

```bash
forte init my-album && cd my-album
forte package add github:owner/pkg     # vendor flat into packages/<name>_<version>/
forte package list                     # what you have (desc/tags/license/version)
forte remote add github:you/my-album   # connect the folder to GitHub
forte push                             # publish: source, assets, .forte/ history
forte pull                             # integrate upstream
```

A vendored package never nests: its own `packages/`, `.forte/` and `.git/`
are excluded on copy, its `requires` are hoisted into the consumer's one
flat `packages/`, and `package.lock` pins each source + commit. The
browser's `catalog.html` (served by `forte browser`, backed by
`/api/packages`) lists every vendored package with browsable sources.

Inside a vendored or cloned project, `forte log` shows **your commits
stacked on top of the original author's**, and
`forte diff <their-commit> HEAD` answers "what did I change from the
original?" in musical terms.

### Forte Studio (VSCode)

`editor/vscode-forte/` — diagnostics, Play/Build, REPL (Shift+Enter), plus a
sidebar with **History** (commits / musical diff / checkout). The UI is a
thin wrapper around the `forte` CLI.

## Repository layout

```
crates/dawcore    real-time engine + DSP (lock-free, deterministic, no GUI)
crates/fortelang  the language: lexer/parser/checker, compiler, CLI (check/build/play/lsp/package)
crates/forteweb   C-ABI wasm for the browser (compile, play, build proof)
web/              browser editor + package catalog (PWA)
editor/           Forte Studio (VSCode extension)
packages/         content packages — the Bitwig-style unit of distribution:
  essentials_0.6.0/
    package.forte   package metadata (desc/tags)
    instruments/    148 instruments (808/909/CR-78/303/Juno/SH-101/Prophet …)
    blocks/         250+ reusable blocks across 49 genres
    songs/          28 complete songs (reference songs + examples)
../packages/essentials_0.6.0/instruments/          standard instrument library (148 instruments incl. classic hardware clones)
songs/            reference songs, genre patterns/, full example songs/
docs/webdaw/      vision / system & software requirements / architecture / roadmap
scripts/          determinism gate, browser E2E
```

## Documentation

- **[User guide](../docs/GUIDE.md)** — hands-on tutorial
- **[Language reference](../docs/webdaw/spec/forte-lang-v1.md)** — the `.forte` language, v1
- **[Vision, requirements, architecture](../docs/webdaw/README.md)** — full design docs

## Testing

```bash
forte ci                               # the full merge gate (all of the below)
forte ci quick                         # tests + clippy + determinism only
cargo test -p dawcore -p fortelang     # engine + language + packages + REPL
scripts/determinism_test.sh            # native/wasm bit-identity gate
node scripts/web_e2e.mjs               # browser E2E (needs playwright)
scripts/check_corpus.sh                # every instrument & song compiles + renders
```

## Contributing

Issues and PRs are welcome. Setup and the PR rules — especially the
**determinism gate**: if a change that shouldn't affect the sound moves a digest
by even one bit, CI fails — are in [CONTRIBUTING.md](../CONTRIBUTING.md).
Roadmap-derived tasks live in the
[issues](https://github.com/ForteLang/forte/issues).

## License

- **Software** (crates/, web/, editor/, scripts/, docs/): [MIT](../LICENSE) © 2026 Shusuke Inoue (fcuro)
- **Package content** (packages/ — songs, albums, blocks, instruments, recorded
  assets): [CC BY-NC-SA 4.0](../packages/LICENSE). Fork it, remix it, learn from
  it — but rendering it to audio (WAV/streams) and monetizing that audio
  requires the rights holder's permission. Authors keep full rights to
  license their own work commercially.
