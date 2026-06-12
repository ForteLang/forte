/**
 * Insert effect implementations. Each effect exposes an `input` and `output`
 * node and an `update(params)` method so the engine can rebuild the chain and
 * push live parameter changes (including modulated values).
 */

export interface FxNode {
  input: AudioNode
  output: AudioNode
  update(params: Record<string, number>): void
  dispose(): void
}

export function createFilter(ctx: AudioContext): FxNode {
  const f = ctx.createBiquadFilter()
  f.type = 'lowpass'
  return {
    input: f,
    output: f,
    update(p) {
      const type = Math.round(p.type ?? 0)
      f.type = (['lowpass', 'highpass', 'bandpass', 'notch'][type] || 'lowpass') as BiquadFilterType
      f.frequency.value = 60 + Math.pow(Math.min(1, p.cutoff ?? 0.7), 2) * 12000
      f.Q.value = 0.3 + (p.reso ?? 0.2) * 24
    },
    dispose() {},
  }
}

export function createDrive(ctx: AudioContext): FxNode {
  const shaper = ctx.createWaveShaper()
  const pre = ctx.createGain()
  const post = ctx.createGain()
  pre.connect(shaper)
  shaper.connect(post)
  const makeCurve = (amount: number) => {
    const n = 1024
    const curve = new Float32Array(n)
    const k = amount * 100
    for (let i = 0; i < n; i++) {
      const x = (i * 2) / n - 1
      curve[i] = ((1 + k) * x) / (1 + k * Math.abs(x))
    }
    return curve
  }
  return {
    input: pre,
    output: post,
    update(p) {
      const drive = p.drive ?? 0.3
      pre.gain.value = 1 + drive * 4
      shaper.curve = makeCurve(drive)
      post.gain.value = 1 / (1 + drive * 1.5)
    },
    dispose() {},
  }
}

export function createDelay(ctx: AudioContext): FxNode {
  const input = ctx.createGain()
  const delay = ctx.createDelay(2.0)
  const feedback = ctx.createGain()
  const wet = ctx.createGain()
  const dry = ctx.createGain()
  const output = ctx.createGain()
  const tone = ctx.createBiquadFilter()
  tone.type = 'lowpass'
  tone.frequency.value = 4000

  input.connect(dry)
  dry.connect(output)
  input.connect(delay)
  delay.connect(tone)
  tone.connect(feedback)
  feedback.connect(delay)
  tone.connect(wet)
  wet.connect(output)

  return {
    input,
    output,
    update(p) {
      delay.delayTime.value = 0.05 + (p.time ?? 0.3) * 0.9
      feedback.gain.value = Math.min(0.92, (p.feedback ?? 0.35) * 0.95)
      const mix = p.mix ?? 0.3
      wet.gain.value = mix
      dry.gain.value = 1 - mix * 0.5
    },
    dispose() {},
  }
}

export function createReverb(ctx: AudioContext): FxNode {
  const input = ctx.createGain()
  const convolver = ctx.createConvolver()
  const wet = ctx.createGain()
  const dry = ctx.createGain()
  const output = ctx.createGain()

  let curSize = -1
  const buildIR = (size: number, decay: number) => {
    const len = Math.max(0.1, size * 4) * ctx.sampleRate
    const buf = ctx.createBuffer(2, len, ctx.sampleRate)
    for (let ch = 0; ch < 2; ch++) {
      const data = buf.getChannelData(ch)
      for (let i = 0; i < len; i++) {
        data[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 2 + decay * 4)
      }
    }
    convolver.buffer = buf
  }

  input.connect(dry)
  dry.connect(output)
  input.connect(convolver)
  convolver.connect(wet)
  wet.connect(output)

  return {
    input,
    output,
    update(p) {
      const size = p.size ?? 0.5
      const decay = p.decay ?? 0.5
      const rounded = Math.round(size * 10)
      if (rounded !== curSize) {
        curSize = rounded
        buildIR(size, decay)
      }
      const mix = p.mix ?? 0.25
      wet.gain.value = mix
      dry.gain.value = 1 - mix * 0.5
    },
    dispose() {},
  }
}

export function createEQ(ctx: AudioContext): FxNode {
  const low = ctx.createBiquadFilter()
  const mid = ctx.createBiquadFilter()
  const high = ctx.createBiquadFilter()
  low.type = 'lowshelf'
  low.frequency.value = 200
  mid.type = 'peaking'
  mid.frequency.value = 1000
  mid.Q.value = 1
  high.type = 'highshelf'
  high.frequency.value = 4000
  low.connect(mid)
  mid.connect(high)
  return {
    input: low,
    output: high,
    update(p) {
      low.gain.value = ((p.low ?? 0.5) - 0.5) * 30
      mid.gain.value = ((p.mid ?? 0.5) - 0.5) * 30
      high.gain.value = ((p.high ?? 0.5) - 0.5) * 30
    },
    dispose() {},
  }
}
