<p align="center">
  <img src="forte/assets/logo.svg" width="112" alt="Forte">
</p>

<h1 align="center">Forte</h1>
<p align="center"><b>Compose music as code.</b></p>
<p align="center">
  Songs, instruments, effects, albums — all readable, forkable source code,
  distributed as packages on GitHub.
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/code-MIT-blue.svg" alt="MIT"></a>
  <a href="packages/LICENSE"><img src="https://img.shields.io/badge/content-CC%20BY--NC--SA%204.0-orange.svg" alt="CC BY-NC-SA 4.0"></a>
</p>

---

Forte is a language, an engine, and a package ecosystem for open music.
Everything is source code — and builds are **deterministic**: the same
commit renders bit-identical audio on native, wasm, and in the browser, so
anyone can re-verify any release from a browser tab.

```forte
import { AcidLine } from "packages/essentials_0.6.0/blocks/acid-line.forte"
import { FourFloor } from "packages/essentials_0.6.0/blocks/four-floor.forte"

song "Block Party" {
  desc "Two reusable blocks, arranged: the song only decides when and in which key."
  tempo 126bpm
  key A minor
  play FourFloor at bars(5..12)
  play AcidLine  at bars(5..12)                  // a 4-bar block, looped
  play AcidLine(key: "D minor") at bars(13..20)  // same line, a fourth up
}
```

## Install

```bash
# Linux needs ALSA headers: sudo apt install libasound2-dev
git clone https://github.com/ForteLang/forte && cd forte
cargo install --path forte/crates/fortelang     # installs the `forte` command
echo 'source <(forte complete bash)' >> ~/.bashrc   # tab-completion (zsh: forte complete zsh)
```

## Listen

**Right now, in your browser** — nothing to install:
**[fortelang.github.io/forte/catalog.html](https://fortelang.github.io/forte/catalog.html)** —
open the First Light album and press play. Your browser compiles the code
and renders the exact same bits as everyone else's.

Or with the CLI:

```bash
forte play packages/essentials_0.6.0/songs/smiley-acid.forte
```

`forte play` is a console player: every track's lane, a moving playhead,
and the song's own description. Save the file while it plays — it hot-reloads.

Or play a whole album — tracks ship as **`.fortesong`** (self-contained,
digest-verified builds; the code rides inside), and `forte play` becomes an
audio player with `space` pause, `n`/`p` track skip, and auto-advance:

```bash
forte play packages/essentials_0.6.0/albums/first-light
```

No install at all? `forte browser` serves a **zero-install player**
(`player.html`): the album plays in the browser from the same code, same
digest — and the catalog links every album straight into it.

## Play instruments

```bash
forte instruments list          # the catalog: 150 devices with params
forte instruments list 808      # filter by name or library
forte instruments play Bass303  # your keyboard becomes a piano
```

Keys `a w s e d f t g y h u j k …` are a chromatic run from C; `z`/`x` shift
the octave, `c`/`v` velocity, digits `1`-`9` grab the instrument's knobs and
`-`/`=` turn them while you play. Quit, and your jam is printed as source
code you can paste into a song.

## Compose

Music is made of **blocks** — self-contained, reusable pieces that nest,
transpose, loop, and inherit:

```forte
block DarkAcid : AcidLine {                       // inherit and override
  track Acid {
    instrument Square303(cutoff: 0.18, reso: 0.9)  // swap the voice
    insert reverb(size: 0.8, mix: 0.25)            // add an effect
  }
}
```

Start with the **[User guide](docs/GUIDE.md)**, keep the
**[language reference](docs/webdaw/spec/forte-lang-v1.md)** nearby, and open
the browser editor with `forte browser` (diagnostics as you type, playback,
fully offline).

## Packages

All content lives in **packages** — themed, versioned collections
(`packages/essentials_0.6.0/` ships 150 instruments, 320+ blocks and 28
songs). Your own project is a package too:

```sh
forte init my-album          # scaffold: package.forte + blocks/ songs/ packages/
cd my-album
forte package add github:fortelang/forte   # vendor a package into packages/
forte package list           # what you have, in each package's own words
forte remote add github:you/my-album       # connect the project to GitHub
forte push                   # publish — the pushed project IS the package
```

Every dependency lands flat in `packages/<name>_<version>/` (a package's
`requires` are hoisted next to it — never nested), and `package.lock`
records exactly what was fetched. Push your project to GitHub and it IS a
package: others run `forte package add github:you/my-album` and import
just the blocks they want.
Package content is licensed [CC BY-NC-SA 4.0](packages/LICENSE): fork and
remix freely; commercializing rendered audio needs the author's permission.

## Repository

```
forte/      the core: engine, language, CLI, browser editor (see forte/README.md)
docs/       user guide, language spec, design docs
packages/   content packages (essentials_0.6.0, …)
```

Core development — building from source, the determinism gate, testing —
is documented in **[forte/README.md](forte/README.md)**. Contributions
welcome: see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

- **Software** (`forte/`, `docs/`): [MIT](LICENSE) © 2026 Shusuke Inoue (fcuro)
- **Content** (`packages/`): [CC BY-NC-SA 4.0](packages/LICENSE)
