# glitchworks — the glitch album toolkit

Everything here follows one pipeline: **produce a source (instruments +
insert chains) → bounce it to audio → play the audio through sampler
machines whose parameters are in motion**. Nothing is a raw oscillator by
the time you hear it.

House rules, learned by ear:

- Drums are **body first, noise last** — no noise-burst snare anchors.
- Every groove is a **four-bar arc**, not a loop: bar 4 breaks the pattern.
- Something **drifts at all times** (pitch swim, window slide, stepped flip).
- **Choke cuts leave rests**; the rests are the groove.
- **No hiss/crackle beds** — the music stays clean; patina is opt-in per song.
- A steal, a cut, a slice end: everything **declicks**.

## Listening: the songbook (`songs/`, ~30 s each)

Openers/closers: `cold-open`, `last-window`.

| mood | songs |
| --- | --- |
| ambient / environment | cold-open, glass-orchard, small-hours, static-lake, slow-postcards, paper-birds, rainy-arcade, last-window |
| lofi / dusty keys | wool-socks, paper-piano, felt-hills, iron-lullaby, copper-lane, sun-through-blinds, dust-ballroom |
| grooves / funk / dub | midnight-vending, street-chess, broken-elevator, vapor-market, spring-loaded, warm-copper, brass-weather, pocket-orchestra |
| house / club | eight-mirrors, eleven-windows, tin-orbit, neon-moss |
| acid / breaks / chip | acid-postcard, laser-garden, gravity-off, two-tapes, cartridge-dream, horizon-modem, tunnel-air |

Earlier audition sketches (`sketch-*.forte`) are process artifacts, kept for
reference.

## The machines (`wrapped/`)

- **glitchset.forte** — the album toolkit: GLIDE family (GlideSub/Vox/
  Strings/Acid/Bell/Brass — wrapped audio sliding between held notes),
  ENSEMBLE family (Dusk/Choir/Funk/Brass/Chip/Glass — several instruments
  bounced together, chopped as one grain), and machines/kits
  (WindowWalkDusk, FreezeChoirHard, ReCutPiano, StutterStrings,
  BackspinPerc, TapeDropKeys, BodyBeat, TomGroove, WoodTick, LowKit,
  HeartSub, CaveDrip, DustClock, FarBell, SlowWave).
- **drumcolor.forte** — SnareRainbow (one rich snare, every hit a different
  EDIT: gate length, pitch, headless cuts, smears, reversed pre-echoes),
  KickStack (sub+punch+click as one hit, three weights), SlicerBeat.
- **grooveworks.forte** — 12 drum/perc grooves with four-bar arcs.
- **bassworks.forte** — 12 bass machines: AcidHalf, SquareRoll, SH101Drive,
  UprightWalk, WobbleGate, SubPulse, PickBass, BassStabEcho, LaserBass,
  OctaveHouse (the octaver pedal), FuzzWahBass (fuzz into an LFO wah),
  SubDeep.
- **melodyworks.forte** — 14 melodic chops: guitars, pianos, vibes, voice
  cut-ups, ViolinSolo, ChipArp, BraindanceKeys, PianoLoop, SweepScrub…
- **textureworks.forte** — 12 textures/transitions: CrashRise, NoiseWall,
  ChoirGrain, ZapArp, ImpactStamp, ColdPulse, TapeAir (the one opt-in
  vinyl bed), PowerWall, RainField, BirdSong, StreetAir.
- **chopworks.forte** — the engineered-chop rack: ScrubStrings, GhostChoir,
  AcidFlip, BreakMachine, KeysCarve, DustBassLoop, GlassKeyPad, KickFlip,
  MomentKit (a whole production as one playable kit rack).
- **drums.forte / bassx.forte / …** — the original wrapped one-shots.

## The engineering surface

Every chop parameter is automatable/modulatable by name:
`start end pitch decay sustain release glide slices choke vary stretch`
plus every insert's params (`gate.duty`, `stutter.mix`, `tapestop.amount`,
`duck.amount`, `vinyl.wow` …).

Signature moves:

```
automate decay from 0.6 to 0.1 over bars(5..8)      // the cut tightens
automate stretch from 0.5 to 0.02 over bars(8..8)   // granular freeze, no pitch fall
modulate start with steps(seq: "…", every: "1/8", amount: 0.25)  // window walks
insert duck(from: Hats, mode: "key", amount: 1.0)   // pad exists only where hats hit
sample T = bounce(MyBlock, note: C3, beats: 8)      // phrase → audio (+2 beats tail)
instrument kit(C1: hitA, C2: hitB, wrap: WholeTake) // pads raw, other keys repitch the take
```

Slice math: a bounce carries +2 beats of tail — 4-beat source → `end:
0.667`, 8-beat → `end: 0.8`. Slice n = bounce note + n semitones.
