import { useStore } from '../state/store'
import { NOTE_NAMES } from '../state/music'

export function Inspector() {
  const tracks = useStore((s) => s.tracks)
  const selectedTrackId = useStore((s) => s.selectedTrackId)
  const renameTrack = useStore((s) => s.renameTrack)
  const setTrackProp = useStore((s) => s.setTrackProp)
  const removeTrack = useStore((s) => s.removeTrack)
  const key = useStore((s) => s.key)
  const bpm = useStore((s) => s.bpm)
  const scenes = useStore((s) => s.scenes)
  const track = tracks.find((t) => t.id === selectedTrackId)

  return (
    <div className="inspector">
      <div className="panel-title">Inspector</div>

      <div className="insp-section">
        <div className="insp-row"><label>Project</label></div>
        <div className="insp-row"><label>Tempo</label><span>{Math.round(bpm)} BPM</span></div>
        <div className="insp-row">
          <label>Key</label>
          <span className="key-badge">{NOTE_NAMES[key.root]} {key.scale}</span>
        </div>
        <div className="insp-row"><label>Tracks</label><span>{tracks.length}</span></div>
        <div className="insp-row"><label>Scenes</label><span>{scenes.length}</span></div>
      </div>

      {track && (
        <div className="insp-section">
          <div className="insp-row"><label>Track</label></div>
          <div className="insp-row">
            <input
              type="text"
              value={track.name}
              onChange={(e) => renameTrack(track.id, e.target.value)}
              style={{ width: '100%' }}
            />
          </div>
          <div className="insp-row"><label>Type</label><span>{track.kind}</span></div>
          <div className="insp-row"><label>Color</label>
            <span style={{ width: 16, height: 16, borderRadius: 3, background: track.color }} />
          </div>
          <div className="insp-row"><label>Volume</label><span>{Math.round(track.volume * 100)}%</span></div>
          <div className="insp-row"><label>Pan</label>
            <span>{track.pan === 0 ? 'C' : track.pan > 0 ? `R${Math.round(track.pan*100)}` : `L${Math.round(-track.pan*100)}`}</span>
          </div>
          <div className="insp-row"><label>Devices</label><span>{track.devices.length}</span></div>
          <div className="insp-row" style={{ marginTop: 6 }}>
            <button style={{ width: '100%' }} onClick={() => removeTrack(track.id)}>Delete Track</button>
          </div>
        </div>
      )}

      <div className="insp-section">
        <div className="insp-row"><label>Keyboard</label></div>
        <div style={{ padding: '0 10px', color: 'var(--text-faint)', fontSize: 10, lineHeight: 1.5 }}>
          Play the selected instrument with your computer keys A–L (white) and W,E,T,Y,U (black). Space toggles play.
        </div>
      </div>
    </div>
  )
}
