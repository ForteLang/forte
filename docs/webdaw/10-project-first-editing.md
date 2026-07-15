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
