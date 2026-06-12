import { midiToFreq } from '../state/music'

/**
 * Polymer — a compact polyphonic subtractive synth voice engine, modelled on
 * Bitwig's "Polymer" device: oscillator → filter → amp envelope.
 *
 * Parameters (0..1 unless noted):
 *   wave      0=sine 1=saw 2=square 3=triangle (quantised)
 *   cutoff    filter cutoff
 *   reso      filter resonance
 *   attack    amp env attack  (seconds-ish, scaled)
 *   decay     amp env decay
 *   sustain   amp env sustain level
 *   release   amp env release
 *   detune    unison detune amount
 *   subOsc    sub-oscillator level
 */
export class Polymer {
  private ctx: AudioContext
  out: GainNode
  params: Record<string, number>

  constructor(ctx: AudioContext, params: Record<string, number>) {
    this.ctx = ctx
    this.params = params
    this.out = ctx.createGain()
    this.out.gain.value = 1
  }

  static defaults(): Record<string, number> {
    return {
      wave: 1,
      cutoff: 0.65,
      reso: 0.15,
      attack: 0.01,
      decay: 0.3,
      sustain: 0.6,
      release: 0.25,
      detune: 0.12,
      subOsc: 0.3,
    }
  }

  private waveType(): OscillatorType {
    const w = Math.round(this.params.wave)
    return (['sine', 'sawtooth', 'square', 'triangle'][w] || 'sawtooth') as OscillatorType
  }

  /** Trigger a note. Returns a release function to stop it. */
  play(pitch: number, velocity: number, when: number, cutoffMod = 0): () => void {
    const ctx = this.ctx
    const freq = midiToFreq(pitch)
    const vel = velocity / 127

    const p = this.params
    const detuneCents = p.detune * 20

    // Two detuned oscillators for width + optional sub
    const osc1 = ctx.createOscillator()
    const osc2 = ctx.createOscillator()
    const sub = ctx.createOscillator()
    osc1.type = osc2.type = this.waveType()
    sub.type = 'sine'
    osc1.frequency.value = freq
    osc2.frequency.value = freq
    sub.frequency.value = freq / 2
    osc1.detune.value = -detuneCents
    osc2.detune.value = detuneCents

    const oscMix = ctx.createGain()
    oscMix.gain.value = 0.5
    const subGain = ctx.createGain()
    subGain.gain.value = p.subOsc * 0.6

    const filter = ctx.createBiquadFilter()
    filter.type = 'lowpass'
    const baseCut = 120 + Math.pow(Math.min(1, p.cutoff + cutoffMod), 2) * 11000
    filter.frequency.value = baseCut
    filter.Q.value = 0.5 + p.reso * 18

    const amp = ctx.createGain()
    amp.gain.value = 0

    osc1.connect(oscMix)
    osc2.connect(oscMix)
    oscMix.connect(filter)
    sub.connect(subGain)
    subGain.connect(filter)
    filter.connect(amp)
    amp.connect(this.out)

    // ADSR
    const a = Math.max(0.002, p.attack * 2)
    const d = Math.max(0.01, p.decay * 2)
    const s = p.sustain
    const peak = 0.25 * vel
    amp.gain.cancelScheduledValues(when)
    amp.gain.setValueAtTime(0, when)
    amp.gain.linearRampToValueAtTime(peak, when + a)
    amp.gain.linearRampToValueAtTime(Math.max(0.0001, peak * s), when + a + d)

    osc1.start(when)
    osc2.start(when)
    sub.start(when)

    let released = false
    const release = () => {
      if (released) return
      released = true
      const t = ctx.currentTime
      const r = Math.max(0.02, p.release * 2.5)
      amp.gain.cancelScheduledValues(t)
      amp.gain.setValueAtTime(amp.gain.value, t)
      amp.gain.linearRampToValueAtTime(0, t + r)
      const stopAt = t + r + 0.05
      osc1.stop(stopAt)
      osc2.stop(stopAt)
      sub.stop(stopAt)
    }
    return release
  }
}
