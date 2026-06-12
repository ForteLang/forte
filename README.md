# Bitwig Studio 6 — Web Clone

A browser-based reimplementation of [Bitwig Studio 6](https://www.bitwig.com)'s
core workflow and look, built with **React + TypeScript + Vite** and the
**Web Audio API**. It actually makes sound — a polyphonic synth, insert
effects, modulators, a looping Clip Launcher and a piano-roll note editor are
all live.

> **Scope honesty.** Bitwig Studio is a commercial, native C++ application with
> a proprietary audio engine, VST/CLAP plugin hosting, and The Grid modular
> environment. A 1:1 binary-identical clone is not achievable outside Bitwig
> GmbH. This project faithfully recreates Bitwig 6's *signature concepts,
> workflow, and visual language* in the browser, not its internal engine.

## Quick start

```bash
npm install
npm run dev      # http://localhost:5173
```

```bash
npm run build    # type-check + production bundle into dist/
npm run preview  # serve the production build
```

Click anywhere first (browsers require a user gesture to start audio), then
hit **Play** (or Space).

## What's implemented

| Bitwig 6 feature | Status in this clone |
| --- | --- |
| **Hybrid Arranger / Clip Launcher** | Clip Launcher grid with per-track slots and scenes; click to launch, scene row to launch a whole scene |
| **Refreshed dark v6 UI** | Permanent dark theme, rounded edges, layered near-black panels, Bitwig accent orange |
| **Project key signature** (new in v6) | Global root + scale selector in the transport; piano roll highlights in-scale notes |
| **Track types** | Instrument / Audio / Effect tracks |
| **Device chains** | Per-track ordered chain of instrument + insert effects, bypass per device |
| **Built-in instrument** | *Polymer* — polyphonic subtractive synth (osc + sub, filter, ADSR) |
| **Insert effects** | Filter+, EQ-5, Distortion, Delay-4, Reverb (convolution) |
| **Modulators** | Per-device LFO modulators routable to any parameter, with live bipolar depth — Bitwig's signature modulation system |
| **Mixer** | Channel strips with faders, pan, mute/solo, live RMS meters, master bus |
| **Piano roll** | Add / move / resize / delete notes, snap-to-grid, in-scale highlighting, playhead |
| **Transport** | Play / stop, tempo, time signature, metronome toggle, bar.beat.tick position |
| **Computer-keyboard MIDI** | Play the selected instrument with A–L / W,E,T,Y,U |

## Architecture

```
src/
  audio/
    AudioEngine.ts      Web Audio graph, look-ahead scheduler, transport clock, modulator ticking
    Polymer.ts          polyphonic synth voice (osc + sub → filter → ADSR amp)
    devices/effects.ts  Filter / EQ / Drive / Delay / Reverb insert effects
  state/
    store.ts            Zustand store — single source of truth, syncs into the engine
    types.ts            data model (tracks, clips, scenes, devices, modulators)
    devices.ts          device factory + parameter metadata
    music.ts            scales, MIDI↔frequency, note naming
  ui/
    App.tsx             layout shell, keyboard input, RAF transport sync
    TransportBar / Inspector / Browser / ClipLauncher / Mixer / DevicePanel / PianoRoll / Knob
  theme/theme.ts        colour tokens approximating Bitwig 6
```

**Data flow:** the Zustand store holds the entire project. Every mutation calls
`engine.syncTracks()`, which reconciles the Web Audio graph (creating/updating/
removing track nodes and rebuilding device chains). The engine owns real-time
concerns only — a 25 ms look-ahead scheduler converts clip notes into precisely
timed Web Audio voice triggers, and a `requestAnimationFrame` loop advances LFO
phases and pushes modulated parameter values into live audio nodes.

## Known simplifications vs the real Bitwig 6

- No audio-clip recording/playback, sampler, or VST/CLAP plugin hosting.
- The Grid, comping, automation lanes, and clip aliases are not implemented.
- Effects are lightweight Web Audio approximations, not Bitwig's DSP.
- Single-window layout; no detachable panels or multi-monitor displays.

These are deliberate scope boundaries for a browser clone, not bugs.
