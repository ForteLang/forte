# Forte Studio: the VS Code fork (ADR D-14)

Status: Approved direction / 2026-07-14
Upstream: 04-software-architecture.md (D-14), 08-daw-functional-requirements.md
Supersedes: the extension-only framing of D-13 (the extension survives as the
fork's bundled core), the Tauri shell of issue #135 (dropped earlier).

## 1. What was decided, in the maintainer's words

> A plugin means the right side only shows visualizations and rich GUI
> operation is impossible. I don't want a plugin running on VS Code — I want
> the Cursor image: fork VS Code, keep its basic machinery, and develop it
> into a DAW.

Forte Studio is therefore a **fork of Code - OSS (microsoft/vscode)**, the
way Cursor forks it: VS Code's editor, terminals, SCM, LSP, remote dev and
extension ecosystem stay; the **workbench itself** becomes a DAW.

## 2. The plugin ceiling (why the extension was not enough)

The extension prototype (0.3.0) proved the projections work — and mapped
exactly where the extension API tops out:

| Wanted | Extension API reality |
| --- | --- |
| Opening a `.forte` file lands you in a **DAW view with code beside it**, as ONE editor | A `CustomEditor` can replace the text editor, but then you lose the text editor (and Monaco is not embeddable inside a webview); side-by-side means two loosely-coupled tabs. No first-class "split document" editor exists for extensions |
| **Transport in the workbench chrome** (play/stop/bar/meters always visible, space = play) | Status-bar text items only; no custom widgets, no meters, no global keybinding that wins over the editor's space key |
| **DAW-grade layout by default** (arrange bottom, mixer panel, library left) | Extensions cannot own or ship a workbench layout; every user assembles panels by hand |
| Drag & drop between library, timeline, editor | Webviews are isolated iframes; cross-view drag needs workbench-level DnD |
| A product that IS a DAW (name, icon, defaults, bundled AI/GitHub/Forte) | Extensions ride inside someone else's product |

Everything ABOVE this ceiling goes into the fork. Everything below it stays
in the bundled extension — the patch set must stay thin (Cursor's lesson:
they rebase on upstream continuously; a fat fork dies at the first rebase).

## 3. Shape of the fork

### 3.1 What stays VS Code (untouched upstream code)

Editor (Monaco), terminals (Claude Code runs here), built-in git SCM,
GitHub/PR extensions, LSP client machinery, settings/keybindings UI, remote
development, the extension host and marketplace client (pointed at Open VSX
— Microsoft's marketplace ToS excludes forks, same as Cursor/VSCodium).

### 3.2 What the fork adds (all inside `src/vs/workbench/contrib/forteDaw/`)

One contrib folder, so rebases touch us at exactly one junction:

1. **The Composer** — the `.forte` editor experience. A first-class
   `EditorPane` that hosts BOTH the real text editor and the DAW canvas over
   one shared text model: arrange timeline on top, code below, one undo
   stack, cursor↔clip selection synced both ways. Reads via `forte viz` /
   `forte edit --sites`, writes via `forte edit` (the lossless edit layer —
   unchanged since P0, it was built CLI-first precisely so any host can own
   it). Grid / piano roll / mixer / inspector are tabs of the same pane, not
   side panels.
2. **Transport part** — a workbench part in the title bar area: play/stop,
   bar:beat counter, master meter, LUFS readout, the build digest. Space =
   play/pause (except when a text editor has focus and is mid-edit),
   consistent with every DAW on earth.
3. **Audio service** — a workbench service wrapping playback. F1: spawn
   `forte play` (the CLI's hot-reload path) and read meters over a pipe.
   F3: replace the subprocess with an N-API binding of forte-core for
   in-process, low-latency audio and 60 fps meters.
4. **Product identity** — product.json (name "Forte Studio", own icon, own
   data dir, Open VSX gallery), default layout, welcome page, bundled
   extensions: `vscode-forte` (LSP, blocks, history) + recommended GitHub +
   AI assistant.

### 3.3 Repo strategy

- New repo **`fortelang/forte-studio`** = fork of microsoft/vscode, pinned
  to an upstream stable tag. All Forte code lives in the ONE contrib folder
  plus `product.json` — the diff against upstream IS the product.
- This repo (`fortelang/forte`) stays the home of the engine, language, CLI,
  extension and web editor. `studio/` here holds the bootstrap script and
  the product.json overlay so the fork can be reproduced from scratch, and
  CI here never depends on the fork.
- Upstream tracking: rebase the contrib folder onto each VS Code stable
  release (monthly). The contrib-folder isolation makes this mostly
  mechanical; product.json is config, not a patch.

### 3.4 Feasibility notes (probed 2026-07-14, this container)

- `git clone microsoft/vscode` works from the dev container; npm registry
  reachable → **the web target of the fork can be built and screenshotted
  here** (same path code-server uses; its native modules already build in
  this container — proven while setting up the D-13 demo).
- Electron binary downloads are blocked here → **desktop builds happen on a
  dev Mac / CI**, not in this container. macOS signing/notarization comes
  back as a workstream (it had disappeared with Tauri; it is the price of
  the fork and it is worth it).

## 4. Milestones

| # | Deliverable | Acceptance |
| --- | --- | --- |
| **F0** | Fork bootstrapped: branding, Open VSX, bundled vscode-forte, web target builds | Open a folder in the browser build, `.forte` diagnostics live, song plays from the bundled extension |
| **F1** | The Composer v1 + transport part + project map (D-15) | Opening the PROJECT (the forte-init package) lands on its map — songs, blocks, instruments; opening a song from it shows arrange-over-code in ONE tab; drag a clip → code changes under the same undo stack; space plays; bar counter runs |
| **F2** | Grid / piano roll / mixer / inspector tabs; block-as-block editing; Blocks library drag-to-place | The full-length workflow starting at `forte init` — new block, edit it AS a block, audition it, import-and-place into a song, mix — is doable without leaving Studio (D-15) |
| **F3** | In-process engine (N-API forte-core), 60 fps meters, recording UI | Latency & meters feel like a DAW, not a viewer; `.frec` capture with provenance |
| **F4** | Desktop ship: signed/notarized DMG, auto-update, first dogfood album composed in Studio | The maintainer composes and commits a full track start-to-finish in Forte Studio |

## 5. Risks

- **Rebase debt** — mitigated by the one-contrib-folder rule and monthly
  cadence; anything expressible in the extension stays in the extension.
- **Engineering weight** — a fork is a product-scale commitment (Cursor has
  a company behind theirs). The counterweight: Forte's DAW surfaces already
  exist as portable web code driven by a CLI, so the fork's own code is thin
  glue, not a DAW rewrite.
- **Marketplace** — Open VSX has most of what matters (incl. AI assistants);
  specific missing extensions can be side-loaded as VSIX.
