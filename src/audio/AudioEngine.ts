import type { Track, Device } from '../state/types'
import { Polymer } from './Polymer'
import {
  createFilter, createDelay, createReverb, createEQ, createDrive, FxNode,
} from './devices/effects'

interface TrackNodes {
  instrument: Polymer | null
  fx: { device: Device; node: FxNode }[]
  chainIn: GainNode      // device chain entry (post-instrument)
  gain: GainNode         // volume fader
  panner: StereoPannerNode
  analyser: AnalyserNode
  activeReleases: Set<() => void>
}

export interface TransportState {
  playing: boolean
  beat: number
  bpm: number
}

type SceneTrigger = { trackId: string; sceneIndex: number }

/**
 * The real-time core. Holds a Web Audio graph that mirrors the project's
 * tracks and device chains, runs a look-ahead scheduler for the Arranger and
 * Clip Launcher, and ticks JS-side modulators (LFOs) onto device parameters.
 */
export class AudioEngine {
  ctx: AudioContext | null = null
  private master!: GainNode
  masterAnalyser!: AnalyserNode

  private nodes = new Map<string, TrackNodes>()
  private tracks: Track[] = []

  bpm = 120
  playing = false
  private playStartTime = 0
  private playStartBeat = 0
  private scheduledBeat = 0
  private loopLen = 16 // arranger loop length in beats
  metronome = false

  /** per-track active launcher clip index (scene) or -1 */
  private activeScene = new Map<string, number>()
  /** queued scene changes applied at next bar */
  private queued: SceneTrigger[] = []

  private timer: number | null = null
  private modRaf: number | null = null
  onBeat?: (beat: number) => void

  // ---- lifecycle ---------------------------------------------------------

  ensureContext() {
    if (this.ctx) return
    const Ctor = window.AudioContext || (window as any).webkitAudioContext
    this.ctx = new Ctor()
    this.master = this.ctx.createGain()
    this.master.gain.value = 0.9
    this.masterAnalyser = this.ctx.createAnalyser()
    this.masterAnalyser.fftSize = 1024
    this.master.connect(this.masterAnalyser)
    this.masterAnalyser.connect(this.ctx.destination)
    this.startModLoop()
  }

  resume() {
    this.ensureContext()
    if (this.ctx!.state === 'suspended') this.ctx!.resume()
  }

  // ---- graph sync --------------------------------------------------------

  syncTracks(tracks: Track[]) {
    this.ensureContext()
    this.tracks = tracks
    const ctx = this.ctx!
    const seen = new Set<string>()

    for (const t of tracks) {
      if (t.kind === 'master') continue
      seen.add(t.id)
      let n = this.nodes.get(t.id)
      if (!n) {
        const gain = ctx.createGain()
        const panner = ctx.createStereoPanner()
        const analyser = ctx.createAnalyser()
        analyser.fftSize = 256
        const chainIn = ctx.createGain()
        gain.connect(panner)
        panner.connect(analyser)
        analyser.connect(this.master)
        n = {
          instrument: null,
          fx: [],
          chainIn,
          gain,
          panner,
          analyser,
          activeReleases: new Set(),
        }
        this.nodes.set(t.id, n)
        if (!this.activeScene.has(t.id)) this.activeScene.set(t.id, -1)
      }
      this.rebuildChain(t, n)
      // mixer values
      const anySolo = tracks.some((x) => x.solo)
      const audible = t.solo || (!t.mute && !(anySolo && !t.solo))
      n.gain.gain.value = audible ? Math.pow(t.volume, 1.6) : 0
      n.panner.pan.value = t.pan
    }

    // remove deleted tracks
    for (const id of [...this.nodes.keys()]) {
      if (!seen.has(id)) {
        this.nodes.delete(id)
        this.activeScene.delete(id)
      }
    }
  }

  private makeFx(kind: Device['kind']): FxNode | null {
    const ctx = this.ctx!
    switch (kind) {
      case 'filter': return createFilter(ctx)
      case 'delay': return createDelay(ctx)
      case 'reverb': return createReverb(ctx)
      case 'eq': return createEQ(ctx)
      case 'drive': return createDrive(ctx)
      default: return null
    }
  }

