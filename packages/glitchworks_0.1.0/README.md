# glitchworks — the glitch album toolkit

Everything here follows one pipeline: **produce a source (instruments +
insert chains) → bounce it to audio → play the audio through sampler
machines whose parameters are in motion**. Nothing is a raw oscillator by
the time you hear it.

House rules, learned by ear:

- Drums are **body first, noise last** — no noise-burst snare anchors.
- Every groove is a **four-bar arc**, not a loop: bar 4 breaks the pattern.
- Something **drifts at all times** (pitch swim, window slide, stepped flip).
- **The BUG is the groove**: stutters, reversed answers, broken
  artifacts — impossible playback that surprises. Choke cuts still leave
  rests, but an ambient bed keeps sounding under them; TOTAL silence is
  an occasional shock, not the default.
- **Pitch bugs are local or transitional, never global**: if the key
  stops being legible the bug is UNACCEPTABLE. Allowed: a toy layer's
  fixed micro-detune riding a stable progression, a glide-in at bar 1,
  a terminal tape-death. Never modulate the whole record's pitch
  mid-song.
- **The whole mix is the sample**: bounce the full band to ONE record and
  chop THAT — a rest then silences everything at once, reverb tails
  included. That mix-wide dead-stop is the glitch groove.
- Comfy noise is welcome (vinyl wow/crackle/hiss beds, filtered washes) — pleasant, never clicky. (Revised 2026-07; the old "no beds" rule is retired.)
- A steal, a cut, a slice end: everything **declicks**.

## Source records: `records/`

