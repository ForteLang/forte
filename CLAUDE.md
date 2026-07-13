# CLAUDE.md — working rules for this repository

## Package & album production discipline (hard rule)

An album is the END of a pipeline, never the start. The pipeline is:

1. **Research the genre's actual production techniques first.** What
   sampler operations, effect chains and arrangement idioms define the
   genre in the real world? Name references and name the techniques.
   Do not guess from the genre label.
2. **Build the vocabulary**: instruments and effects that implement those
   techniques. The palette comes before any song.
3. **Write short block songs** (like the unpublished songs in
   `packages/essentials_0.6.0/songs/patterns/` and `examples/`) that
   exercise the palette. These are for the maintainer to LISTEN to and
   judge.
4. **Only after the maintainer approves the sound** do full 3-minute
   tracks and an album happen.

Never ship an album while groping in the dark and call the job done.
(Learned the hard way with the first Raw Signal attempt: sine waves +
clicks passed the gates but missed the genre completely.)

## What glitch actually is (so it is never misread again)

Glitch is a SAMPLER phenomenon, not an oscillator one. The instruments
built so far synthesize waveforms and play them at a pitch. Glitch takes
the RESULT of that — the waveform as audio — and manipulates the audio
itself:

- repitching audio after the fact (audio-domain pitch ≠ oscillator
  pitch: it drags formants, artifacts and time along with it)
- chopping audio mid-flight with an ADSR/gate as a sampler operation —
  the unnatural cut, and the rest it leaves behind, IS the groove
- glides born from post-hoc audio pitch bends, stutters born from
  re-triggering a buffer, textures born from wrapping an existing
  instrument's render in a sampler and abusing start/end/loop/reverse
- e.g. wrap an 808 kick render in a sampler and repitch it low → the
  hip-hop 808 sub bass. Sampler-wrapped instruments + effect chains are
  how one palette becomes a hundred.

Saturation and guitar-pedal-style processing (fuzz, wah, chained
stomp-box effects) are part of the same audio-domain vocabulary and are
required, not optional.

Maintainer refinement (2026-07, after the melodic pressings): the BUG
is the groove, not the silence. Glitch means bug-like behavior —
"impossible music" that surprises. Concretely:

- Do not over-index on dead silence. A continuous ambient bed keeps
  sounding UNDER the chopped record, so a rest in the record reveals
  the bed instead of true zero. Total mix-wide silence still happens,
  but as an occasional shock, not the default groove.
- Pitch bugs are a BALANCE question (2026-07 refinement): if the
  song's original key stops being legible, that is an UNACCEPTABLE
  bug. Whole-record pitch wobble/leans/sags mid-song are 不快 — never
  modulate or automate the full mix's pitch while it plays. Acceptable
  forms: toy-piano MICRO detune on ONE element riding a stable
  progression; a glide-in at the very start (the record spinning up
  into place); a terminal tape-death at the very end. Transitional or
  local, never global and sustained.
- Pleasant, non-clicky noise belongs (vinyl wow/crackle/hiss beds,
  filtered noise washes) — comfy noise, not プチプチ.
- More broken-sounding artifacts: stutters, reversed answers, crushed
  moments, granular freezes — things a correct player would never do.

## Standing rules

- Merge gate: `forte ci quick` must pass (exit code checked) before any
  commit lands; determinism digests are sacred.
- All docs and package content in English; CLI messages in Japanese.
- Commits authored as the repository's configured git user with no
  co-author trailers and no model identifiers in any pushed artifact.
- GitHub Actions workflows must never be (re)added; the internal
  pages-build-deployment workflow is fine.
