# studio/ — bootstrap for the Forte Studio fork (ADR D-14)

Forte Studio is a fork of Code - OSS (microsoft/vscode), the Cursor way:
VS Code's machinery stays, the workbench becomes a DAW. The fork lives in
its own repository (`fortelang/forte-studio`); THIS directory holds what is
needed to reproduce the fork from scratch and keeps the product definition
under this repo's review:

- `product.json` — the product overlay (name, dirs, Open VSX gallery).
  Config, not a patch: it is copied over upstream's file.
- `bootstrap.sh` — clones upstream at the pinned tag, applies the overlay,
  vendors the bundled `vscode-forte` extension, and prints the build
  commands for the web and desktop targets.

Design and milestones: `docs/webdaw/09-forte-studio-fork.md`.
Fork rule of thumb: all Forte workbench code goes in
`src/vs/workbench/contrib/forteDaw/` (one rebase junction); anything the
extension API can express stays in `editor/vscode-forte`.

Container notes (2026-07): cloning microsoft/vscode and building the WEB
target works in the dev container (native modules proven by the code-server
demo). Electron downloads are blocked there, so desktop builds run on a dev
Mac or CI.