Five melodic source records for future album work — *felt* (piano),
*lattice* (arps), *dawn* (choir), *floor* (a melodic-techno kit with NO
bassline), *lantern* (voice) — all A minor at 120, all carrying one
motif (A C B E D). A first album attempt built on them was scrapped:
everything through a sampler currently collapses to MONO (issue #122),
which flattens exactly the pads/width these records live on. The records
stay; the album returns after stereo sampling lands.

## Genre targets: `profiles/`

Reference profiles for `forte analyze --against`: **glitch-chop** (the
songbook's architecture: real silence share, chop density),
**melodic-glitch** (the album target: wide, bright, club-adjacent) and
**ambient-records** (what a source record should measure like). Plain
JSON target ranges — fork them like everything else. A profile miss is
a work order with a number on it, not an adjective.

## The melodic pressings: `songs/melodic/`

The first songs written WITH the whole 2026-07 core-library wave and
measured against `profiles/melodic-glitch.profile` — wide `uni`/unison
prisma stacks, degree progressions with nearest-motion voice leading,
`vcf` ladder basses, air hats, the band bounced to one record, lightly
chopped, and pressed through the house chain (transient-safe glue →
parcomp → EQ tilt → exciter → limiter, loudness by `level`).
V2 (the bug is the groove, pitch-disciplined): every pressing carries
an ambient BED — a unison drone plus a vinyl'd noise wash that keep
sounding under the chops, so rests reveal a lit room instead of dead
zero. The record stays strictly IN KEY: whole-pitch events are
transitional only (a spin-up glide-in at bar 1; tape-deaths at the
end; still-glass's bar-12 brake into its one great drop). The micro
pitch-bug lives in ONE voice instead — the toy layer, a fixed
few-cent-detuned unison with per-voice vcf drift riding the stable
progression. Reversed copies answer from the gaps; stutter fits seize
the fast one. Remaining profile misses (true peak everywhere; sub
during tape-deaths; cold-letter's high band) are documented physics
and tooling gaps (see the true-peak limiter issue), not hidden.

## Listening: the songbook (`songs/`, ~30 s each)

Three shelves:

- **`songs/dig/`** — crate digging: the glitch songs are the RECORDS.
  `sample Rec = dig("../glitch/glitch-01.forte", beats: 16, skip: 16)`
  renders a whole other song deterministically and hands it to the
  sampler — `end` lands on the musical edge automatically, `semis: 5`
  repitches into the new key, `skip` drops the needle mid-record. Every
  dig song builds its backing out of two or three records (traded cuts,
  simultaneous spins, backwards answers) and then plays its OWN drums,
  bass and lead live on top.

| # | records dug | ours on top |
| --- | --- | --- |
| 01 Crate One | first-cut + funk-fraction, traded in two-beat cuts | BodyBeat, GlideSub |
| 02 Two Turntables | house + club, spinning at once | KickStack, ChipArp |
| 03 Brass on Wax | the two G-minor records, same-key pair | SnareRainbow, UprightWalk |
| 04 Choir Loan | choir over doom slabs, freeze ending | SubDeep, WoodTick |
| 05 Jungle Reissue | amen re-cut harder, chips a fifth up | GlideSub, GlideVox |
| 06 Piano Flip | piano vs violin, the session that never happened | ModalBackbeat, PickBass |
| 07 Bell Exchange | bells vs dusk, every 4th bar backwards | HatWork, SubPulse |
| 08 Tape Arbitrage | F-minor lounge + backspins pulled up a fourth | ShakerSwing, UprightWalk |
| 09 Vox Reprint | voice re-cut into a new sentence, wars as punctuation | WobbleGate, ClapRun |
| 10 The Anthology | THREE records collaged; the closer's ending cut twice | SlowWave |

- **`songs/glitch/`** — the main event: twenty MIX-CHOP songs. Every one
  follows the same law: the full band (a `block Mix` placing the
  machines) is bounced to ONE record (`sample MixS = bounce(Mix, beats:
  16)`), and the song is that record re-cut — slice = one beat of the
  mix, note length = gate length, `choke` on. Every rest is the ENTIRE
  mix going silent at once, reverb tails and all; reversed answers,
  retrig bursts, granular freezes and tapestop/pitch-dive finales are
  cut from the same record. 28–86 mix-wide silences per song, 7–19
  seconds of true zero in each.
- **`songs/sampler_sample/`** — the earlier 34 palette showcases: the
  machines played straight, one mood each. Kept as the reference shelf.

### `glitch/` tracklist

| # | thesis |
| --- | --- |
| 01 First Cut | a kick LINE that eats itself; piano re-cut tighter until the power dies |
| 02 Window Walker | the dusk ensemble scrubbed as a record, upright walking under it |
| 03 Stutter Budget | house floor; the string record's stutter lever pushed further every turn |
| 04 Half Heart | the break at doom speed, lunging; it freezes mid-air at the end |
| 05 Amen Ledger | jungle: the break re-cut every pass, an 808 sub that only slides |
| 06 Funk Fraction | guitar+bass+clap glued as ONE grain, re-cut against a self-editing backbeat |
| 07 Acid Receipt | the 303 slide on audio; the lane ridden down a fourth at the end |
| 08 Brass Debt | dub: one glued brass stab choked into answers, tape dying at each turn |
| 09 Cartridge Splinter | chip ensemble splintered over the body kit; the cartridge jams and gets pulled |
| 10 Violin Economy | a violin phrase and the sampler's opinion of it, over a heartbeat |
| 11 Cutting Room Choir | the frozen vowel wall CARVED — the gate's duty cycle is the melody |
| 12 Pedalboard Alley | fuzz→wah bass, funk guitar recut with rests, stomp-box music sampled |
| 13 Backspin Economy | everything runs backwards at least once |
| 14 Piano Reassembly | the upright taken apart bar by bar, the music box answering in mirror |
| 15 Vox Machine | impossible syllables answered by a voice that only slides |
| 16 Bell Ledger | a music box forwards and backwards at once, a bell bent on the platter |
| 17 Freeze Tag | granular freezes traded like tags; the last bar hangs mid-air |
| 18 Tape Debt | keys losing power at every turn, paying more interest each time |
| 19 Scrub Wars | two records fighting over one turntable, trading scrubbed windows |
| 20 Last Cut | the closer: the whole song held as a chord, ridden down, power cut |

### `sampler_sample/` (the reference shelf)

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
A `dig` record needs NONE of this: `end` defaults to the musical edge,
`semis: -5` repitches in semitones, `skip:` drops the needle mid-record.

```
sample Rec = dig("../glitch/glitch-08.forte", beats: 16, skip: 16)
instrument sampler(sample: Rec, slices: 16, choke: "on", sustain: 1.0,
                   release: 0.05, semis: 5)   // slice = one beat of the record
```
