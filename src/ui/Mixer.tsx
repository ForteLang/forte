import { useRef, useState } from 'react'
import { useStore } from '../state/store'
import { engine } from '../audio/AudioEngine'
import { useRaf } from './useRaf'
import type { Track } from '../state/types'

function Meter({ trackId, master }: { trackId?: string; master?: boolean }) {
  const [lvl, setLvl] = useState(0)
  useRaf(() => {
    const v = master ? engine.getMasterLevel() : engine.getLevel(trackId!)
    setLvl(v)
  })
  return (
    <div className="meter">
      <div className="meter-fill" style={{ height: `${lvl * 100}%` }} />
    </div>
  )
}

function Fader({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const ref = useRef<HTMLDivElement>(null)
  const onPointer = (e: React.PointerEvent) => {
    if (!(e.buttons & 1) && e.type === 'pointermove') return
    const el = ref.current!
    const rect = el.getBoundingClientRect()
    const v = 1 - (e.clientY - rect.top) / rect.height
    onChange(Math.max(0, Math.min(1, v)))
  }
  return (
    <div
      ref={ref}
      className="fader-track"
      onPointerDown={(e) => { (e.target as HTMLElement).setPointerCapture(e.pointerId); onPointer(e) }}
      onPointerMove={onPointer}
    >
      <div className="fader-fill" style={{ height: `${value * 100}%` }} />
      <div className="fader-handle" style={{ bottom: `calc(${value * 100}% - 4px)` }} />
    </div>
  )
}

function Channel({ track, selected }: { track: Track; selected: boolean }) {
  const setTrackProp = useStore((s) => s.setTrackProp)
  const selectTrack = useStore((s) => s.selectTrack)
  return (
    <div className={`channel${selected ? ' sel' : ''}`} onClick={() => selectTrack(track.id)}>
      <div className="ch-colorbar" style={{ background: track.color }} />
      <div className="ch-name">{track.name}</div>
      <div className="ch-pan">
        <input
          type="range" min={-1} max={1} step={0.01} value={track.pan}
          onChange={(e) => setTrackProp(track.id, { pan: Number(e.target.value) })}
          style={{ width: '100%' }}
        />
        <span className="hint">{track.pan === 0 ? 'C' : track.pan > 0 ? `R${Math.round(track.pan * 100)}` : `L${Math.round(-track.pan * 100)}`}</span>
      </div>
      <div className="fader-area">
        <Fader value={track.volume} onChange={(v) => setTrackProp(track.id, { volume: v })} />
        <Meter trackId={track.id} />
      </div>
      <div className="ch-btns">
        <button className={track.mute ? 'active' : ''} onClick={(e) => { e.stopPropagation(); setTrackProp(track.id, { mute: !track.mute }) }}>M</button>
        <button className={track.solo ? 'active' : ''} onClick={(e) => { e.stopPropagation(); setTrackProp(track.id, { solo: !track.solo }) }}>S</button>
      </div>
    </div>
  )
}

export function Mixer() {
  const tracks = useStore((s) => s.tracks)
  const selectedTrackId = useStore((s) => s.selectedTrackId)
  const [masterVol, setMasterVol] = useState(0.9)

  return (
    <div className="mixer">
      {tracks.map((t) => (
        <Channel key={t.id} track={t} selected={t.id === selectedTrackId} />
      ))}
      <div className="channel master-ch">
        <div className="ch-colorbar" style={{ background: 'var(--accent)' }} />
        <div className="ch-name">Master</div>
        <div className="ch-pan"><span className="hint">Main Out</span></div>
        <div className="fader-area">
          <Fader value={masterVol} onChange={(v) => { setMasterVol(v); engine.setMasterGain(v) }} />
          <Meter master />
        </div>
        <div className="ch-btns"><span className="hint">2.0</span></div>
      </div>
    </div>
  )
}
