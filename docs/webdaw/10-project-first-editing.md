# Project-first editing (ADR D-15)

Status: Approved direction / 2026-07-15
Upstream: 09-forte-studio-fork.md (D-14), 08-daw-functional-requirements.md
(DAW-PRJ rows, P0 spike status)

## 1. What was decided, in the maintainer's words

> The current shape — pick one file and enter the editor — is not right.
> `forte init` creates a package whose elements are blocks and songs;
> editing must reach ALL of them or it is meaningless. A block is edited
> as a block; when you make a song you edit it as a song. Every editing
> task that starts from a forte init project must be completable inside
> the GUI editor. Anything less is not acceptable.

## 2. The decision

1. **The unit the GUI opens is the PACKAGE** — the directory `forte init`
   creates — never a bare file. "Open" means open the project; the first
   surface is the project map (package meta, `blocks/`, `songs/`,
   `instruments/`, `albums/`, vendored `packages/`), not a text buffer.
   Single-file entry is not a mode.
2. **Element-appropriate editors over one medium.** A song opens as a
   song (the Composer: arrange / grid / mixer over the song body); a
   block opens as a block (the same surfaces bound to the block's body —
   blocks are playable, so audition works standalone); an instrument /
   effect device opens as its grid-graph projection with `set_arg`
   knobs; `package.forte` opens as the metadata form. Every view stays a
   projection of the code (law 1, SYS-EDT-003). The edit layer already
   addresses bodies by `path`, so block-as-block editing is the same op
   stream as song editing — the substrate was built project-ready; the
   hosts were not.
3. **Cross-file gestures are first-class GUI operations**: create a new
   block / new song from a template, import a block into a song (writes
   the `import` statement), place it on the timeline (`add_place`),
   audition any block from the explorer. The project explorer is a write
   surface, not a listing.
4. **CLI-first, like the edit layer** (D-14 §3.2): `forte project`
   emits the machine-readable project inventory — the read side any host
   (web editor, vscode-forte, the Studio fork) binds its explorer to.
   Writes go through `forte edit` (per file, body-addressed). The fork's
   project-home UI consumes exactly these two commands.

## 3. Gap map (what exists vs. what this changes, 2026-07-15)

| Surface | Today | Under D-15 |
| --- | --- | --- |
| Web editor | `loadSong` opens ONE file; the file tree lists files but each is an isolated surface; grid/arrange bind to the open file only | Opens the project; explorer groups by element (songs / blocks / instruments); grid/arrange/mixer bind to whichever body is opened, block or song |
| vscode-forte | Panels remember the LAST `.forte` file — the same single-file frame | Panels bind to the project; the DAW view follows the selected element, not the last file |
| Read side | `packages_json` is a listening catalog of vendored packages, not an editing inventory of the root project | `forte project` inventories the root package for editing: every song, block, device with names, lines and edit coordinates |
| Write side | `forte edit` is already body-addressed (`path: ["A","B"]`) | Unchanged — this is why the change is host-side, not engine-side |

## 4. Milestone impact

F1/F2 acceptance in 09-forte-studio-fork.md are re-phrased project-first:
opening the project is the entry point (F1); the full-length workflow
starting at `forte init` — new block, audition it, import-and-place into
a song, mix it — is completable without leaving Studio (F2).

## 5. One DAW (maintainer directive, same day)

> Unify all of this into Forte's DAW. For a package that already exists
> or one just made with forte init, `forte daw [project path]` opens the
> GUI editor, and there you create multiple songs and blocks. A basic
> DAW works per song; Forte works per PACKAGE: define lots of blocks,
> fork other people's packages, combine them, grow the blocks — into an
> album.

Clearing up the naming confusion this directive answered:

| Name | What it is | Fate under this directive |
| --- | --- | --- |
| `forte edit` | The CLI write path (JSON ops → minimal text splice). Not a GUI. | Stays — it is the write substrate every GUI uses |
| Web editor (`forte browser`) | The browser app in web/ — wasm compiler, OPFS local files, demo songs, catalog | Becomes the DAW's demo/hosted mode; same app |
| **`forte daw [DIR]`** | **THE DAW**: the same browser app opened on a real package via the project API — real files, project tree, new block/song, package add | **The one entry point for composing** |
| Forte Studio (D-14 fork) | The VS Code fork | The desktop shell that will embed the SAME surfaces and the same two commands (`forte project` read, `forte edit` write); F1's project map = the `forte daw` project view |

Implementation (landed with this ADR): `forte daw [DIR]` serves web/
with a project API (`/api/project`, `/api/list`, `/api/modules`,
`/api/assets`, `/api/src` GET/POST, `/api/new` block/song scaffolds,
`/api/pkg` = forte package add). The web app detects the API and flips
to project mode: the tree is the package (with +曲 / +block / +package
controls), files load from and autosave to DISK, and imports resolve
from the open file's own directory (`fw_base_commit` carries the base
dir into the wasm compiler). OPFS demo mode is untouched when no API is
present.

Second slice (same branch): the block library is a working surface. The
tree's `blocks` section lists every block in the package (with its bar
length from the inventory); `▶` auditions a block standalone (a block
library compiles with its last block as root), and `+曲` runs the
library gesture — `add_import` (a new edit op: merges into an existing
import of the same path, no-ops when present, else inserts below the
last import) followed by `add_place` after the open song's last used
bar. `forte hub fork` (PRJ-03) still needs its CLI before its GUI.

Third slice — the transport and the mixer (DAW-MIX-01/02/08):
`viz_json` carries each track's volume/pan; a mixer panel under the
arrange renders one strip per track (live peak meter, volume fader,
pan) whose release-writes go through `set_track` — the fader IS the
code. M/S are engine-side monitor state for the session only (the
worklet's `mute` command), never written to source. Space = play/stop
(except while typing or performing), double-click in the arrange seeks
the playhead. Known limit: `set_track` targets tracks defined in the
OPEN file, so mixer writes work in block files and inline-track songs;
strips for tracks that arrive via `play <ImportedBlock>` report the
edit error instead (routing those to the block's own file is the next
step).