  private rebuildChain(t: Track, n: TrackNodes) {
    const ctx = this.ctx!
    // Instrument
    const instDev = t.devices.find((d) => d.kind === 'polymer')
    if (instDev && !n.instrument) {
      n.instrument = new Polymer(ctx, { ...instDev.params })
    } else if (!instDev && n.instrument) {
      n.instrument = null
    }
    if (instDev && n.instrument) n.instrument.params = { ...instDev.params }

    // Tear down and rebuild fx chain (cheap enough at this scale)
    n.fx.forEach((f) => f.node.dispose())
    n.fx = []
    try { n.chainIn.disconnect() } catch {}

    const fxDevices = t.devices.filter((d) => d.kind !== 'polymer')
    let cursor: AudioNode = n.chainIn
    for (const dev of fxDevices) {
      const node = this.makeFx(dev.kind)
      if (!node) continue
      node.update(this.effectiveParams(dev))
      if (dev.enabled) {
        cursor.connect(node.input)
        cursor = node.output
      }
      n.fx.push({ device: dev, node })
    }
    cursor.connect(n.gain)

    if (n.instrument) {
      try { n.instrument.out.disconnect() } catch {}
      n.instrument.out.connect(n.chainIn)
    }
  }

  /** Resolve a device's params including current modulator offsets. */
  private effectiveParams(dev: Device): Record<string, number> {
    const out = { ...dev.params }
    for (const mod of dev.modulators) {
      const v = this.modValue(mod.id, mod.source, mod.params)
      for (const route of mod.routes) {
        if (route.targetDeviceId !== dev.id) continue
        out[route.targetParam] = clamp01(
          (out[route.targetParam] ?? 0) + v * route.amount,
        )
      }
    }
    return out
  }

  // ---- modulators --------------------------------------------------------

  private modPhase = new Map<string, number>()

  private modValue(id: string, source: string, params: Record<string, number>): number {
    if (source === 'lfo') {
      const phase = this.modPhase.get(id) ?? 0
      const shape = Math.round(params.shape ?? 0)
      // 0 sine, 1 triangle, 2 saw, 3 square
      switch (shape) {
        case 1: return 1 - 4 * Math.abs(((phase) % 1) - 0.5)
        case 2: return (phase % 1) * 2 - 1
        case 3: return (phase % 1) < 0.5 ? 1 : -1
        default: return Math.sin(phase * Math.PI * 2)
      }
    }
    return 0
  }

  private startModLoop() {
    let last = performance.now()
    const tick = () => {
      const now = performance.now()
      const dt = (now - last) / 1000
      last = now
      // advance LFO phases & apply to fx params
      for (const t of this.tracks) {
        const n = this.nodes.get(t.id)
        if (!n) continue
        for (const dev of t.devices) {
          for (const mod of dev.modulators) {
            if (mod.source === 'lfo') {
              const rate = 0.05 + (mod.params.rate ?? 0.3) * 8 // Hz
              const ph = (this.modPhase.get(mod.id) ?? 0) + dt * rate
              this.modPhase.set(mod.id, ph % 1)
            }
          }
        }
        // push effective params into live fx nodes
        for (const f of n.fx) {
          f.node.update(this.effectiveParams(f.device))
        }
        if (n.instrument) {
          const instDev = t.devices.find((d) => d.kind === 'polymer')
          if (instDev) n.instrument.params = { ...instDev.params }
        }
      }
      this.modRaf = requestAnimationFrame(tick)
    }
    this.modRaf = requestAnimationFrame(tick)
  }

  // ---- transport ---------------------------------------------------------

  setBpm(bpm: number) {
    if (this.playing && this.ctx) {
      // re-anchor so beat position is continuous
      this.playStartBeat = this.currentBeat()
      this.playStartTime = this.ctx.currentTime
    }
    this.bpm = bpm
  }

  private bps() { return this.bpm / 60 }

  currentBeat(): number {
    if (!this.playing || !this.ctx) return this.playStartBeat
    return this.playStartBeat + (this.ctx.currentTime - this.playStartTime) * this.bps()
  }

  play(fromBeat = 0, mode: 'arrange' | 'launcher' = 'launcher') {
    this.resume()
    this.playing = true
    this.playMode = mode
    this.playStartTime = this.ctx!.currentTime + 0.05
    this.playStartBeat = fromBeat
    this.scheduledBeat = fromBeat
    if (this.timer == null) {
      this.timer = window.setInterval(() => this.scheduler(), 25)
    }
  }

  private playMode: 'arrange' | 'launcher' = 'launcher'

  stop() {
    this.playing = false
    if (this.timer != null) { clearInterval(this.timer); this.timer = null }
    // release any hanging voices
    for (const n of this.nodes.values()) {
      for (const r of n.activeReleases) r()
      n.activeReleases.clear()
    }
    this.playStartBeat = 0
    this.onBeat?.(0)
  }

  // ---- launcher ----------------------------------------------------------

  triggerSlot(trackId: string, sceneIndex: number) {
    this.activeScene.set(trackId, sceneIndex)
    if (!this.playing) this.play(0, 'launcher')
  }

  stopSlot(trackId: string) {
    this.activeScene.set(trackId, -1)
  }

