import { useState } from 'react'
import { useStore } from '../state/store'
import { engine } from '../audio/AudioEngine'
import { useRaf } from './useRaf'
import { NOTE_NAMES } from '../state/music'
import type { ScaleName } from '../state/types'

const SCALE_OPTIONS: ScaleName[] = [
  'major', 'minor', 'dorian', 'phrygian', 'lydian', 'mixolydian', 'locrian', 'harmonicMinor', 'chromatic',
]

function formatPos(beat: number, sig: [number, number]): string {
  const [num] = sig
  const bar = Math.floor(beat / num) + 1
  const b = Math.floor(beat % num) + 1
  const sixteenth = Math.floor((beat % 1) * 4) + 1
  return `${bar}.${b}.${sixteenth}`
}

export function TransportBar() {
  const playing = useStore((s) => s.playing)
  const togglePlay = useStore((s) => s.togglePlay)
  const stop = useStore((s) => s.stop)
  const bpm = useStore((s) => s.bpm)
  const setBpm = useStore((s) => s.setBpm)
  const timeSig = useStore((s) => s.timeSig)
  const metronome = useStore((s) => s.metronome)
  const toggleMetronome = useStore((s) => s.toggleMetronome)
  const view = useStore((s) => s.view)
  const setView = useStore((s) => s.setView)
  const key = useStore((s) => s.key)
  const setKeyRoot = useStore((s) => s.setKeyRoot)
  const setScale = useStore((s) => s.setScale)

  const [pos, setPos] = useState('1.1.1')
  useRaf(() => {
    setPos(formatPos(engine.playing ? engine.currentBeat() : 0, timeSig))
  })

  return (
    <div className="transport">
      <div className="logo"><span className="mark" />BITWIG <span style={{ color: 'var(--text-dim)', fontWeight: 400 }}>Studio 6</span></div>
      <div className="sep" />

      <div className="group">
        <button className={`tbtn rec`} title="Record">●</button>
        <button className={`tbtn play${playing ? ' active' : ''}`} onClick={togglePlay} title="Play (Space)">
          {playing ? '⏸' : '▶'}
        </button>
        <button className="tbtn" onClick={stop} title="Stop">■</button>
        <button
          className={`tbtn${metronome ? ' active' : ''}`}
          onClick={toggleMetronome}
          title="Metronome"
        >
          🎵
        </button>
      </div>

      <div className="sep" />
      <div className="pos">{pos}</div>
      <div className="sep" />

      <div className="field">
        <label>Tempo</label>
        <input
          className="bpm-input"
          type="number"
          value={Math.round(bpm)}
          min={20}
          max={300}
          onChange={(e) => setBpm(Number(e.target.value))}
        />
      </div>
      <div className="field">
        <label>Signature</label>
        <div className="val">{timeSig[0]}/{timeSig[1]}</div>
      </div>

      <div className="sep" />
      <div className="field">
        <label>Project Key</label>
        <div style={{ display: 'flex', gap: 4 }}>
          <select value={key.root} onChange={(e) => setKeyRoot(Number(e.target.value))}>
            {NOTE_NAMES.map((n, i) => <option key={n} value={i}>{n}</option>)}
          </select>
          <select value={key.scale} onChange={(e) => setScale(e.target.value as ScaleName)}>
            {SCALE_OPTIONS.map((s) => <option key={s} value={s}>{s}</option>)}
          </select>
        </div>
      </div>

      <div className="spacer" />
      <div className="view-toggle">
        <button className={view === 'arrange' ? 'active' : ''} onClick={() => setView('arrange')}>Arrange</button>
        <button className={view === 'mix' ? 'active' : ''} onClick={() => setView('mix')}>Mix</button>
      </div>
    </div>
  )
}
