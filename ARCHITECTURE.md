# Architecture

How this DAW is organised so it can keep growing without collapsing under its
own weight.

## The device taxonomy (the load-bearing decision)

Every device is exactly one of three **signal-transformation stages**,
mirroring Bitwig's own taxonomy:

| Stage | Transform | Trait | Examples |
| --- | --- | --- | --- |
| Note FX | Note → Note | `device::NoteFx` | Arpeggiator, Transposer, Note Repeat |
| Instrument | Note → Audio | `device::Instrument` | Polymer, Sampler, Poly Grid |
| Audio FX | Audio → Audio | `device::AudioFx` | Filter+, EQ-5, Distortion, Delay-4, Reverb |

A track renders each block as a fixed pipeline:

```
scheduled notes ─┐
live input ──────┴─► [Note FX chain] ─► [Instruments] ─► Σ ─► [Audio FX chain] ─► fader/pan ─► master
                                          audio clips ──┘                   └─► post-fader sends
```

Note events are block-local `(sample, on, pitch, velocity)` tuples; Note FX may
transform, swallow or emit events (with a `BlockCtx` carrying tempo/beat so
arps and repeats stay phase-locked to the transport — or free-run when
stopped, so live performance works without pressing play).

### Adding a device

1. Implement **one trait** in `crates/dawcore/src/device.rs` (or wrap existing
   DSP from `dsp/`).
2. Add a `DeviceKind` variant in `crates/dawcore/src/model.rs` with its
   `stage()`, `label()`, `params()`, `defaults()` (and `options()` for
   dropdowns), and append it to `DeviceKind::ALL`.
3. Add one arm to `device::build_dsp`.

That's it. The browser sections, device-panel knobs, modulation routing,
save/load and the engine pipeline all derive from that metadata. Nothing else
changes.

## Real-time discipline

The audio callback never allocates, locks, or syscalls:

- UI → audio messages cross a lock-free SPSC ring (`ringbuf`). Hot messages
  (notes, params) are `Copy`; structural payloads are built on the UI thread
  and shipped as `Box`es.
- Displaced heap objects return through a **garbage channel** and are dropped
  by the UI thread (`mem::swap` keeps even replacements allocation-free).
- Note FX use bounded pushes (`push_bounded`) into pre-allocated event
  buffers; the chain ping-pongs between two reused `Vec`s.
- Readback (position, meters, voice counts) is published via atomics.

## Crate layout

```
crates/dawcore   engine + DSP, no GUI/hardware deps (unit-testable offline)
  device.rs      the three stage traits + implementations + factory
  dsp/           oscillators, envelopes, filters, sampler, grid interpreter…
  engine.rs      audio-thread engine: scheduler, note-fx pipeline, mixer, sends
  model.rs       serialisable project model (single source of truth for the UI)
  command.rs     UI→audio protocol + garbage returns
  bounce.rs      offline render (same engine, faster than real time)
crates/dawapp    cpal backend (with silent fallback) + egui front-end
```

## Key map

Two layers, toggled with **Tab** (transport shows an orange `PLAY` chip while
performance mode is on):

| Key | Command mode | Performance mode |
| --- | --- | --- |
| Space | play/stop | play/stop |
| Tab | enter performance mode | back to command mode |
| A–K / W E T Y U | — | play the selected instrument |
| L / M | loop / metronome | — |
| B / I / D | toggle browser / inspector / bottom panel | — |
| F1 / F2 / F3 | Arrange / Launcher / Mix | same |
| ↑ / ↓ | select track | — |
| Enter / Delete | open-or-create / clear selected clip slot | — |
| Ctrl+T / Ctrl+Shift+T | new instrument / effect track | same |
| Ctrl+Z / Ctrl+Y / Ctrl+S | undo / redo / save | same |
| Esc | close editors & dialogs | same |

Launcher mouse model (Bitwig-style): the **left strip (▶)** of a clip
launches/stops it; clicking the body selects; double-click opens the editor;
double-click an empty slot creates a clip.

## Visual regression tests

`scripts/visual_test.sh` captures six deterministic app states (arrange,
launcher, mix, piano roll, grid editor, and a 960×620 window) under Xvfb with
software GL, and compares them pixel-wise against goldens in
`tests/visual/golden/` (ImageMagick `compare`, 3% per-pixel fuzz, fail at
>0.4% differing pixels — catches layout drift such as track-lane
misalignment while tolerating antialiasing).

```bash
cargo build --release -p dawapp
scripts/visual_test.sh check    # compare against goldens
scripts/visual_test.sh update   # re-capture goldens after intended UI changes
```

Requires `Xvfb` and `imagemagick` (`apt install xvfb imagemagick`); Linux
builds also need `libasound2-dev` for cpal.