  triggerScene(sceneIndex: number) {
    for (const t of this.tracks) {
      if (t.kind === 'instrument') this.activeScene.set(t.id, sceneIndex)
    }
    if (!this.playing) this.play(0, 'launcher')
  }

  getActiveScene(trackId: string): number {
    return this.activeScene.get(trackId) ?? -1
  }

  // ---- scheduler ---------------------------------------------------------

  private scheduler() {
    if (!this.playing || !this.ctx) return
    const lookaheadBeats = 0.2 * this.bps() // ~200ms window
    const windowEnd = this.currentBeat() + lookaheadBeats
    const from = this.scheduledBeat

    for (const t of this.tracks) {
      const n = this.nodes.get(t.id)
      if (!n || !n.instrument) continue

      if (this.playMode === 'launcher') {
        const scene = this.activeScene.get(t.id) ?? -1
        if (scene < 0) continue
        const clip = t.slots[scene]?.clip
        if (!clip || clip.notes.length === 0) continue
        this.scheduleLoopingClip(t, n, clip.notes, clip.length, from, windowEnd)
      } else {
        for (const clip of t.arrangerClips) {
          this.scheduleArrangerClip(t, n, clip, from, windowEnd)
        }
      }
    }

    this.scheduledBeat = windowEnd

    // wrap arranger loop
    if (this.playMode === 'arrange' && this.currentBeat() >= this.loopLen) {
      this.play(0, 'arrange')
    }

    this.onBeat?.(this.currentBeat())
  }

  private beatToTime(beat: number): number {
    return this.playStartTime + (beat - this.playStartBeat) / this.bps()
  }

  private fire(n: TrackNodes, pitch: number, vel: number, onsetBeat: number, lenBeats: number) {
    const when = this.beatToTime(onsetBeat)
    const release = n.instrument!.play(pitch, vel, when)
    n.activeReleases.add(release)
    const lenSec = lenBeats / this.bps()
    const delayMs = Math.max(0, (when - this.ctx!.currentTime + lenSec) * 1000)
    window.setTimeout(() => {
      release()
      n.activeReleases.delete(release)
    }, delayMs)
  }

  private scheduleLoopingClip(
    t: Track, n: TrackNodes, notes: { pitch: number; start: number; length: number; velocity: number }[],
    clipLen: number, from: number, to: number,
  ) {
    for (const note of notes) {
      // first repetition index k such that k*clipLen + note.start >= from
      const kStart = Math.ceil((from - note.start) / clipLen)
      const kEnd = Math.floor((to - note.start) / clipLen)
      for (let k = Math.max(0, kStart); k <= kEnd; k++) {
        const onset = k * clipLen + note.start
        if (onset >= from && onset < to) {
          this.fire(n, note.pitch, note.velocity, onset, note.length)
        }
      }
    }
  }

  private scheduleArrangerClip(
    t: Track, n: TrackNodes, clip: { startBeat: number; notes: any[] },
    from: number, to: number,
  ) {
    for (const note of clip.notes) {
      const onset = clip.startBeat + note.start
      if (onset >= from && onset < to) {
        this.fire(n, note.pitch, note.velocity, onset, note.length)
      }
    }
  }

  // ---- live keyboard input ----------------------------------------------

  private liveVoices = new Map<string, () => void>()

  noteOn(trackId: string, pitch: number, velocity = 100) {
    this.resume()
    const n = this.nodes.get(trackId)
    if (!n || !n.instrument) return
    const key = `${trackId}:${pitch}`
    this.liveVoices.get(key)?.()
    const release = n.instrument.play(pitch, velocity, this.ctx!.currentTime)
    this.liveVoices.set(key, release)
  }

  noteOff(trackId: string, pitch: number) {
    const key = `${trackId}:${pitch}`
    this.liveVoices.get(key)?.()
    this.liveVoices.delete(key)
  }

  // ---- metering ----------------------------------------------------------

  getLevel(trackId: string): number {
    const n = this.nodes.get(trackId)
    if (!n) return 0
    return rms(n.analyser)
  }

  getMasterLevel(): number {
    if (!this.masterAnalyser) return 0
    return rms(this.masterAnalyser)
  }

  setMasterGain(v: number) {
    if (this.master) this.master.gain.value = Math.pow(v, 1.6)
  }
}

function rms(analyser: AnalyserNode): number {
  const buf = new Float32Array(analyser.fftSize)
  analyser.getFloatTimeDomainData(buf)
  let sum = 0
  for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i]
  return Math.min(1, Math.sqrt(sum / buf.length) * 2.5)
}

function clamp01(v: number) { return Math.max(0, Math.min(1, v)) }

export const engine = new AudioEngine()
