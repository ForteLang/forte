import { useStore } from '../state/store'
import { DEVICE_LABELS } from '../state/devices'
import type { DeviceKind, TrackKind } from '../state/types'

const INSTRUMENTS: DeviceKind[] = ['polymer']
const EFFECTS: DeviceKind[] = ['filter', 'eq', 'drive', 'delay', 'reverb']
const TRACK_KINDS: { kind: TrackKind; label: string }[] = [
  { kind: 'instrument', label: 'Instrument Track' },
  { kind: 'audio', label: 'Audio Track' },
  { kind: 'effect', label: 'Effect Track' },
]

const COLORS: Record<DeviceKind, string> = {
  polymer: '#9a6fd0', filter: '#4fb6c8', delay: '#e08a3c',
  reverb: '#5a8ad0', eq: '#9bcf52', drive: '#e0584f',
}

export function Browser() {
  const addDevice = useStore((s) => s.addDevice)
  const addTrack = useStore((s) => s.addTrack)
  const selectedTrackId = useStore((s) => s.selectedTrackId)

  return (
    <div className="browser">
      <div className="panel-title">Browser</div>

      <div className="cat">Add Track</div>
      {TRACK_KINDS.map((t) => (
        <div key={t.kind} className="item" onClick={() => addTrack(t.kind)}>
          <span className="dot" style={{ background: 'var(--accent)' }} />{t.label}
        </div>
      ))}

      <div className="cat">Instruments</div>
      {INSTRUMENTS.map((k) => (
        <div
          key={k}
          className="item"
          onClick={() => selectedTrackId && addDevice(selectedTrackId, k)}
          title="Add to selected track"
        >
          <span className="dot" style={{ background: COLORS[k] }} />{DEVICE_LABELS[k]}
        </div>
      ))}

      <div className="cat">Audio FX</div>
      {EFFECTS.map((k) => (
        <div
          key={k}
          className="item"
          onClick={() => selectedTrackId && addDevice(selectedTrackId, k)}
          title="Add to selected track"
        >
          <span className="dot" style={{ background: COLORS[k] }} />{DEVICE_LABELS[k]}
        </div>
      ))}

      <div className="cat">Modulators</div>
      <div className="item" style={{ color: 'var(--text-faint)', cursor: 'default' }}>
        <span className="dot" style={{ background: 'var(--play)' }} />LFO (add in device)
      </div>
    </div>
  )
}
